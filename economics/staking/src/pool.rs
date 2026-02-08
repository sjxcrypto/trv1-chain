use crate::rewards::calculate_epoch_reward;
use crate::tiers::LockTier;
use crate::types::*;

/// Check if a stake entry is unlocked at the given epoch.
fn is_unlocked(unlock_epoch: Option<u64>, current_epoch: u64) -> bool {
    match unlock_epoch {
        None => false, // Permanent lock
        Some(unlock) => current_epoch >= unlock,
    }
}

/// The main staking pool that manages all stakes and delegations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct StakingPool {
    state: StakingState,
}

impl StakingPool {
    /// Create a new empty staking pool.
    pub fn new() -> Self {
        Self {
            state: StakingState::default(),
        }
    }

    /// Advance the pool to a new epoch.
    pub fn set_epoch(&mut self, epoch: u64) {
        self.state.current_epoch = epoch;
    }

    /// Get the current epoch.
    pub fn current_epoch(&self) -> u64 {
        self.state.current_epoch
    }

    /// Stake tokens with a given lock tier.
    pub fn stake(
        &mut self,
        staker: PublicKey,
        amount: u64,
        tier: LockTier,
    ) -> Result<(), StakingError> {
        if amount == 0 {
            return Err(StakingError::ZeroAmount);
        }

        let unlock_epoch = tier
            .lock_duration_epochs()
            .map(|d| self.state.current_epoch + d);

        let entry = StakeEntry {
            staker,
            amount,
            lock_tier: tier,
            start_epoch: self.state.current_epoch,
            unlock_epoch,
        };

        self.state
            .entries
            .entry(staker)
            .or_default()
            .push(entry);

        self.state.total_staked = self
            .state
            .total_staked
            .checked_add(amount)
            .ok_or(StakingError::Overflow)?;

        Ok(())
    }

    /// Unstake tokens. Removes from the earliest unlockable entries first.
    pub fn unstake(&mut self, staker: PublicKey, mut amount: u64) -> Result<(), StakingError> {
        if amount == 0 {
            return Err(StakingError::ZeroAmount);
        }

        let current_epoch = self.state.current_epoch;
        let entries = self
            .state
            .entries
            .get_mut(&staker)
            .ok_or(StakingError::NoStakeFound)?;

        // Check that enough unlocked stake is available.
        let unlocked_total: u64 = entries
            .iter()
            .filter(|e| is_unlocked(e.unlock_epoch, current_epoch))
            .map(|e| e.amount)
            .sum();

        if unlocked_total < amount {
            return Err(StakingError::InsufficientBalance {
                have: unlocked_total,
                need: amount,
            });
        }

        // Remove from unlocked entries (FIFO).
        let mut removed_total = 0u64;
        let mut i = 0;
        while amount > 0 && i < entries.len() {
            if is_unlocked(entries[i].unlock_epoch, current_epoch) {
                if entries[i].amount <= amount {
                    amount -= entries[i].amount;
                    removed_total += entries[i].amount;
                    entries.remove(i);
                } else {
                    entries[i].amount -= amount;
                    removed_total += amount;
                    amount = 0;
                }
            } else {
                i += 1;
            }
        }
        self.state.total_staked -= removed_total;

        // Clean up empty vec.
        if entries.is_empty() {
            self.state.entries.remove(&staker);
        }

        Ok(())
    }

    /// Delegate tokens to a validator.
    pub fn delegate(
        &mut self,
        delegator: PublicKey,
        validator: PublicKey,
        amount: u64,
        tier: LockTier,
    ) -> Result<(), StakingError> {
        if amount == 0 {
            return Err(StakingError::ZeroAmount);
        }

        let unlock_epoch = tier
            .lock_duration_epochs()
            .map(|d| self.state.current_epoch + d);

        let entry = DelegationEntry {
            delegator,
            validator,
            amount,
            lock_tier: tier,
            start_epoch: self.state.current_epoch,
            unlock_epoch,
        };

        let key = (delegator, validator);
        self.state
            .delegations
            .entry(key)
            .or_default()
            .push(entry);

        self.state.total_staked = self
            .state
            .total_staked
            .checked_add(amount)
            .ok_or(StakingError::Overflow)?;

        Ok(())
    }

