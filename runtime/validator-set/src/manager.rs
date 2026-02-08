use trv1_staking::StakingPool;

use crate::rotation;
use crate::types::*;

/// The maximum number of active validators.
pub const ACTIVE_SET_CAP: usize = 200;

/// Manages the validator set: registration, rotation, proposer selection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidatorSetManager {
    state: ValidatorSetState,
}

impl ValidatorSetManager {
    /// Create a new validator set manager with default config.
    pub fn new() -> Self {
        Self {
            state: ValidatorSetState {
                config: ValidatorSetConfig::default(),
                ..Default::default()
            },
        }
    }

    /// Create with a custom config.
    pub fn with_config(config: ValidatorSetConfig) -> Self {
        Self {
            state: ValidatorSetState {
                config,
                ..Default::default()
            },
        }
    }

    /// Register a new validator. Placed into Active if room, otherwise Standby.
    pub fn register_validator(
        &mut self,
        pubkey: PublicKey,
        initial_stake: u64,
        commission_rate: u16,
        current_height: u64,
    ) -> ValidatorSetResult<ValidatorStatus> {
        if self.state.validators.contains_key(&pubkey) {
            return Err(ValidatorSetError::AlreadyRegistered(pubkey));
        }

        if initial_stake < self.state.config.min_stake {
            return Err(ValidatorSetError::InsufficientStake {
                have: initial_stake,
                need: self.state.config.min_stake,
            });
        }

        let active_count = self.active_count();
        let status = if active_count < self.state.config.active_set_cap {
            ValidatorStatus::Active
        } else {
            ValidatorStatus::Standby
        };

        let info = ValidatorInfo {
            pubkey,
            stake: initial_stake,
            commission_rate,
            status,
            performance_score: 10_000, // Start at perfect score.
            join_height: current_height,
        };

        self.state.validators.insert(pubkey, info);
        Ok(status)
    }

    /// Deregister a validator, removing them from the set entirely.
    pub fn deregister_validator(&mut self, pubkey: &PublicKey) -> ValidatorSetResult<ValidatorInfo> {
        self.state
            .validators
            .remove(pubkey)
            .ok_or(ValidatorSetError::NotFound(*pubkey))
    }

    /// Update the recorded stake for a validator.
    pub fn update_stake(
        &mut self,
        pubkey: &PublicKey,
        new_stake: u64,
    ) -> ValidatorSetResult<()> {
        let validator = self
            .state
            .validators
            .get_mut(pubkey)
            .ok_or(ValidatorSetError::NotFound(*pubkey))?;

        validator.stake = new_stake;
        Ok(())
    }

    /// Get the list of currently active validators, sorted by stake descending.
    pub fn get_active_set(&self, staking_pool: &StakingPool) -> Vec<ValidatorInfo> {
        let active: Vec<ValidatorInfo> = self
            .state
            .validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .cloned()
            .collect();

        let ranked = rotation::sort_by_stake(&active, staking_pool);
        ranked
            .iter()
            .filter_map(|r| self.state.validators.get(&r.pubkey).cloned())
            .collect()
    }

