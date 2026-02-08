use crate::types::*;

/// Fee splitting logic: distributes fees to burn, validator, treasury, and developer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeeSplit {
    config: SplitConfig,
}

impl FeeSplit {
    /// Create a new FeeSplit with the default 40/30/20/10 ratio.
    pub fn new() -> Self {
        Self {
            config: SplitConfig::default(),
        }
    }

    /// Create a FeeSplit with custom ratios.
    /// Returns an error if ratios don't sum to 10,000 bps.
    pub fn with_config(config: SplitConfig) -> Result<Self, FeeError> {
        if !config.validate() {
            let sum = config.burn_bps + config.validator_bps + config.treasury_bps + config.developer_bps;
            return Err(FeeError::InvalidSplitRatios(sum));
        }
        Ok(Self { config })
    }

    /// Split a total fee into the 4-way distribution.
    ///
    /// Any remainder from integer rounding goes to the burn bucket
    /// to ensure the total always matches.
    pub fn split_fee(&self, total_fee: u64) -> SplitResult {
        let validator = (total_fee as u128 * self.config.validator_bps as u128 / 10_000) as u64;
        let treasury = (total_fee as u128 * self.config.treasury_bps as u128 / 10_000) as u64;
        let developer = (total_fee as u128 * self.config.developer_bps as u128 / 10_000) as u64;
        // Burn gets the remainder to ensure exact total.
        let burn = total_fee - validator - treasury - developer;

        SplitResult {
            burn,
            validator,
            treasury,
            developer,
        }
    }

    /// Get the current split configuration.
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
    fn test_default_split() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(1_000_000);

        assert_eq!(result.burn, 400_000);
        assert_eq!(result.validator, 300_000);
        assert_eq!(result.treasury, 200_000);
        assert_eq!(result.developer, 100_000);

        // Verify total conservation.
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            1_000_000
        );
    }

    #[test]
    fn test_split_zero_fee() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(0);
        assert_eq!(result.burn, 0);
        assert_eq!(result.validator, 0);
        assert_eq!(result.treasury, 0);
        assert_eq!(result.developer, 0);
    }

    #[test]
    fn test_split_small_fee() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(7);
        // 7 * 30% = 2.1 -> 2, 7 * 20% = 1.4 -> 1, 7 * 10% = 0.7 -> 0
        // burn = 7 - 2 - 1 - 0 = 4
        assert_eq!(result.validator, 2);
        assert_eq!(result.treasury, 1);
        assert_eq!(result.developer, 0);
        assert_eq!(result.burn, 4);
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            7
        );
    }

    #[test]
    fn test_split_with_custom_config() {
        let config = SplitConfig {
            burn_bps: 5000,   // 50%
            validator_bps: 2500, // 25%
            treasury_bps: 1500, // 15%
            developer_bps: 1000, // 10%
        };
        let splitter = FeeSplit::with_config(config).unwrap();
        let result = splitter.split_fee(1_000_000);

        assert_eq!(result.validator, 250_000);
        assert_eq!(result.treasury, 150_000);
        assert_eq!(result.developer, 100_000);
        assert_eq!(result.burn, 500_000);
    }

    #[test]
    fn test_invalid_config() {
        let config = SplitConfig {
            burn_bps: 5000,
            validator_bps: 2500,
            treasury_bps: 1500,
            developer_bps: 2000, // Sum = 11000
        };
        let result = FeeSplit::with_config(config);
        assert!(matches!(result, Err(FeeError::InvalidSplitRatios(11000))));
    }

    #[test]
    fn test_split_conserves_total() {
        let splitter = FeeSplit::new();
        // Test with various amounts that might cause rounding issues.
        for amount in [1, 3, 7, 13, 99, 101, 999_999, 1_000_001, u64::MAX / 2] {
            let result = splitter.split_fee(amount);
            assert_eq!(
                result.burn + result.validator + result.treasury + result.developer,
                amount,
                "Total not conserved for amount {}",
                amount
            );
        }
    }

    #[test]
    fn test_all_to_burn() {
        let config = SplitConfig {
            burn_bps: 10_000,
            validator_bps: 0,
            treasury_bps: 0,
            developer_bps: 0,
        };
        let splitter = FeeSplit::with_config(config).unwrap();
        let result = splitter.split_fee(1_000_000);
        assert_eq!(result.burn, 1_000_000);
        assert_eq!(result.validator, 0);
        assert_eq!(result.treasury, 0);
        assert_eq!(result.developer, 0);
    }

    #[test]
    fn test_large_fee_no_overflow() {
        let splitter = FeeSplit::new();
        let result = splitter.split_fee(u64::MAX);
        assert_eq!(
            result.burn + result.validator + result.treasury + result.developer,
            u64::MAX
        );
    }
}
