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

/// Individual fee split ratios in basis points (100 bps = 1%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitRatios {
    pub burn_bps: u64,
    pub validator_bps: u64,
    pub treasury_bps: u64,
    pub developer_bps: u64,
}

impl SplitRatios {
    /// Validate that ratios sum to 10_000 bps (100%).
    pub fn validate(&self) -> bool {
        self.burn_bps + self.validator_bps + self.treasury_bps + self.developer_bps == 10_000
    }
}

/// Configuration for transitioning fee splits from launch to maturity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitConfig {
    /// Fee split ratios at launch (epoch 0).
    pub launch: SplitRatios,
    /// Fee split ratios at maturity (after transition_epochs).
    pub maturity: SplitRatios,
    /// Number of epochs over which to linearly transition from launch to maturity.
    pub transition_epochs: u64,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            launch: SplitRatios {
                burn_bps: 1000,
                validator_bps: 0,
                treasury_bps: 4500,
                developer_bps: 4500,
            },
            maturity: SplitRatios {
                burn_bps: 2500,
                validator_bps: 2500,
                treasury_bps: 2500,
                developer_bps: 2500,
            },
            transition_epochs: 1825,
        }
    }
}

impl SplitConfig {
    /// Validate that both launch and maturity ratios sum to 10_000 bps.
    pub fn validate(&self) -> bool {
        self.launch.validate() && self.maturity.validate()
    }

    /// Get the interpolated split ratios at a given epoch.
    /// Uses linear interpolation from launch to maturity over transition_epochs.
    /// After transition_epochs, returns maturity ratios.
    pub fn split_at_epoch(&self, epoch: u64) -> SplitRatios {
        if epoch >= self.transition_epochs {
            return self.maturity.clone();
        }

        let interp = |launch_val: u64, maturity_val: u64| -> u64 {
            let l = launch_val as i64;
            let m = maturity_val as i64;
            let result = l + (m - l) * epoch as i64 / self.transition_epochs as i64;
            result as u64
        };

        let validator_bps = interp(self.launch.validator_bps, self.maturity.validator_bps);
        let treasury_bps = interp(self.launch.treasury_bps, self.maturity.treasury_bps);
        let developer_bps = interp(self.launch.developer_bps, self.maturity.developer_bps);
        // Burn absorbs rounding remainder
        let burn_bps = 10_000 - validator_bps - treasury_bps - developer_bps;

        SplitRatios {
            burn_bps,
            validator_bps,
            treasury_bps,
            developer_bps,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_split_config_validates() {
        assert!(SplitConfig::default().validate());
    }

    #[test]
    fn split_at_epoch_zero_returns_launch() {
        let config = SplitConfig::default();
        let ratios = config.split_at_epoch(0);
        assert_eq!(ratios.burn_bps, 1000);
        assert_eq!(ratios.validator_bps, 0);
        assert_eq!(ratios.treasury_bps, 4500);
        assert_eq!(ratios.developer_bps, 4500);
    }

    #[test]
    fn split_at_epoch_maturity_returns_maturity() {
        let config = SplitConfig::default();
        // Exactly at transition_epochs
        let ratios = config.split_at_epoch(1825);
        assert_eq!(ratios.burn_bps, 2500);
        assert_eq!(ratios.validator_bps, 2500);
        assert_eq!(ratios.treasury_bps, 2500);
        assert_eq!(ratios.developer_bps, 2500);

        // Well past maturity
        let ratios = config.split_at_epoch(10_000);
        assert_eq!(ratios.burn_bps, 2500);
        assert_eq!(ratios.validator_bps, 2500);
        assert_eq!(ratios.treasury_bps, 2500);
        assert_eq!(ratios.developer_bps, 2500);
    }

    #[test]
    fn split_at_epoch_midpoint() {
        let config = SplitConfig::default();
        let ratios = config.split_at_epoch(912);
        // validator: 0 + (2500-0)*912/1825 = 1249
        assert_eq!(ratios.validator_bps, 1249);
        // treasury: 4500 + (2500-4500)*912/1825 = 4500 - 999 = 3501
        assert_eq!(ratios.treasury_bps, 3501);
        // developer: same as treasury
        assert_eq!(ratios.developer_bps, 3501);
        // burn absorbs remainder: 10000 - 1249 - 3501 - 3501 = 1749
        assert_eq!(ratios.burn_bps, 1749);
        // Total must be 10_000
        assert_eq!(
            ratios.burn_bps + ratios.validator_bps + ratios.treasury_bps + ratios.developer_bps,
            10_000
        );
    }

    #[test]
    fn split_ratios_validate() {
        let valid = SplitRatios {
            burn_bps: 2500,
            validator_bps: 2500,
            treasury_bps: 2500,
            developer_bps: 2500,
        };
        assert!(valid.validate());

        let invalid = SplitRatios {
            burn_bps: 2500,
            validator_bps: 2500,
            treasury_bps: 2500,
            developer_bps: 3000,
        };
        assert!(!invalid.validate());
    }
}