    /// Get the list of standby validators.
    pub fn get_standby_set(&self) -> Vec<ValidatorInfo> {
        self.state
            .validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Standby)
            .cloned()
            .collect()
    }

    /// Get the list of jailed validators.
    pub fn get_jailed_set(&self) -> Vec<ValidatorInfo> {
        self.state
            .validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Jailed)
            .cloned()
            .collect()
    }

    /// Trigger epoch rotation: swap standby validators in if they have higher
    /// effective stake than the weakest active validators.
    pub fn epoch_rotation(
        &mut self,
        staking_pool: &StakingPool,
    ) -> Vec<(PublicKey, PublicKey)> {
        let swaps = rotation::rotate(
            &mut self.state.validators,
            staking_pool,
            self.state.config.active_set_cap,
        );
        self.state.current_epoch += 1;
        swaps
    }

    /// Check if a validator is currently active.
    pub fn is_active(&self, pubkey: &PublicKey) -> bool {
        self.state
            .validators
            .get(pubkey)
            .map_or(false, |v| v.status == ValidatorStatus::Active)
    }

    /// Get information about a specific validator.
    pub fn get_validator(&self, pubkey: &PublicKey) -> Option<&ValidatorInfo> {
        self.state.validators.get(pubkey)
    }

    /// Get a mutable reference to a validator's info.
    pub fn get_validator_mut(&mut self, pubkey: &PublicKey) -> Option<&mut ValidatorInfo> {
        self.state.validators.get_mut(pubkey)
    }

    /// Jail a validator (move to Jailed status).
    pub fn jail(&mut self, pubkey: &PublicKey) -> ValidatorSetResult<()> {
        rotation::jail(&mut self.state.validators, pubkey)
    }

    /// Unjail a validator (move from Jailed to Standby).
    pub fn unjail(&mut self, pubkey: &PublicKey) -> ValidatorSetResult<()> {
        rotation::unjail(&mut self.state.validators, pubkey)
    }

    /// Deterministic proposer selection for a given block height.
    ///
    /// Uses weighted round-robin: validators are assigned contiguous ranges
    /// proportional to their voting power, and the height modulo total power
    /// determines who proposes.
    pub fn get_proposer(
        &self,
        height: u64,
        staking_pool: &StakingPool,
    ) -> Option<PublicKey> {
        let mut active: Vec<(PublicKey, u64)> = self
            .state
            .validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .map(|v| (v.pubkey, staking_pool.get_voting_power(&v.pubkey)))
            .collect();

        if active.is_empty() {
            return None;
        }

        // Sort deterministically by pubkey for consistent ordering.
        active.sort_by_key(|(pk, _)| *pk);

        let total_power: u64 = active.iter().map(|(_, p)| *p).sum();
        if total_power == 0 {
            return None;
        }

        let slot = height % total_power;
        let mut cumulative = 0u64;
        for (pubkey, power) in &active {
            cumulative += *power;
            if slot < cumulative {
                return Some(*pubkey);
            }
        }

        // Should not reach here, but return the last validator as fallback.
        active.last().map(|(pk, _)| *pk)
    }

    /// Current epoch.
    pub fn current_epoch(&self) -> u64 {
        self.state.current_epoch
    }

    /// Number of active validators.
    pub fn active_count(&self) -> usize {
        self.state
            .validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .count()
    }

    /// Total number of registered validators (all statuses).
    pub fn total_count(&self) -> usize {
        self.state.validators.len()
    }

    /// Access the full validators map (for slashing module).
    pub fn validators(&self) -> &std::collections::HashMap<PublicKey, ValidatorInfo> {
        &self.state.validators
    }

    /// Mutable access to the validators map (for slashing module).
    pub fn validators_mut(&mut self) -> &mut std::collections::HashMap<PublicKey, ValidatorInfo> {
        &mut self.state.validators
    }
}

