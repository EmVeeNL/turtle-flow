//! SHA-256 digest value object (hashing itself happens in IO crates).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A SHA-256 digest, displayed as `sha256:<64 lowercase hex chars>`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256([u8; 32]);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HashParseError {
    #[error("expected 64 hex characters (optionally prefixed with `sha256:`), got {0}")]
    Length(usize),
    #[error("invalid hex character in digest")]
    InvalidHex,
}

impl Sha256 {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex without prefix.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Short form for UI: first 4 and last 4 hex characters, e.g. `91af…3c07`.
    #[must_use]
    pub fn short(&self) -> String {
        let hex = self.to_hex();
        format!("{}…{}", &hex[..4], &hex[60..])
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:{}", self.to_hex())
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({})", self.short())
    }
}

impl FromStr for Sha256 {
    type Err = HashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix("sha256:").unwrap_or(s);
        if hex.len() != 64 {
            return Err(HashParseError::Length(hex.len()));
        }
        let mut out = [0u8; 32];
        for (i, chunk) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let pair = std::str::from_utf8(chunk).map_err(|_| HashParseError::InvalidHex)?;
            out[i] = u8::from_str_radix(pair, 16).map_err(|_| HashParseError::InvalidHex)?;
        }
        Ok(Self(out))
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "91af000000000000000000000000000000000000000000000000000000003c07";

    #[test]
    fn parses_with_and_without_prefix() {
        let a: Sha256 = HEX.parse().expect("bare hex");
        let b: Sha256 = format!("sha256:{HEX}").parse().expect("prefixed");
        assert_eq!(a, b);
        assert_eq!(a.to_string(), format!("sha256:{HEX}"));
        assert_eq!(a.short(), "91af…3c07");
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!("abc".parse::<Sha256>(), Err(HashParseError::Length(3)));
        let bad = "zz".repeat(32);
        assert_eq!(bad.parse::<Sha256>(), Err(HashParseError::InvalidHex));
    }

    #[test]
    fn serde_round_trip() {
        let a: Sha256 = HEX.parse().expect("valid");
        let json = serde_json::to_string(&a).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Sha256>(&json).expect("deserialize"),
            a
        );
    }
}
