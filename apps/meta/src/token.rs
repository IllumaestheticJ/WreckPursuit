//! Random tokens and their at-rest hashes. Tokens are only ever stored as SHA-256 hex.

use sha2::{Digest, Sha256};

/// Generate a random token with a readable prefix, e.g. `wps_` + 64 hex chars (256 bits).
pub fn generate(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS random source available");
    format!("{prefix}{}", hex::encode(bytes))
}

/// Short human-typeable code (party invites): 8 chars from an unambiguous alphabet.
pub fn short_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("OS random source available");
    bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

/// SHA-256 hex of a token (what the database stores).
pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Token prefixes, so a leaked token says what it is.
pub mod prefix {
    /// Player session token.
    pub const SESSION: &str = "wps_";
    /// One-time instance join token.
    pub const JOIN: &str = "wpj_";
    /// Reconnect token.
    pub const RECONNECT: &str = "wpr_";
    /// Instance credential.
    pub const INSTANCE: &str = "wpi_";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_hash_stably() {
        let a = generate(prefix::SESSION);
        let b = generate(prefix::SESSION);
        assert_ne!(a, b);
        assert!(a.starts_with("wps_") && a.len() == 4 + 64);
        assert_eq!(hash(&a), hash(&a));
        assert_ne!(hash(&a), hash(&b));
        assert_eq!(short_code().len(), 8);
    }
}
