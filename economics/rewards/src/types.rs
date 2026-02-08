use serde::{Deserialize, Serialize};

/// A unique identity as a 32-byte compressed public key.
pub type PublicKey = [u8; 32];

/// A unique contract address (32-byte hash).
pub type ContractAddress = [u8; 32];

/// Registry entry for a deployed contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractRegistry {
    /// The contract's address.
    pub contract_address: ContractAddress,
    /// The deployer's public key.
    pub deployer: PublicKey,
    /// Block height at which the contract was deployed.
    pub deploy_height: u64,
    /// Total fees earned by this contract (accumulated).
    pub total_fees_earned: u64,
}

/// A reward distribution event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RewardEvent {
    /// The contract that generated the fees.
    pub contract: ContractAddress,
    /// The developer receiving the reward.
    pub developer: PublicKey,
    /// Amount distributed.
    pub amount: u64,
    /// Block height at which the distribution occurred.
    pub height: u64,
}

/// Errors that can occur during developer reward operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RewardsError {
    #[error("contract already registered: {0:?}")]
    AlreadyRegistered(ContractAddress),

    #[error("contract not found: {0:?}")]
    ContractNotFound(ContractAddress),

    #[error("fee amount must be greater than zero")]
    ZeroFeeAmount,

    #[error("arithmetic overflow")]
    Overflow,
}
