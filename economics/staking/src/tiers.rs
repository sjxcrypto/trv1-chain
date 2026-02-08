use serde::{Deserialize, Serialize};

/// Lock tier for staked tokens, determining rate percentage and vote weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LockTier {
    /// No lock period.
    NoLock,
    /// 30-day lock.
    ThirtyDay,
    /// 90-day lock.
    NinetyDay,
    /// 180-day lock.
    OneEightyDay,
    /// 360-day lock.
    ThreeSixtyDay,
    /// Delegator tier.
    Delegator,
    /// Permanent lock (cannot unstake).
    Permanent,
}

/// Number of epochs per year, assuming 1 epoch = 1 day.
pub const EPOCHS_PER_YEAR: u64 = 365;

/// Number of epochs in each lock duration.
pub const THIRTY_DAY_EPOCHS: u64 = 30;
pub const NINETY_DAY_EPOCHS: u64 = 90;
pub const ONE_EIGHTY_DAY_EPOCHS: u64 = 180;
pub const THREE_SIXTY_DAY_EPOCHS: u64 = 360;

impl LockTier {
    /// Percentage of the base validator rate earned by this tier.
    pub fn rate_pct(&self) -> u64 {
        match self {
            LockTier::NoLock => 5,
            LockTier::ThirtyDay => 10,
            LockTier::NinetyDay => 20,
            LockTier::OneEightyDay => 30,
            LockTier::ThreeSixtyDay => 50,
            LockTier::Delegator => 100,
            LockTier::Permanent => 120,
        }
    }

    /// Vote weight multiplier as fixed-point (x1000).
    /// e.g., 0.0x = 0, 0.1x = 100, 1.0x = 1000, 1.5x = 1500.
    pub fn vote_weight_bps(&self) -> u64 {
        match self {
            LockTier::NoLock => 0,
            LockTier::ThirtyDay => 100,
            LockTier::NinetyDay => 200,
            LockTier::OneEightyDay => 300,
            LockTier::ThreeSixtyDay => 500,
            LockTier::Delegator => 1000,
            LockTier::Permanent => 1500,
        }
    }

    /// Vote weight as f64.
    pub fn vote_weight(&self) -> f64 {
        self.vote_weight_bps() as f64 / 1000.0
    }

    /// Lock duration in epochs. Returns None for Permanent (never unlocks).
    pub fn lock_duration_epochs(&self) -> Option<u64> {
        match self {
            LockTier::NoLock => Some(0),
            LockTier::ThirtyDay => Some(THIRTY_DAY_EPOCHS),
            LockTier::NinetyDay => Some(NINETY_DAY_EPOCHS),
            LockTier::OneEightyDay => Some(ONE_EIGHTY_DAY_EPOCHS),
            LockTier::ThreeSixtyDay => Some(THREE_SIXTY_DAY_EPOCHS),
            LockTier::Delegator => Some(0),
            LockTier::Permanent => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_lock_tier() {
        let tier = LockTier::NoLock;
        assert_eq!(tier.rate_pct(), 5);
        assert_eq!(tier.vote_weight_bps(), 0);
        assert!((tier.vote_weight() - 0.0).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(0));
    }

    #[test]
    fn test_thirty_day_tier() {
        let tier = LockTier::ThirtyDay;
        assert_eq!(tier.rate_pct(), 10);
        assert_eq!(tier.vote_weight_bps(), 100);
        assert!((tier.vote_weight() - 0.1).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(30));
    }

    #[test]
    fn test_ninety_day_tier() {
        let tier = LockTier::NinetyDay;
        assert_eq!(tier.rate_pct(), 20);
        assert_eq!(tier.vote_weight_bps(), 200);
        assert!((tier.vote_weight() - 0.2).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(90));
    }

    #[test]
    fn test_one_eighty_day_tier() {
        let tier = LockTier::OneEightyDay;
        assert_eq!(tier.rate_pct(), 30);
        assert_eq!(tier.vote_weight_bps(), 300);
        assert!((tier.vote_weight() - 0.3).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(180));
    }

    #[test]
    fn test_three_sixty_day_tier() {
        let tier = LockTier::ThreeSixtyDay;
        assert_eq!(tier.rate_pct(), 50);
        assert_eq!(tier.vote_weight_bps(), 500);
        assert!((tier.vote_weight() - 0.5).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(360));
    }

    #[test]
    fn test_delegator_tier() {
        let tier = LockTier::Delegator;
        assert_eq!(tier.rate_pct(), 100);
        assert_eq!(tier.vote_weight_bps(), 1000);
        assert!((tier.vote_weight() - 1.0).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(0));
    }

    #[test]
    fn test_permanent_tier() {
        let tier = LockTier::Permanent;
        assert_eq!(tier.rate_pct(), 120);
        assert_eq!(tier.vote_weight_bps(), 1500);
        assert!((tier.vote_weight() - 1.5).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), None);
    }

    #[test]
    fn test_tiers_ordered_by_rate_pct() {
        let tiers = [
            LockTier::NoLock,
            LockTier::ThirtyDay,
            LockTier::NinetyDay,
            LockTier::OneEightyDay,
            LockTier::ThreeSixtyDay,
            LockTier::Delegator,
            LockTier::Permanent,
        ];
        for i in 1..tiers.len() {
            assert!(
                tiers[i].rate_pct() >= tiers[i - 1].rate_pct(),
                "rate_pct not non-decreasing at index {}",
                i
            );
            assert!(
                tiers[i].vote_weight_bps() >= tiers[i - 1].vote_weight_bps(),
                "vote_weight_bps not non-decreasing at index {}",
                i
            );
        }
    }
}
