//! Installation-local approval signing key. Key material never enters the database or logs.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::error::{Result, StoreError};

#[derive(Clone)]
pub(crate) struct Key(pub(crate) [u8; 32]);

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key([redacted])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStorage {
    OsKeychain,
    FileFallback,
    Ephemeral,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_hex(value: &str) -> Result<Key> {
    let value = value.trim();
    if value.len() != 64 {
        return Err(StoreError::Corrupt("invalid approval key length".into()));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| StoreError::Corrupt("invalid approval key encoding".into()))?;
    }
    Ok(Key(bytes))
}

fn fresh() -> Result<Key> {
    let mut bytes = [0; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(Key(bytes))
}

fn keychain_read(id: &str) -> Option<Key> {
    let output = if cfg!(target_os = "macos") {
        Command::new("security")
            .args([
                "find-generic-password",
                "-a",
                id,
                "-s",
                "turtle-flow-approval",
                "-w",
            ])
            .output()
            .ok()?
    } else if cfg!(target_os = "linux") {
        Command::new("secret-tool")
            .args([
                "lookup",
                "service",
                "turtle-flow-approval",
                "installation",
                id,
            ])
            .output()
            .ok()?
    } else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    parse_hex(&String::from_utf8(output.stdout).ok()?).ok()
}

fn keychain_write(id: &str, key: &Key) -> bool {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut cmd = Command::new("security");
        cmd.args([
            "add-generic-password",
            "-a",
            id,
            "-s",
            "turtle-flow-approval",
            "-w",
        ]);
        cmd
    } else if cfg!(target_os = "linux") {
        let mut cmd = Command::new("secret-tool");
        cmd.args([
            "store",
            "--label=Turtle Flow approval key",
            "service",
            "turtle-flow-approval",
            "installation",
            id,
        ]);
        cmd
    } else {
        return false;
    };
    let Ok(mut child) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let Some(mut stdin) = child.stdin.take() else {
        return false;
    };
    if stdin.write_all(hex(&key.0).as_bytes()).is_err() {
        return false;
    }
    drop(stdin);
    child.wait().is_ok_and(|status| status.success())
}

pub(crate) fn load(
    path: Option<&Path>,
    id: &str,
    signed_rows: bool,
) -> Result<(Key, KeyStorage, bool)> {
    let Some(db_path) = path else {
        return Ok((fresh()?, KeyStorage::Ephemeral, true));
    };
    let file = db_path.with_extension("approval-key");
    if let Some(key) = keychain_read(id) {
        return Ok((key, KeyStorage::OsKeychain, false));
    }
    if file.exists() {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&file)?.permissions().mode() & 0o077 != 0 {
            return Err(StoreError::Corrupt(
                "approval key file has unsafe permissions".into(),
            ));
        }
        return Ok((
            parse_hex(&std::fs::read_to_string(file)?)?,
            KeyStorage::FileFallback,
            false,
        ));
    }
    if signed_rows {
        return Err(StoreError::Corrupt(
            "approval signing key is unavailable".into(),
        ));
    }
    let key = fresh()?;
    if keychain_write(id, &key) {
        return Ok((key, KeyStorage::OsKeychain, true));
    }
    use std::os::unix::fs::OpenOptionsExt;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)?;
    output.write_all(hex(&key.0).as_bytes())?;
    output.sync_all()?;
    Ok((key, KeyStorage::FileFallback, true))
}

pub(crate) fn mac(key: &Key, message: &[u8]) -> String {
    let mut block = [0; 64];
    block[..32].copy_from_slice(&key.0);
    let mut inner = block;
    let mut outer = block;
    for byte in &mut inner {
        *byte ^= 0x36;
    }
    for byte in &mut outer {
        *byte ^= 0x5c;
    }
    let digest = Sha256::digest([inner.as_slice(), message].concat());
    hex(&Sha256::digest(
        [outer.as_slice(), digest.as_slice()].concat(),
    ))
}

pub(crate) fn matches(expected: &str, actual: &str) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    expected
        .bytes()
        .zip(actual.bytes())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_an_independent_sha256_vector() {
        let mut key = [0; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = index as u8;
        }
        assert_eq!(
            mac(&Key(key), b"Turtle Flow approval row"),
            "54e4ee147a90eab336cad34cf9a22c589f92cbe55cc715b4a3c9cba3cfed029a"
        );
    }
}
