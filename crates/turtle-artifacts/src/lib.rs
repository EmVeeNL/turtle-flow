//! Content-addressed blob store and canonical JSON (`docs/adr/ADR-013`).
//!
//! Blobs are immutable files named by their SHA-256. Writes are atomic (temporary file, then
//! rename), directories are `0700` and files `0600` (SEC-013), and every read re-hashes the file,
//! so a corrupted or tampered blob is detected instead of silently used.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use turtle_domain::Sha256 as Digest256;

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("blob {0} not found")]
    NotFound(Digest256),
    #[error("blob {expected} is corrupted: its content hashes to {actual}")]
    Corrupted {
        expected: Digest256,
        actual: Digest256,
    },
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Opens (creating if needed) a store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, BlobError> {
        let root = root.into();
        create_private_dir(&root)?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_of(&self, hash: &Digest256) -> PathBuf {
        let hex = hash.to_hex();
        self.root
            .join("sha256")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }

    /// Stores `bytes` and returns their hash. Storing the same content again is a no-op.
    pub fn put(&self, bytes: &[u8]) -> Result<Digest256, BlobError> {
        let hash = Digest256::from_bytes(Sha256::digest(bytes).into());
        let path = self.path_of(&hash);
        if path.exists() {
            return Ok(hash);
        }
        let dir = path.parent().expect("blob paths have a parent");
        create_private_dir(dir)?;
        let tmp = dir.join(format!(".tmp-{}", ulid::Ulid::generate()));
        {
            let mut f = open_private(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        // Rename is atomic on the same filesystem; a concurrent writer of the same content
        // produces an identical file, so losing the race is harmless.
        std::fs::rename(&tmp, &path)?;
        Ok(hash)
    }

    /// Stores the canonical JSON form of `value`.
    pub fn put_json<T: Serialize>(&self, value: &T) -> Result<Digest256, BlobError> {
        self.put(canonical_json(value)?.as_bytes())
    }

    /// Reads a blob and verifies its hash.
    pub fn get(&self, hash: &Digest256) -> Result<Vec<u8>, BlobError> {
        let bytes = match std::fs::read(self.path_of(hash)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobError::NotFound(*hash));
            }
            Err(e) => return Err(e.into()),
        };
        let actual = Digest256::from_bytes(Sha256::digest(&bytes).into());
        if actual == *hash {
            Ok(bytes)
        } else {
            Err(BlobError::Corrupted {
                expected: *hash,
                actual,
            })
        }
    }

    pub fn exists(&self, hash: &Digest256) -> bool {
        self.path_of(hash).exists()
    }
}

/// JSON with object keys sorted and no insignificant whitespace, so equal documents always hash
/// equally. Floating-point numbers are not used in stored documents.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    // `serde_json::Value` objects are sorted maps (no `preserve_order` feature).
    serde_json::to_string(&serde_json::to_value(value)?)
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let s = BlobStore::open(dir.path().join("blobs")).expect("open");
        (dir, s)
    }

    #[test]
    fn put_get_round_trip_and_dedup() {
        let (_d, s) = store();
        let h = s.put(b"hello").expect("put");
        assert_eq!(h, s.put(b"hello").expect("put again"));
        assert_eq!(s.get(&h).expect("get"), b"hello");
        assert!(s.exists(&h));
        assert_ne!(h, s.put(b"hello!").expect("put other"));
        // Known SHA-256 of "hello".
        assert_eq!(
            h.to_hex(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn missing_blob_is_not_found() {
        let (_d, s) = store();
        let h = Digest256::from_bytes([7; 32]);
        assert!(matches!(s.get(&h), Err(BlobError::NotFound(_))));
        assert!(!s.exists(&h));
    }

    #[test]
    fn tampering_is_detected_on_read() {
        let (_d, s) = store();
        let h = s.put(b"original").expect("put");
        let path = s.path_of(&h);
        std::fs::write(&path, b"forged").expect("tamper");
        assert!(matches!(s.get(&h), Err(BlobError::Corrupted { .. })));
    }

    #[test]
    fn private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, s) = store();
        let h = s.put(b"secret-ish").expect("put");
        let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(&s.path_of(&h)), 0o600);
        assert_eq!(mode(s.root()), 0o700);
        assert_eq!(mode(s.path_of(&h).parent().expect("parent")), 0o700);
    }

    #[test]
    fn canonical_json_is_order_independent() {
        let a = canonical_json(&json!({"b": 1, "a": {"y": 2, "x": 1}})).expect("a");
        let b: serde_json::Value =
            serde_json::from_str(r#"{"a":{"x":1,"y":2},"b":1}"#).expect("json");
        assert_eq!(a, canonical_json(&b).expect("b"));
        assert_eq!(a, r#"{"a":{"x":1,"y":2},"b":1}"#);
        let (_d, s) = store();
        assert_eq!(
            s.put_json(&json!({"b": 1, "a": 2})).expect("put"),
            s.put_json(&json!({"a": 2, "b": 1})).expect("put")
        );
    }

    #[test]
    fn leaves_no_temporary_files_behind() {
        let (_d, s) = store();
        let h = s.put(b"x").expect("put");
        let dir = s.path_of(&h).parent().expect("parent").to_path_buf();
        let leftovers = std::fs::read_dir(dir)
            .expect("read")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
            .count();
        assert_eq!(leftovers, 0);
    }
}