    /// Undelegate tokens from a validator.
    pub fn undelegate(
        &mut self,
        delegator: PublicKey,
        validator: PublicKey,
        mut amount: u64,
    ) -> Result<(), StakingError> {
        if amount == 0 {
            return Err(StakingError::ZeroAmount);
        }

        let current_epoch = self.state.current_epoch;
        let key = (delegator, validator);
        let entries = self
            .state
            .delegations
            .get_mut(&key)
            .ok_or(StakingError::NoDelegationFound)?;

        let unlocked_total: u64 = entries
            .iter()
            .filter(|e| is_unlocked(e.unlock_epoch, current_epoch))
            .map(|e| e.amount)
            .sum();

        if unlocked_total < amount {
            return Err(StakingError::InsufficientBalance {
                have: unlocked_total,
                need: amount,
            });
        }

        let mut removed_total = 0u64;
        let mut i = 0;
        while amount > 0 && i < entries.len() {
            if is_unlocked(entries[i].unlock_epoch, current_epoch) {
                if entries[i].amount <= amount {
                    amount -= entries[i].amount;
                    removed_total += entries[i].amount;
                    entries.remove(i);
                } else {
                    entries[i].amount -= amount;
                    removed_total += amount;
                    amount = 0;
                }
            } else {
                i += 1;
            }
        }
        self.state.total_staked -= removed_total;

        if entries.is_empty() {
            self.state.delegations.remove(&key);
        }

        Ok(())
    }

    /// Distribute rewards for one epoch to all stakers and delegators.
    /// Returns a list of (pubkey, reward_amount) pairs.
    pub fn distribute_epoch_rewards(&self) -> Vec<(PublicKey, u64)> {
        let mut rewards = Vec::new();

        // Rewards for direct stakers.
        for (staker, entries) in &self.state.entries {
            let total_reward: u64 = entries
                .iter()
                .map(|e| calculate_epoch_reward(e.amount, &e.lock_tier))
                .sum();
            if total_reward > 0 {
                rewards.push((*staker, total_reward));
            }
        }

        // Rewards for delegators (delegators earn rewards, not validators).
        for ((delegator, _validator), entries) in &self.state.delegations {
            let total_reward: u64 = entries
                .iter()
                .map(|e| calculate_epoch_reward(e.amount, &e.lock_tier))
                .sum();
            if total_reward > 0 {
                rewards.push((*delegator, total_reward));
            }
        }

        rewards
    }

    /// Get the total voting power for a pubkey, considering both direct stakes
    /// and delegations TO this pubkey as a validator.
    pub fn get_voting_power(&self, pubkey: &PublicKey) -> u64 {
        let mut power: u64 = 0;

        // Direct stake voting power.
        if let Some(entries) = self.state.entries.get(pubkey) {
            for entry in entries {
                // amount * vote_weight_bps / 1000
                let entry_power = (entry.amount as u128)
                    * (entry.lock_tier.vote_weight_bps() as u128)
                    / 1000;
                power = power.saturating_add(entry_power as u64);
            }
        }

        // Delegated voting power (delegations where this pubkey is the validator).
        for ((_, validator), entries) in &self.state.delegations {
            if validator == pubkey {
                for entry in entries {
                    let entry_power = (entry.amount as u128)
                        * (entry.lock_tier.vote_weight_bps() as u128)
                        / 1000;
                    power = power.saturating_add(entry_power as u64);
                }
            }
        }

        power
    }

    /// Get the total amount staked across the entire pool.
    pub fn total_staked(&self) -> u64 {
        self.state.total_staked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    #[test]
    fn test_stake_and_total() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::NoLock).unwrap();
        assert_eq!(pool.total_staked(), 1_000_000);

