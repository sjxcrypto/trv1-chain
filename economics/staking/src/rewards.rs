use crate::tiers::{LockTier, EPOCHS_PER_YEAR};

/// Base staking APY: 5% (500 basis points).
pub const BASE_APY_BPS: u64 = 500;

/// Calculate the reward for a staked amount over a number of elapsed epochs.
///
/// Formula: reward = amount * (base_rate + tier_bonus) * (epochs_elapsed / epochs_per_year)
///
/// All arithmetic uses integer math with basis-point precision to avoid
/// floating-point rounding issues in consensus-critical code.
pub fn calculate_reward(amount: u64, tier: &LockTier, epochs_elapsed: u64) -> u64 {
    if amount == 0 || epochs_elapsed == 0 {
        return 0;
    }

    let total_apy_bps = BASE_APY_BPS + tier.bonus_apy_bps();

    // reward = amount * total_apy_bps * epochs_elapsed / (10_000 * EPOCHS_PER_YEAR)
    // We use u128 to avoid overflow for large stakes.
    let numerator = (amount as u128)
        .checked_mul(total_apy_bps as u128)
        .unwrap()
        .checked_mul(epochs_elapsed as u128)
        .unwrap();
    let denominator = 10_000u128 * EPOCHS_PER_YEAR as u128;

    (numerator / denominator) as u64
}

/// Calculate reward for a single epoch.
pub fn calculate_epoch_reward(amount: u64, tier: &LockTier) -> u64 {
    calculate_reward(amount, tier, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_amount() {
        assert_eq!(calculate_reward(0, &LockTier::NoLock, 365), 0);
    }

    #[test]
    fn test_zero_epochs() {
        assert_eq!(calculate_reward(1_000_000, &LockTier::NoLock, 0), 0);
    }

    #[test]
    fn test_base_apy_full_year_no_lock() {
        // 1,000,000 tokens * 5% over 365 epochs = 50,000
        let reward = calculate_reward(1_000_000, &LockTier::NoLock, 365);
        assert_eq!(reward, 50_000);
    }

    #[test]
    fn test_three_month_tier_full_year() {
        // 1,000,000 * (5% + 1%) = 60,000 over a full year
        let reward = calculate_reward(1_000_000, &LockTier::ThreeMonth, 365);
        assert_eq!(reward, 60_000);
    }

    #[test]
    fn test_six_month_tier_full_year() {
        // 1,000,000 * (5% + 2%) = 70,000 over a full year
        let reward = calculate_reward(1_000_000, &LockTier::SixMonth, 365);
        assert_eq!(reward, 70_000);
    }

    #[test]
    fn test_one_year_tier_full_year() {
        // 1,000,000 * (5% + 3%) = 80,000 over a full year
        let reward = calculate_reward(1_000_000, &LockTier::OneYear, 365);
        assert_eq!(reward, 80_000);
    }

    #[test]
    fn test_permanent_tier_full_year() {
        // 1,000,000 * (5% + 5%) = 100,000 over a full year
        let reward = calculate_reward(1_000_000, &LockTier::Permanent, 365);
        assert_eq!(reward, 100_000);
    }

    #[test]
    fn test_single_epoch_reward() {
        // 1,000,000 * 5% / 365 = 136 (integer division)
        let reward = calculate_epoch_reward(1_000_000, &LockTier::NoLock);
        assert_eq!(reward, 136);
    }

    #[test]
    fn test_half_year() {
        // 1,000,000 * 5% * (182/365) = 24,931 (integer division)
        let reward = calculate_reward(1_000_000, &LockTier::NoLock, 182);
        // 1_000_000 * 500 * 182 / (10_000 * 365) = 91_000_000 / 3_650_000 = 24_931
        assert_eq!(reward, 24_931);
    }

    #[test]
    fn test_large_stake() {
        // 1 billion tokens, permanent tier, full year
        // 1_000_000_000 * 10% = 100_000_000
        let reward = calculate_reward(1_000_000_000, &LockTier::Permanent, 365);
        assert_eq!(reward, 100_000_000);
    }

    #[test]
    fn test_very_small_stake() {
        // 100 tokens, no lock, 1 epoch => 100 * 500 * 1 / (10000 * 365) = 0 (rounds down)
        let reward = calculate_reward(100, &LockTier::NoLock, 1);
        assert_eq!(reward, 0);
    }

    #[test]
    fn test_reward_increases_with_tier() {
        let amount = 10_000_000;
        let epochs = 365;
        let r0 = calculate_reward(amount, &LockTier::NoLock, epochs);
        let r1 = calculate_reward(amount, &LockTier::ThreeMonth, epochs);
        let r2 = calculate_reward(amount, &LockTier::SixMonth, epochs);
        let r3 = calculate_reward(amount, &LockTier::OneYear, epochs);
        let r4 = calculate_reward(amount, &LockTier::Permanent, epochs);
        assert!(r0 < r1);
        assert!(r1 < r2);
        assert!(r2 < r3);
        assert!(r3 < r4);
    }
}
