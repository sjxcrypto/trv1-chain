use crate::tiers::{LockTier, EPOCHS_PER_YEAR};

/// Base staking APY: 5% (500 basis points).
pub const BASE_APY_BPS: u64 = 500;

/// Calculate the reward for a staked amount over a number of elapsed epochs.
///
/// Formula: reward = amount * BASE_APY_BPS * tier.rate_pct() * epochs / (100 * 10_000 * 365)
///
/// All arithmetic uses integer math with basis-point precision to avoid
/// floating-point rounding issues in consensus-critical code.
pub fn calculate_reward(amount: u64, tier: &LockTier, epochs_elapsed: u64) -> u64 {
    if amount == 0 || epochs_elapsed == 0 {
        return 0;
    }

    let numerator = (amount as u128)
        .checked_mul(BASE_APY_BPS as u128)
        .unwrap()
        .checked_mul(tier.rate_pct() as u128)
        .unwrap()
        .checked_mul(epochs_elapsed as u128)
        .unwrap();
    let denominator = 100u128 * 10_000u128 * EPOCHS_PER_YEAR as u128;

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
    fn test_no_lock_full_year() {
        // 1_000_000 * 500 * 5 * 365 / (100 * 10_000 * 365) = 2,500
        let reward = calculate_reward(1_000_000, &LockTier::NoLock, 365);
        assert_eq!(reward, 2_500);
    }

    #[test]
    fn test_thirty_day_tier_full_year() {
        // 1_000_000 * 500 * 10 * 365 / (100 * 10_000 * 365) = 5,000
        let reward = calculate_reward(1_000_000, &LockTier::ThirtyDay, 365);
        assert_eq!(reward, 5_000);
    }

    #[test]
    fn test_ninety_day_tier_full_year() {
        // 1_000_000 * 500 * 20 * 365 / (100 * 10_000 * 365) = 10,000
        let reward = calculate_reward(1_000_000, &LockTier::NinetyDay, 365);
        assert_eq!(reward, 10_000);
    }

    #[test]
    fn test_one_eighty_day_tier_full_year() {
        // 1_000_000 * 500 * 30 * 365 / (100 * 10_000 * 365) = 15,000
        let reward = calculate_reward(1_000_000, &LockTier::OneEightyDay, 365);
        assert_eq!(reward, 15_000);
    }

    #[test]
    fn test_three_sixty_day_tier_full_year() {
        // 1_000_000 * 500 * 50 * 365 / (100 * 10_000 * 365) = 25,000
        let reward = calculate_reward(1_000_000, &LockTier::ThreeSixtyDay, 365);
        assert_eq!(reward, 25_000);
    }

    #[test]
    fn test_delegator_tier_full_year() {
        // 1_000_000 * 500 * 100 * 365 / (100 * 10_000 * 365) = 50,000
        let reward = calculate_reward(1_000_000, &LockTier::Delegator, 365);
        assert_eq!(reward, 50_000);
    }

    #[test]
    fn test_permanent_tier_full_year() {
        // 1_000_000 * 500 * 120 * 365 / (100 * 10_000 * 365) = 60,000
        let reward = calculate_reward(1_000_000, &LockTier::Permanent, 365);
        assert_eq!(reward, 60_000);
    }

    #[test]
    fn test_single_epoch_reward() {
        // 1_000_000 * 500 * 5 * 1 / (100 * 10_000 * 365) = 6
        let reward = calculate_epoch_reward(1_000_000, &LockTier::NoLock);
        assert_eq!(reward, 6);
    }

    #[test]
    fn test_half_year() {
        // 1_000_000 * 500 * 5 * 182 / (100 * 10_000 * 365) = 455_000_000_000 / 365_000_000 = 1246
        let reward = calculate_reward(1_000_000, &LockTier::NoLock, 182);
        assert_eq!(reward, 1_246);
    }

    #[test]
    fn test_large_stake() {
        // 1_000_000_000 * 500 * 120 * 365 / (100 * 10_000 * 365) = 60_000_000
        let reward = calculate_reward(1_000_000_000, &LockTier::Permanent, 365);
        assert_eq!(reward, 60_000_000);
    }

    #[test]
    fn test_very_small_stake() {
        // 100 * 500 * 5 * 1 / (365_000_000) = 0
        let reward = calculate_reward(100, &LockTier::NoLock, 1);
        assert_eq!(reward, 0);
    }

    #[test]
    fn test_reward_increases_with_tier() {
        let amount = 10_000_000;
        let epochs = 365;
        let r0 = calculate_reward(amount, &LockTier::NoLock, epochs);
        let r1 = calculate_reward(amount, &LockTier::ThirtyDay, epochs);
        let r2 = calculate_reward(amount, &LockTier::NinetyDay, epochs);
        let r3 = calculate_reward(amount, &LockTier::OneEightyDay, epochs);
        let r4 = calculate_reward(amount, &LockTier::ThreeSixtyDay, epochs);
        let r5 = calculate_reward(amount, &LockTier::Delegator, epochs);
        let r6 = calculate_reward(amount, &LockTier::Permanent, epochs);
        assert!(r0 < r1);
        assert!(r1 < r2);
        assert!(r2 < r3);
        assert!(r3 < r4);
        assert!(r4 < r5);
        assert!(r5 < r6);
    }
}
