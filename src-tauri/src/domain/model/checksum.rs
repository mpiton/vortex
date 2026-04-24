//! Checksum algorithm and parsing helpers.
//!
//! Pure domain types. Used by checksum validation flow to detect the algorithm
//! from the expected hash format, persist the algorithm/computed pair, and
//! report mismatches via domain events.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChecksumAlgorithm {
    Sha256,
    Md5,
}

impl ChecksumAlgorithm {
    /// Detect the algorithm from the expected hash string.
    ///
    /// Recognises hex-encoded SHA-256 (64 chars) and MD5 (32 chars). Returns
    /// `None` for any other length or for non-hex characters.
    pub fn detect_from_hex(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        match trimmed.len() {
            64 => Some(ChecksumAlgorithm::Sha256),
            32 => Some(ChecksumAlgorithm::Md5),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ChecksumAlgorithm::Sha256 => "SHA-256",
            ChecksumAlgorithm::Md5 => "MD5",
        }
    }
}

impl std::fmt::Display for ChecksumAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ChecksumAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "SHA-256" | "SHA256" => Ok(ChecksumAlgorithm::Sha256),
            "MD5" => Ok(ChecksumAlgorithm::Md5),
            other => Err(format!("unknown checksum algorithm: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_sha256_from_64_hex_chars() {
        let sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            ChecksumAlgorithm::detect_from_hex(sha256),
            Some(ChecksumAlgorithm::Sha256)
        );
    }

    #[test]
    fn test_detect_md5_from_32_hex_chars() {
        let md5 = "d41d8cd98f00b204e9800998ecf8427e";
        assert_eq!(
            ChecksumAlgorithm::detect_from_hex(md5),
            Some(ChecksumAlgorithm::Md5)
        );
    }

    #[test]
    fn test_detect_returns_none_for_unsupported_length() {
        assert_eq!(ChecksumAlgorithm::detect_from_hex("abc"), None);
        assert_eq!(
            ChecksumAlgorithm::detect_from_hex(&"a".repeat(40)),
            None,
            "SHA-1 (40 chars) is not supported"
        );
    }

    #[test]
    fn test_detect_returns_none_for_non_hex() {
        let not_hex = "z".repeat(32);
        assert_eq!(ChecksumAlgorithm::detect_from_hex(&not_hex), None);
    }

    #[test]
    fn test_detect_trims_whitespace() {
        let padded = format!("  {}  ", "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            ChecksumAlgorithm::detect_from_hex(&padded),
            Some(ChecksumAlgorithm::Md5)
        );
    }

    #[test]
    fn test_display_renders_canonical_label() {
        assert_eq!(ChecksumAlgorithm::Sha256.to_string(), "SHA-256");
        assert_eq!(ChecksumAlgorithm::Md5.to_string(), "MD5");
    }

    #[test]
    fn test_from_str_parses_canonical_and_aliases() {
        use std::str::FromStr;
        assert_eq!(
            ChecksumAlgorithm::from_str("SHA-256").unwrap(),
            ChecksumAlgorithm::Sha256
        );
        assert_eq!(
            ChecksumAlgorithm::from_str("sha256").unwrap(),
            ChecksumAlgorithm::Sha256
        );
        assert_eq!(
            ChecksumAlgorithm::from_str("md5").unwrap(),
            ChecksumAlgorithm::Md5
        );
        assert!(ChecksumAlgorithm::from_str("crc32").is_err());
    }
}
