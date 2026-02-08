use crate::types::*;

/// EIP-1559 dynamic fee market implementation.
///
/// The base fee adjusts up/down by up to 12.5% per block depending
/// on whether actual gas usage exceeds the target.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeeMarket {
    config: FeeConfig,
    base_fee: u64,
}

impl FeeMarket {
    /// Create a new fee market with the given config and initial base fee.
    pub fn new(config: FeeConfig, initial_base_fee: u64) -> Result<Self, FeeError> {
        if config.elasticity_multiplier == 0 {
            return Err(FeeError::ZeroElasticity);
        }
        if !config.split.validate() {
            let sum = config.split.burn_bps
                + config.split.validator_bps
                + config.split.treasury_bps
                + config.split.developer_bps;
            return Err(FeeError::InvalidSplitRatios(sum));
        }
        let base_fee = initial_base_fee.max(config.base_fee_floor);
        Ok(Self { config, base_fee })
    }

    /// Update the base fee after a block is produced.
    ///
    /// - If `gas_used > gas_target`: increase base fee proportionally (up to 12.5%).
    /// - If `gas_used < gas_target`: decrease base fee proportionally (up to 12.5%).
    /// - The base fee never drops below `base_fee_floor`.
    pub fn update_base_fee(&mut self, gas_used: u64) {
        let gas_target = self.config.target_gas_per_block;
        let elasticity = self.config.elasticity_multiplier;

        if gas_used == gas_target {
            // No adjustment needed.
            return;
        }

        if gas_used > gas_target {
            // Fee goes up: base_fee += base_fee * (gas_used - gas_target) / (gas_target * elasticity)
            let gas_delta = gas_used - gas_target;
            let fee_delta = (self.base_fee as u128 * gas_delta as u128)
                / (gas_target as u128 * elasticity as u128);
            // Ensure at least 1 unit increase if there is any excess.
            let fee_delta = fee_delta.max(1) as u64;
            self.base_fee = self.base_fee.saturating_add(fee_delta);
        } else {
            // Fee goes down: base_fee -= base_fee * (gas_target - gas_used) / (gas_target * elasticity)
            let gas_delta = gas_target - gas_used;
            let fee_delta = (self.base_fee as u128 * gas_delta as u128)
                / (gas_target as u128 * elasticity as u128);
            let fee_delta = fee_delta as u64;
            self.base_fee = self.base_fee.saturating_sub(fee_delta);
        }

        // Enforce floor.
        if self.base_fee < self.config.base_fee_floor {
            self.base_fee = self.config.base_fee_floor;
        }
    }

    /// Calculate the fee for a transaction.
    pub fn calculate_fee(&self, gas_units: u64, priority_fee_per_gas: u64) -> TransactionFee {
        let base_fee = self.base_fee.saturating_mul(gas_units);
        let priority_fee = priority_fee_per_gas.saturating_mul(gas_units);
        TransactionFee {
            base_fee,
            priority_fee,
            total: base_fee.saturating_add(priority_fee),
        }
    }

    /// Get the current base fee per gas unit.
    pub fn current_base_fee(&self) -> u64 {
        self.base_fee
    }

    /// Get the fee config.
    pub fn config(&self) -> &FeeConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_market() -> FeeMarket {
        FeeMarket::new(FeeConfig::default(), 100).unwrap()
    }

    #[test]
    fn test_initial_base_fee() {
        let market = default_market();
        assert_eq!(market.current_base_fee(), 100);
    }

    #[test]
    fn test_base_fee_floor_enforcement() {
        let config = FeeConfig {
            base_fee_floor: 50,
            ..FeeConfig::default()
        };
        // Initial base fee below floor gets clamped.
        let market = FeeMarket::new(config, 10).unwrap();
        assert_eq!(market.current_base_fee(), 50);
    }

    #[test]
    fn test_base_fee_no_change_at_target() {
        let mut market = default_market();
        let target = market.config.target_gas_per_block;
        market.update_base_fee(target);
        assert_eq!(market.current_base_fee(), 100);
    }

    #[test]
    fn test_base_fee_increases_above_target() {
        let mut market = default_market();
        let target = market.config.target_gas_per_block;
        // Use double the target gas (max).
        market.update_base_fee(target * 2);
        // Delta = 100 * (target) / (target * 8) = 100/8 = 12 (12.5%)
        assert_eq!(market.current_base_fee(), 112);
    }

    #[test]
    fn test_base_fee_decreases_below_target() {
        let mut market = default_market();
        // Use zero gas.
        market.update_base_fee(0);
        // Delta = 100 * target / (target * 8) = 100/8 = 12 (12.5%)
        assert_eq!(market.current_base_fee(), 88);
    }

    #[test]
    fn test_base_fee_small_increase() {
        let mut market = default_market();
        let target = market.config.target_gas_per_block;
        // Use slightly above target.
        market.update_base_fee(target + target / 10); // +10%
        // Delta = 100 * (target/10) / (target * 8) = 100/80 = 1
        assert_eq!(market.current_base_fee(), 101);
    }

    #[test]
    fn test_base_fee_does_not_go_below_floor() {
        let config = FeeConfig {
            base_fee_floor: 50,
            ..FeeConfig::default()
        };
        let mut market = FeeMarket::new(config, 50).unwrap();
        // Large decrease.
        market.update_base_fee(0);
        // Would be 50 - 6 = 44, but floor is 50.
        assert_eq!(market.current_base_fee(), 50);
    }

    #[test]
    fn test_calculate_fee() {
        let market = default_market();
        let fee = market.calculate_fee(100_000, 5);
        // base_fee = 100 * 100_000 = 10_000_000
        // priority_fee = 5 * 100_000 = 500_000
        // total = 10_500_000
        assert_eq!(
            fee,
            TransactionFee {
                base_fee: 10_000_000,
                priority_fee: 500_000,
                total: 10_500_000,
            }
        );
    }

    #[test]
    fn test_fee_calculation_zero_priority() {
        let market = default_market();
        let fee = market.calculate_fee(50_000, 0);
        assert_eq!(fee.base_fee, 5_000_000);
        assert_eq!(fee.priority_fee, 0);
        assert_eq!(fee.total, 5_000_000);
    }

    #[test]
    fn test_invalid_split_ratios() {
        let config = FeeConfig {
            split: SplitConfig {
                burn_bps: 5000,
                validator_bps: 3000,
                treasury_bps: 2000,
                developer_bps: 2000, // Sum = 12000
            },
            ..FeeConfig::default()
        };
        let result = FeeMarket::new(config, 100);
        assert!(matches!(result, Err(FeeError::InvalidSplitRatios(12000))));
    }

    #[test]
    fn test_zero_elasticity() {
        let config = FeeConfig {
            elasticity_multiplier: 0,
            ..FeeConfig::default()
        };
        let result = FeeMarket::new(config, 100);
        assert!(matches!(result, Err(FeeError::ZeroElasticity)));
    }

    #[test]
    fn test_successive_increases() {
        let mut market = default_market();
        let target = market.config.target_gas_per_block;
        // Simulate high utilization over several blocks.
        for _ in 0..10 {
            market.update_base_fee(target * 2);
        }
        // Base fee should have compounded upward.
        assert!(market.current_base_fee() > 200);
    }

    #[test]
    fn test_successive_decreases_hit_floor() {
        let config = FeeConfig {
            base_fee_floor: 10,
            ..FeeConfig::default()
        };
        let mut market = FeeMarket::new(config, 100).unwrap();
        // Empty blocks.
        for _ in 0..100 {
            market.update_base_fee(0);
        }
        assert_eq!(market.current_base_fee(), 10);
    }
}
