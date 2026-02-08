use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::tiers::LockTier;

/// A unique staker/validator identity as a 32-byte compressed public key.
pub type PublicKey = [u8; 32];

/// A single staking entry representing a staker's locked position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeEntry {
    /// The staker's public key.
    pub staker: PublicKey,
    /// Amount staked in the smallest token unit.
    pub amount: u64,
    /// Which lock tier this stake is committed to.
    pub lock_tier: LockTier,
    /// Epoch when the stake was created.
    pub start_epoch: u64,
    /// Epoch when the stake can be unlocked (None for Permanent).
    pub unlock_epoch: Option<u64>,
}

/// A delegation from one party to a validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationEntry {
    /// The delegator's public key.
    pub delegator: PublicKey,
    /// The validator being delegated to.
    pub validator: PublicKey,
    /// Amount delegated.
    pub amount: u64,
    /// Lock tier chosen by the delegator.
    pub lock_tier: LockTier,
    /// Epoch when the delegation started.
    pub start_epoch: u64,
    /// Epoch when the delegation can be undelegated (None for Permanent).
    pub unlock_epoch: Option<u64>,
}

/// Key for looking up delegations: (delegator, validator).
pub type DelegationKey = (PublicKey, PublicKey);

/// The aggregate staking state for the whole chain.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StakingState {
    /// Total amount staked across all entries.
    pub total_staked: u64,
    /// All stake entries keyed by staker pubkey.
    pub entries: HashMap<PublicKey, Vec<StakeEntry>>,
    /// All delegation entries keyed by (delegator, validator).
    pub delegations: HashMap<DelegationKey, Vec<DelegationEntry>>,
    /// Current epoch for time tracking.
    pub current_epoch: u64,
}

/// Errors that can occur during staking operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StakingError {
    #[error("insufficient stake balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },

    #[error("stake is still locked until epoch {unlock_epoch}, current epoch is {current_epoch}")]
    StillLocked {
        unlock_epoch: u64,
        current_epoch: u64,
    },

    #[error("permanent stakes cannot be unstaked")]
    PermanentLock,

    #[error("no stake found for the given pubkey")]
    NoStakeFound,

    #[error("no delegation found for the given delegator/validator pair")]
    NoDelegationFound,

    #[error("amount must be greater than zero")]
    ZeroAmount,

    #[error("arithmetic overflow")]
    Overflow,
}
