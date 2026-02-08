use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A 32-byte compressed Ed25519 public key used as validator identity.
pub type PublicKey = [u8; 32];

/// The operational status of a validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValidatorStatus {
    /// Actively participating in consensus and earning rewards.
    Active,
    /// Registered but waiting for a rotation opportunity.
    Standby,
    /// Penalized and excluded from consensus and rotation.
    Jailed,
}

/// Information about a single validator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// The validator's Ed25519 public key.
    pub pubkey: PublicKey,
    /// Raw stake amount (smallest token unit).
    pub stake: u64,
    /// Commission rate in basis points (e.g., 500 = 5%).
    pub commission_rate: u16,
    /// Current operational status.
    pub status: ValidatorStatus,
    /// Performance score (0-10000 bps). Used for slashing/reward adjustments.
    pub performance_score: u16,
    /// Block height at which this validator joined.
    pub join_height: u64,
}

impl ValidatorInfo {
    /// Compute the effective stake used for ranking, incorporating vote weight.
    /// `vote_weight_bps` comes from the staking tier (1000 = 1.0x).
    pub fn effective_stake(&self, vote_weight_bps: u64) -> u64 {
        ((self.stake as u128) * (vote_weight_bps as u128) / 1000) as u64
    }
}

/// Configuration for the validator set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSetConfig {
    /// Maximum number of active validators (default: 200).
    pub active_set_cap: usize,
    /// Number of blocks per epoch (rotation happens at epoch boundaries).
    pub epoch_length: u64,
    /// Minimum stake required to register as a validator.
    pub min_stake: u64,
}

impl Default for ValidatorSetConfig {
    fn default() -> Self {
        Self {
            active_set_cap: 200,
            epoch_length: 100,
            min_stake: 1_000_000,
        }
    }
}

/// The complete state of the validator set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidatorSetState {
    /// All registered validators keyed by public key.
    pub validators: HashMap<PublicKey, ValidatorInfo>,
    /// Configuration.
    pub config: ValidatorSetConfig,
    /// Current epoch number.
    pub current_epoch: u64,
}


/// Errors produced by the validator set module.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidatorSetError {
    #[error("validator already registered: {0:?}")]
    AlreadyRegistered(PublicKey),

    #[error("validator not found: {0:?}")]
    NotFound(PublicKey),

    #[error("insufficient stake: have {have}, need {need}")]
    InsufficientStake { have: u64, need: u64 },

    #[error("validator is jailed and cannot be activated")]
    Jailed,

    #[error("validator is not jailed")]
    NotJailed,

    #[error("validator is already active")]
    AlreadyActive,

    #[error("validator is already standby")]
    AlreadyStandby,

    #[error("arithmetic overflow")]
    Overflow,
}

pub type ValidatorSetResult<T> = Result<T, ValidatorSetError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn pubkey(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    #[test]
    fn effective_stake_with_vote_weight() {
        let info = ValidatorInfo {
            pubkey: pubkey(1),
            stake: 1_000_000,
            commission_rate: 500,
            status: ValidatorStatus::Active,
            performance_score: 10_000,
            join_height: 0,
        };
        // 1x vote weight
        assert_eq!(info.effective_stake(1000), 1_000_000);
        // 1.5x vote weight
        assert_eq!(info.effective_stake(1500), 1_500_000);
        // 5x vote weight
        assert_eq!(info.effective_stake(5000), 5_000_000);
    }

    #[test]
    fn default_config() {
        let cfg = ValidatorSetConfig::default();
        assert_eq!(cfg.active_set_cap, 200);
        assert_eq!(cfg.epoch_length, 100);
        assert_eq!(cfg.min_stake, 1_000_000);
    }

    #[test]
    fn validator_status_equality() {
        assert_eq!(ValidatorStatus::Active, ValidatorStatus::Active);
        assert_ne!(ValidatorStatus::Active, ValidatorStatus::Jailed);
    }
}
