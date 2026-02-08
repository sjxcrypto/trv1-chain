use serde::{Deserialize, Serialize};

/// A 32-byte compressed Ed25519 public key.
pub type PublicKey = [u8; 32];

/// Types of slashable offenses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlashingOffense {
    /// Validator signed two different blocks at the same height/round.
    DoubleSign,
    /// Validator missed too many consecutive blocks.
    Downtime,
    /// Validator proposed an invalid block.
    InvalidBlock,
}

impl std::fmt::Display for SlashingOffense {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlashingOffense::DoubleSign => write!(f, "DoubleSign"),
            SlashingOffense::Downtime => write!(f, "Downtime"),
            SlashingOffense::InvalidBlock => write!(f, "InvalidBlock"),
        }
    }
}

/// A record of a slashing event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashEvent {
    /// The offending validator's public key.
    pub offender: PublicKey,
    /// The type of offense committed.
    pub offense: SlashingOffense,
    /// The amount of stake that was slashed.
    pub slash_amount: u64,
    /// The block height at which the offense occurred.
    pub height: u64,
    /// SHA-256 hash of the evidence that triggered this slash.
    pub evidence_hash: [u8; 32],
}

/// Configuration for slash percentages and thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingConfig {
    /// Slash percentage for double-signing in basis points (500 = 5%).
    pub double_sign_slash_bps: u64,
    /// Slash percentage for downtime in basis points (100 = 1%).
    pub downtime_slash_bps: u64,
    /// Slash percentage for invalid block in basis points (1000 = 10%).
    pub invalid_block_slash_bps: u64,
    /// Number of consecutive missed blocks before downtime slash triggers.
    pub downtime_threshold: u64,
}

impl Default for SlashingConfig {
    fn default() -> Self {
        Self {
            double_sign_slash_bps: 500,  // 5%
            downtime_slash_bps: 100,     // 1%
            invalid_block_slash_bps: 1000, // 10%
            downtime_threshold: 100,
        }
    }
}

impl SlashingConfig {
    /// Get the slash percentage in basis points for a given offense.
    pub fn slash_bps(&self, offense: &SlashingOffense) -> u64 {
        match offense {
            SlashingOffense::DoubleSign => self.double_sign_slash_bps,
            SlashingOffense::Downtime => self.downtime_slash_bps,
            SlashingOffense::InvalidBlock => self.invalid_block_slash_bps,
        }
    }
}

/// A piece of evidence submitted to prove validator misbehavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    /// The type of offense.
    pub offense: SlashingOffense,
    /// The offending validator's public key.
    pub offender: PublicKey,
    /// Block height at which the offense occurred.
    pub height: u64,
    /// Raw evidence data (serialized votes, block hashes, etc.).
    pub data: Vec<u8>,
    /// Whether this evidence has been processed.
    pub processed: bool,
}

impl EvidenceRecord {
    /// Compute a deterministic 32-byte hash of this evidence record.
    /// Uses a simple hash based on serializing the key fields.
    pub fn hash(&self) -> [u8; 32] {
        // Build a deterministic byte representation.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.offender);
        bytes.extend_from_slice(&self.height.to_le_bytes());
        bytes.extend_from_slice(&self.data);
        bytes.push(self.offense as u8);

        // Use ed25519_dalek's sha2 dependency indirectly via serde_json for hashing.
        // Simple FNV-like hash spread across 32 bytes for deduplication.
        let json = serde_json::to_vec(&bytes).unwrap_or_default();
        let mut out = [0u8; 32];
        // Simple deterministic hash: fold the serialized bytes.
        for (i, &b) in json.iter().enumerate() {
            out[i % 32] ^= b;
            // Mix bits.
            out[(i + 1) % 32] = out[(i + 1) % 32].wrapping_add(b.wrapping_mul(31));
        }
        out
    }
}

/// Errors from the slashing module.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SlashingError {
    #[error("validator not found: {0:?}")]
    ValidatorNotFound(PublicKey),

    #[error("duplicate evidence already submitted")]
    DuplicateEvidence,

    #[error("evidence is invalid: {0}")]
    InvalidEvidence(String),

    #[error("validator already jailed")]
    AlreadyJailed,

    #[error("validator set error: {0}")]
    ValidatorSetError(String),
}

pub type SlashingResult<T> = Result<T, SlashingError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn pubkey(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    #[test]
    fn default_config_percentages() {
        let cfg = SlashingConfig::default();
        assert_eq!(cfg.slash_bps(&SlashingOffense::DoubleSign), 500);
        assert_eq!(cfg.slash_bps(&SlashingOffense::Downtime), 100);
        assert_eq!(cfg.slash_bps(&SlashingOffense::InvalidBlock), 1000);
    }

    #[test]
    fn evidence_hash_deterministic() {
        let evidence = EvidenceRecord {
            offense: SlashingOffense::DoubleSign,
            offender: pubkey(1),
            height: 42,
            data: b"conflicting_vote_data".to_vec(),
            processed: false,
        };
        let h1 = evidence.hash();
        let h2 = evidence.hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn offense_display() {
        assert_eq!(SlashingOffense::DoubleSign.to_string(), "DoubleSign");
        assert_eq!(SlashingOffense::Downtime.to_string(), "Downtime");
        assert_eq!(SlashingOffense::InvalidBlock.to_string(), "InvalidBlock");
    }
}
