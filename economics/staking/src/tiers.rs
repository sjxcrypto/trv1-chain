use serde::{Deserialize, Serialize};

/// Lock tier for staked tokens, determining multiplier, bonus APY, and vote weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LockTier {
    /// No lock period: 1.0x multiplier, 0% bonus, 1.0x vote weight.
    NoLock,
    /// 3-month lock: 1.2x multiplier, 1% bonus, 1.5x vote weight.
    ThreeMonth,
    /// 6-month lock: 1.5x multiplier, 2% bonus, 2.0x vote weight.
    SixMonth,
    /// 1-year lock: 2.0x multiplier, 3% bonus, 3.0x vote weight.
    OneYear,
    /// Permanent lock (cannot unstake): 3.0x multiplier, 5% bonus, 5.0x vote weight.
    Permanent,
}

/// Number of epochs per year, assuming 1 epoch = 1 day.
pub const EPOCHS_PER_YEAR: u64 = 365;

/// Number of epochs in each lock duration.
pub const THREE_MONTHS_EPOCHS: u64 = 90;
pub const SIX_MONTHS_EPOCHS: u64 = 180;
pub const ONE_YEAR_EPOCHS: u64 = 365;

impl LockTier {
    /// The reward multiplier as a fixed-point value (x1000 for precision).
    /// e.g., 1.0x = 1000, 1.2x = 1200, etc.
    pub fn multiplier_bps(&self) -> u64 {
        match self {
            LockTier::NoLock => 1000,
            LockTier::ThreeMonth => 1200,
            LockTier::SixMonth => 1500,
            LockTier::OneYear => 2000,
            LockTier::Permanent => 3000,
        }
    }

    /// The reward multiplier as f64.
    pub fn multiplier(&self) -> f64 {
        self.multiplier_bps() as f64 / 1000.0
    }

    /// Bonus APY in basis points (100 bps = 1%).
    pub fn bonus_apy_bps(&self) -> u64 {
        match self {
            LockTier::NoLock => 0,
            LockTier::ThreeMonth => 100,
            LockTier::SixMonth => 200,
            LockTier::OneYear => 300,
            LockTier::Permanent => 500,
        }
    }

    /// Bonus APY as f64 (e.g., 0.01 for 1%).
    pub fn bonus_apy(&self) -> f64 {
        self.bonus_apy_bps() as f64 / 10_000.0
    }

    /// Vote weight multiplier as fixed-point (x1000).
    /// e.g., 1.0x = 1000, 1.5x = 1500, etc.
    pub fn vote_weight_bps(&self) -> u64 {
        match self {
            LockTier::NoLock => 1000,
            LockTier::ThreeMonth => 1500,
            LockTier::SixMonth => 2000,
            LockTier::OneYear => 3000,
            LockTier::Permanent => 5000,
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
            LockTier::ThreeMonth => Some(THREE_MONTHS_EPOCHS),
            LockTier::SixMonth => Some(SIX_MONTHS_EPOCHS),
            LockTier::OneYear => Some(ONE_YEAR_EPOCHS),
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
        assert_eq!(tier.multiplier_bps(), 1000);
        assert!((tier.multiplier() - 1.0).abs() < f64::EPSILON);
        assert_eq!(tier.bonus_apy_bps(), 0);
        assert!((tier.bonus_apy() - 0.0).abs() < f64::EPSILON);
        assert_eq!(tier.vote_weight_bps(), 1000);
        assert!((tier.vote_weight() - 1.0).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(0));
    }

    #[test]
    fn test_three_month_tier() {
        let tier = LockTier::ThreeMonth;
        assert_eq!(tier.multiplier_bps(), 1200);
        assert!((tier.multiplier() - 1.2).abs() < f64::EPSILON);
        assert_eq!(tier.bonus_apy_bps(), 100);
        assert!((tier.bonus_apy() - 0.01).abs() < f64::EPSILON);
        assert_eq!(tier.vote_weight_bps(), 1500);
        assert!((tier.vote_weight() - 1.5).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(90));
    }

    #[test]
    fn test_six_month_tier() {
        let tier = LockTier::SixMonth;
        assert_eq!(tier.multiplier_bps(), 1500);
        assert!((tier.multiplier() - 1.5).abs() < f64::EPSILON);
        assert_eq!(tier.bonus_apy_bps(), 200);
        assert!((tier.bonus_apy() - 0.02).abs() < f64::EPSILON);
        assert_eq!(tier.vote_weight_bps(), 2000);
        assert!((tier.vote_weight() - 2.0).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(180));
    }

    #[test]
    fn test_one_year_tier() {
        let tier = LockTier::OneYear;
        assert_eq!(tier.multiplier_bps(), 2000);
        assert!((tier.multiplier() - 2.0).abs() < f64::EPSILON);
        assert_eq!(tier.bonus_apy_bps(), 300);
        assert!((tier.bonus_apy() - 0.03).abs() < f64::EPSILON);
        assert_eq!(tier.vote_weight_bps(), 3000);
        assert!((tier.vote_weight() - 3.0).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), Some(365));
    }

    #[test]
    fn test_permanent_tier() {
        let tier = LockTier::Permanent;
        assert_eq!(tier.multiplier_bps(), 3000);
        assert!((tier.multiplier() - 3.0).abs() < f64::EPSILON);
        assert_eq!(tier.bonus_apy_bps(), 500);
        assert!((tier.bonus_apy() - 0.05).abs() < f64::EPSILON);
        assert_eq!(tier.vote_weight_bps(), 5000);
        assert!((tier.vote_weight() - 5.0).abs() < f64::EPSILON);
        assert_eq!(tier.lock_duration_epochs(), None);
    }

    #[test]
    fn test_tiers_ordered_by_multiplier() {
        let tiers = [
            LockTier::NoLock,
            LockTier::ThreeMonth,
            LockTier::SixMonth,
            LockTier::OneYear,
            LockTier::Permanent,
        ];
        for i in 1..tiers.len() {
            assert!(tiers[i].multiplier_bps() > tiers[i - 1].multiplier_bps());
            assert!(tiers[i].vote_weight_bps() > tiers[i - 1].vote_weight_bps());
        }
    }
}