        pool.stake(key(2), 2_000_000, LockTier::ThreeMonth).unwrap();
        assert_eq!(pool.total_staked(), 3_000_000);
    }

    #[test]
    fn test_stake_zero_amount() {
        let mut pool = StakingPool::new();
        let result = pool.stake(key(1), 0, LockTier::NoLock);
        assert_eq!(result, Err(StakingError::ZeroAmount));
    }

    #[test]
    fn test_unstake_no_lock() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::NoLock).unwrap();

        // NoLock has 0 duration, so epoch 0 >= epoch 0, immediately unlockable.
        pool.unstake(key(1), 500_000).unwrap();
        assert_eq!(pool.total_staked(), 500_000);

        pool.unstake(key(1), 500_000).unwrap();
        assert_eq!(pool.total_staked(), 0);
    }

    #[test]
    fn test_unstake_locked_fails() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::ThreeMonth).unwrap();

        // At epoch 0, the unlock epoch is 90. Cannot unstake yet.
        let result = pool.unstake(key(1), 500_000);
        assert!(matches!(result, Err(StakingError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_unstake_after_lock_expires() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::ThreeMonth).unwrap();

        // Advance to epoch 90 (lock expires).
        pool.set_epoch(90);
        pool.unstake(key(1), 1_000_000).unwrap();
        assert_eq!(pool.total_staked(), 0);
    }

    #[test]
    fn test_permanent_lock_cannot_unstake() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::Permanent).unwrap();

        pool.set_epoch(10_000);
        let result = pool.unstake(key(1), 500_000);
        assert!(matches!(result, Err(StakingError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_unstake_no_stake_found() {
        let mut pool = StakingPool::new();
        let result = pool.unstake(key(99), 100);
        assert_eq!(result, Err(StakingError::NoStakeFound));
    }

    #[test]
    fn test_delegate_and_undelegate() {
        let mut pool = StakingPool::new();
        pool.delegate(key(1), key(2), 500_000, LockTier::NoLock).unwrap();
        assert_eq!(pool.total_staked(), 500_000);

        pool.undelegate(key(1), key(2), 200_000).unwrap();
        assert_eq!(pool.total_staked(), 300_000);
    }

    #[test]
    fn test_delegate_locked_undelegate_fails() {
        let mut pool = StakingPool::new();
        pool.delegate(key(1), key(2), 500_000, LockTier::SixMonth).unwrap();

        let result = pool.undelegate(key(1), key(2), 100_000);
        assert!(matches!(result, Err(StakingError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_delegate_undelegate_after_lock() {
        let mut pool = StakingPool::new();
        pool.delegate(key(1), key(2), 500_000, LockTier::SixMonth).unwrap();

        pool.set_epoch(180);
        pool.undelegate(key(1), key(2), 500_000).unwrap();
        assert_eq!(pool.total_staked(), 0);
    }

    #[test]
    fn test_undelegate_not_found() {
        let mut pool = StakingPool::new();
        let result = pool.undelegate(key(1), key(2), 100);
        assert_eq!(result, Err(StakingError::NoDelegationFound));
    }

    #[test]
    fn test_epoch_rewards_distribution() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::NoLock).unwrap();
        pool.stake(key(2), 1_000_000, LockTier::Permanent).unwrap();

        let rewards = pool.distribute_epoch_rewards();
        assert_eq!(rewards.len(), 2);

        // Find rewards by pubkey.
        let r1 = rewards.iter().find(|(k, _)| *k == key(1)).unwrap().1;
        let r2 = rewards.iter().find(|(k, _)| *k == key(2)).unwrap().1;

        // NoLock: 1_000_000 * 500 / (10_000 * 365) = 136
        assert_eq!(r1, 136);
        // Permanent: 1_000_000 * 1000 / (10_000 * 365) = 273
        assert_eq!(r2, 273);
    }

    #[test]
    fn test_voting_power_direct_stake() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::NoLock).unwrap();

        // 1_000_000 * 1.0 = 1_000_000
        assert_eq!(pool.get_voting_power(&key(1)), 1_000_000);
    }

    #[test]
    fn test_voting_power_tiered() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::Permanent).unwrap();

        // 1_000_000 * 5.0 = 5_000_000
        assert_eq!(pool.get_voting_power(&key(1)), 5_000_000);
    }

    #[test]
    fn test_voting_power_with_delegations() {
        let mut pool = StakingPool::new();
        // Validator has own stake.
        pool.stake(key(1), 1_000_000, LockTier::NoLock).unwrap();
        // Delegator delegates to validator.
        pool.delegate(key(2), key(1), 500_000, LockTier::OneYear).unwrap();

        // Validator power: 1_000_000 * 1.0 + 500_000 * 3.0 = 2_500_000
        assert_eq!(pool.get_voting_power(&key(1)), 2_500_000);
    }

    #[test]
    fn test_multiple_stakes_same_staker() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 500_000, LockTier::NoLock).unwrap();
        pool.stake(key(1), 500_000, LockTier::ThreeMonth).unwrap();
        assert_eq!(pool.total_staked(), 1_000_000);

        // Voting power: 500_000 * 1.0 + 500_000 * 1.5 = 1_250_000
        assert_eq!(pool.get_voting_power(&key(1)), 1_250_000);
    }

    #[test]
    fn test_partial_unstake() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 1_000_000, LockTier::NoLock).unwrap();
        pool.unstake(key(1), 300_000).unwrap();
        assert_eq!(pool.total_staked(), 700_000);
        assert_eq!(pool.get_voting_power(&key(1)), 700_000);
    }

    #[test]
    fn test_mixed_locked_unlocked_unstake() {
        let mut pool = StakingPool::new();
        pool.stake(key(1), 500_000, LockTier::NoLock).unwrap();
        pool.stake(key(1), 500_000, LockTier::ThreeMonth).unwrap();

        // Can only unstake the NoLock portion.
        pool.unstake(key(1), 500_000).unwrap();
        assert_eq!(pool.total_staked(), 500_000);

        // Trying to unstake more fails since the rest is locked.
        let result = pool.unstake(key(1), 100);
        assert!(matches!(result, Err(StakingError::InsufficientBalance { .. })));
    }
}
