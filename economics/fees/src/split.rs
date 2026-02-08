use crate::types::*;

/// Fee splitting logic: distributes fees to burn, validator, treasury, and developer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeeSplit {
    config: SplitConfig,
}

impl FeeSplit {
    pub fn new() -> Self {
        Self {
            config: SplitConfig::default(),
        }
    }

    pub fn with_config(config: SplitConfig) -> Result<Self, FeeError> {
        if !config.validate() {
            return Err(FeeError::InvalidSplitRatios(0));
        }
        Ok(Self { config })
    }

    /// Split a total fee into the 4-way distribution at a given epoch.
    /// The epoch determines the interpolated split ratios.
    pub fn split_fee(&self, total_fee: u64, epoch: u64) -> SplitResult {
        let ratios = self.config.split_at_epoch(epoch);
        let validator = (total_fee as u128 * ratios.validator_bps as u128 / 10_000) as u64;
        let treasury = (total_fee as u128 * ratios.treasury_bps as u128 / 10_000) as u64;
        let developer = (total_fee as u128 * ratios.developer_bps as u128 / 10_000) as u64;
        let burn = total_fee - validator - treasury - developer;

        SplitResult {
            burn,
            validator,
            treasury,
            developer,
        }
    }

    pub fn config(&self) -> &SplitConfig {
        &self.config
    }
}

impl Default for FeeSplit {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_at_launch() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(1_000_000, 0);
        assert_eq!(result.burn, 100_000);
        assert_eq!(result.validator, 0);
        assert_eq!(result.treasury, 450_000);
        assert_eq!(result.developer, 450_000);
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            1_000_000
        );
    }

    #[test]
    fn test_split_at_maturity() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(1_000_000, 1825);
        assert_eq!(result.burn, 250_000);
        assert_eq!(result.validator, 250_000);
        assert_eq!(result.treasury, 250_000);
        assert_eq!(result.developer, 250_000);
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            1_000_000
        );
    }

    #[test]
    fn test_split_at_midpoint() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(1_000_000, 912);
        // validator_bps = 0 + 2500*912/1825 = 1249 -> 1M * 1249/10000 = 124_900
        assert_eq!(result.validator, 124_900);
        // treasury_bps = 4500 - 2000*912/1825 = 3501 -> 1M * 3501/10000 = 350_100
        assert_eq!(result.treasury, 350_100);
        // developer_bps = same = 3501 -> 350_100
        assert_eq!(result.developer, 350_100);
        // burn = 1M - 124_900 - 350_100 - 350_100 = 174_900
        assert_eq!(result.burn, 174_900);
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            1_000_000
        );
    }

    #[test]
    fn test_split_zero_fee() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(0, 500);
        assert_eq!(result.burn, 0);
        assert_eq!(result.validator, 0);
        assert_eq!(result.treasury, 0);
        assert_eq!(result.developer, 0);
    }

    #[test]
    fn test_split_small_fee_at_launch() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(7, 0);
        // At epoch 0: burn=1000, validator=0, treasury=4500, developer=4500
        // validator = 7 * 0/10000 = 0
        // treasury = 7 * 4500/10000 = 3.15 -> 3
        // developer = 7 * 4500/10000 = 3.15 -> 3
        // burn = 7 - 0 - 3 - 3 = 1
        assert_eq!(result.validator, 0);
        assert_eq!(result.treasury, 3);
        assert_eq!(result.developer, 3);
        assert_eq!(result.burn, 1);
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            7
        );
    }

    #[test]
    fn test_split_conserves_total() {
        let splitter = FeeSplit::new();
        let amounts = [1, 3, 7, 13, 99, 101, 999_999, 1_000_001, u64::MAX / 2];
        let epochs = [0, 500, 912, 1825, 5000];
        for &amount in &amounts {
            for &epoch in &epochs {
                let result = splitter.split_fee(amount, epoch);
                assert_eq!(
                    result.burn + result.validator + result.treasury + result.developer,
                    amount,
                    "Total not conserved for amount {} at epoch {}",
                    amount,
                    epoch
                );
            }
        }
    }

    #[test]
    fn test_split_with_custom_config() {
        let config = SplitConfig {
            launch: SplitRatios {
                burn_bps: 5000,
                validator_bps: 2500,
                treasury_bps: 1500,
                developer_bps: 1000,
            },
            maturity: SplitRatios {
                burn_bps: 2500,
                validator_bps: 2500,
                treasury_bps: 2500,
                developer_bps: 2500,
            },
            transition_epochs: 1000,
        };
        let splitter = FeeSplit::with_config(config).unwrap();
        let result = splitter.split_fee(1_000_000, 0);
        assert_eq!(result.burn, 500_000);
        assert_eq!(result.validator, 250_000);
        assert_eq!(result.treasury, 150_000);
        assert_eq!(result.developer, 100_000);
    }

    #[test]
    fn test_invalid_config() {
        let config = SplitConfig {
            launch: SplitRatios {
                burn_bps: 5000,
                validator_bps: 2500,
                treasury_bps: 1500,
                developer_bps: 2000, // Sum = 11000
            },
            maturity: SplitRatios {
                burn_bps: 2500,
                validator_bps: 2500,
                treasury_bps: 2500,
                developer_bps: 2500,
            },
            transition_epochs: 1000,
        };
        let result = FeeSplit::with_config(config);
        assert!(matches!(result, Err(FeeError::InvalidSplitRatios(0))));
    }

    #[test]
    fn test_all_to_burn_at_custom() {
        let config = SplitConfig {
            launch: SplitRatios {
                burn_bps: 10_000,
                validator_bps: 0,
                treasury_bps: 0,
                developer_bps: 0,
            },
            maturity: SplitRatios {
                burn_bps: 10_000,
                validator_bps: 0,
                treasury_bps: 0,
                developer_bps: 0,
            },
            transition_epochs: 100,
        };
        let splitter = FeeSplit::with_config(config).unwrap();
        let result = splitter.split_fee(1_000_000, 50);
        assert_eq!(result.burn, 1_000_000);
        assert_eq!(result.validator, 0);
        assert_eq!(result.treasury, 0);
        assert_eq!(result.developer, 0);
    }

    #[test]
    fn test_large_fee_no_overflow() {
        let splitter = FeeSplit::new();
        for epoch in [0, 912, 1825, 5000] {
            let result = splitter.split_fee(u64::MAX, epoch);
            assert_eq!(
                result.burn + result.validator + result.treasury + result.developer,
                u64::MAX,
                "Total not conserved for u64::MAX at epoch {}",
                epoch
            );
        }
    }
}