impl Default for ValidatorSetManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trv1_staking::LockTier;

    fn pubkey(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    fn setup_pool_and_manager(
        entries: &[(u8, u64, LockTier)],
        cap: usize,
    ) -> (StakingPool, ValidatorSetManager) {
        let mut pool = StakingPool::new();
        let mut manager = ValidatorSetManager::with_config(ValidatorSetConfig {
            active_set_cap: cap,
            epoch_length: 100,
            min_stake: 100,
        });

        for &(n, amount, tier) in entries {
            pool.stake(pubkey(n), amount, tier).unwrap();
            manager
                .register_validator(pubkey(n), amount, 500, 0)
                .unwrap();
        }
        (pool, manager)
    }

    #[test]
    fn register_validator_active_when_room() {
        let mut manager = ValidatorSetManager::with_config(ValidatorSetConfig {
            active_set_cap: 2,
            min_stake: 100,
            ..Default::default()
        });

        let status = manager.register_validator(pubkey(1), 1000, 500, 0).unwrap();
        assert_eq!(status, ValidatorStatus::Active);
        assert_eq!(manager.active_count(), 1);
    }

    #[test]
    fn register_validator_standby_when_full() {
        let mut manager = ValidatorSetManager::with_config(ValidatorSetConfig {
            active_set_cap: 1,
            min_stake: 100,
            ..Default::default()
        });

        manager.register_validator(pubkey(1), 1000, 500, 0).unwrap();
        let status = manager.register_validator(pubkey(2), 2000, 500, 0).unwrap();
        assert_eq!(status, ValidatorStatus::Standby);
    }

    #[test]
    fn register_duplicate_fails() {
        let mut manager = ValidatorSetManager::new();
        manager
            .register_validator(pubkey(1), 1_000_000, 500, 0)
            .unwrap();
        let result = manager.register_validator(pubkey(1), 2_000_000, 500, 0);
        assert_eq!(result, Err(ValidatorSetError::AlreadyRegistered(pubkey(1))));
    }

    #[test]
    fn register_below_min_stake_fails() {
        let mut manager = ValidatorSetManager::new(); // min_stake = 1_000_000
        let result = manager.register_validator(pubkey(1), 999, 500, 0);
        assert!(matches!(result, Err(ValidatorSetError::InsufficientStake { .. })));
    }

    #[test]
    fn deregister_validator() {
        let mut manager = ValidatorSetManager::with_config(ValidatorSetConfig {
            min_stake: 100,
            ..Default::default()
        });
        manager.register_validator(pubkey(1), 1000, 500, 0).unwrap();
        let removed = manager.deregister_validator(&pubkey(1)).unwrap();
        assert_eq!(removed.pubkey, pubkey(1));
        assert_eq!(manager.total_count(), 0);
    }

    #[test]
    fn deregister_not_found() {
        let mut manager = ValidatorSetManager::new();
        let result = manager.deregister_validator(&pubkey(99));
        assert_eq!(result, Err(ValidatorSetError::NotFound(pubkey(99))));
    }

    #[test]
    fn update_stake() {
        let mut manager = ValidatorSetManager::with_config(ValidatorSetConfig {
            min_stake: 100,
            ..Default::default()
        });
        manager.register_validator(pubkey(1), 1000, 500, 0).unwrap();
        manager.update_stake(&pubkey(1), 5000).unwrap();
        assert_eq!(manager.get_validator(&pubkey(1)).unwrap().stake, 5000);
    }

    #[test]
    fn active_set_cap_200() {
        assert_eq!(ACTIVE_SET_CAP, 200);
        let config = ValidatorSetConfig::default();
        assert_eq!(config.active_set_cap, ACTIVE_SET_CAP);
    }

    #[test]
    fn epoch_rotation_swaps() {
        let (pool, mut manager) = setup_pool_and_manager(
            &[
                (1, 100, LockTier::Delegator),   // weakest active
                (2, 200, LockTier::Delegator),   // active
                (3, 500, LockTier::Delegator),   // should start standby since cap=2
            ],
            2,
        );

        // Validator 3 is in standby (registered third, cap=2).
        assert_eq!(manager.get_validator(&pubkey(3)).unwrap().status, ValidatorStatus::Standby);

        // Rotation should swap 3 in for 1.
        let swaps = manager.epoch_rotation(&pool);
        assert_eq!(swaps.len(), 1);
        assert!(manager.is_active(&pubkey(3)));
        assert!(!manager.is_active(&pubkey(1)));
    }

    #[test]
    fn proposer_selection_deterministic() {
        let (pool, manager) = setup_pool_and_manager(
            &[
                (1, 1000, LockTier::Delegator),
                (2, 1000, LockTier::Delegator),
            ],
            10,
        );

        // Same height should always give same proposer.
        let p1 = manager.get_proposer(42, &pool);
        let p2 = manager.get_proposer(42, &pool);
        assert_eq!(p1, p2);
        assert!(p1.is_some());
    }

    #[test]
    fn proposer_weighted_distribution() {
        let (pool, manager) = setup_pool_and_manager(
            &[
                (1, 3000, LockTier::Delegator), // 3000 power
                (2, 1000, LockTier::Delegator), // 1000 power
            ],
            10,
        );

        let total_power = 4000u64;
        let mut count_1 = 0u64;
        let mut count_2 = 0u64;

        for h in 0..total_power {
            match manager.get_proposer(h, &pool) {
                Some(pk) if pk == pubkey(1) => count_1 += 1,
                Some(pk) if pk == pubkey(2) => count_2 += 1,
                _ => {}
            }
        }

        // Validator 1 should get ~3x the slots of validator 2.
        assert_eq!(count_1, 3000);
        assert_eq!(count_2, 1000);
    }

    #[test]
    fn proposer_empty_set_returns_none() {
        let pool = StakingPool::new();
        let manager = ValidatorSetManager::new();
        assert_eq!(manager.get_proposer(0, &pool), None);
    }

    #[test]
    fn jail_and_unjail_via_manager() {
        let mut manager = ValidatorSetManager::with_config(ValidatorSetConfig {
            min_stake: 100,
            ..Default::default()
        });
        manager.register_validator(pubkey(1), 1000, 500, 0).unwrap();

        manager.jail(&pubkey(1)).unwrap();
        assert_eq!(
            manager.get_validator(&pubkey(1)).unwrap().status,
            ValidatorStatus::Jailed
        );

        manager.unjail(&pubkey(1)).unwrap();
        assert_eq!(
            manager.get_validator(&pubkey(1)).unwrap().status,
            ValidatorStatus::Standby
        );
    }

    #[test]
    fn get_active_set_sorted_by_stake() {
        let (pool, manager) = setup_pool_and_manager(
            &[
                (1, 100, LockTier::Delegator),
                (2, 300, LockTier::Delegator),
                (3, 200, LockTier::Delegator),
            ],
            10,
        );

        let active = manager.get_active_set(&pool);
        assert_eq!(active.len(), 3);
        assert_eq!(active[0].pubkey, pubkey(2)); // 300
        assert_eq!(active[1].pubkey, pubkey(3)); // 200
        assert_eq!(active[2].pubkey, pubkey(1)); // 100
    }

    #[test]
    fn rotation_increments_epoch() {
        let (pool, mut manager) = setup_pool_and_manager(
            &[(1, 1000, LockTier::Delegator)],
            10,
        );

        assert_eq!(manager.current_epoch(), 0);
        manager.epoch_rotation(&pool);
        assert_eq!(manager.current_epoch(), 1);
    }
}
