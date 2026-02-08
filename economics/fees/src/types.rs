use serde::{Deserialize, Serialize};

/// Configuration for the EIP-1559 fee market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    /// Target gas usage per block (equilibrium point).
    pub target_gas_per_block: u64,
    /// Maximum gas allowed per block.
    pub max_gas_per_block: u64,
    /// Minimum base fee (floor) -- never goes below this.
    pub base_fee_floor: u64,
    /// Elasticity multiplier (denominator for fee adjustment).
    /// The standard EIP-1559 value is 8, giving 12.5% max adjustment.
    pub elasticity_multiplier: u64,
    /// Fee split ratios in basis points (must sum to 10_000).
    pub split: SplitConfig,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            target_gas_per_block: 15_000_000,
            max_gas_per_block: 30_000_000,
            base_fee_floor: 1,
            elasticity_multiplier: 8,
            split: SplitConfig::default(),
        }
    }
}

/// Split configuration as basis points (100 bps = 1%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitConfig {
    /// Percentage to burn (default 40% = 4000 bps).
    pub burn_bps: u64,
    /// Percentage to validator (default 30% = 3000 bps).
    pub validator_bps: u64,
    /// Percentage to treasury (default 20% = 2000 bps).
    pub treasury_bps: u64,
    /// Percentage to developer (default 10% = 1000 bps).
    pub developer_bps: u64,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            burn_bps: 4000,
            validator_bps: 3000,
            treasury_bps: 2000,
            developer_bps: 1000,
        }
    }
}

impl SplitConfig {
    /// Validate that split ratios sum to 10_000 bps (100%).
    pub fn validate(&self) -> bool {
        self.burn_bps + self.validator_bps + self.treasury_bps + self.developer_bps == 10_000
    }
}

/// A calculated transaction fee.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionFee {
    /// The base fee component (burned/split).
    pub base_fee: u64,
    /// The priority fee (tip to validator).
    pub priority_fee: u64,
    /// Total fee = base_fee + priority_fee.
    pub total: u64,
}

/// Result of splitting a fee into 4 destinations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SplitResult {
    /// Amount to burn (removed from circulation).
    pub burn: u64,
    /// Amount to the block validator.
    pub validator: u64,
    /// Amount to the protocol treasury.
    pub treasury: u64,
    /// Amount to the contract developer.
    pub developer: u64,
}

/// Errors that can occur during fee operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FeeError {
    #[error("split ratios must sum to 10000 bps, got {0}")]
    InvalidSplitRatios(u64),

    #[error("gas exceeds block maximum: {used} > {max}")]
    GasExceedsMax { used: u64, max: u64 },

    #[error("elasticity multiplier must be > 0")]
    ZeroElasticity,

    #[error("arithmetic overflow")]
    Overflow,
}
