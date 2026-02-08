use trv1_staking::StakingPool;

use crate::types::*;

/// A ranked entry pairing a validator with their effective stake.
#[derive(Debug, Clone)]
pub struct RankedValidator {
    pub pubkey: PublicKey,
    pub effective_stake: u64,
}

/// Sort validators by effective stake in descending order.
/// Uses the staking pool to look up each validator's vote weight.
pub fn sort_by_stake(
    validators: &[ValidatorInfo],
    staking_pool: &StakingPool,
) -> Vec<RankedValidator> {
    let mut ranked: Vec<RankedValidator> = validators
        .iter()
        .map(|v| {
            let voting_power = staking_pool.get_voting_power(&v.pubkey);
            RankedValidator {
                pubkey: v.pubkey,
                effective_stake: voting_power,
            }
        })
        .collect();

    // Sort descending by effective stake; ties broken by pubkey (lexicographic, for determinism).
    ranked.sort_by(|a, b| {
        b.effective_stake
            .cmp(&a.effective_stake)
            .then_with(|| a.pubkey.cmp(&b.pubkey))
    });

    ranked
}

/// Perform epoch rotation.
///
/// If any standby validator has higher effective stake than the lowest-ranked
/// active validator, swap them. Returns a list of (promoted, demoted) pairs.
pub fn rotate(
    validators: &mut std::collections::HashMap<PublicKey, ValidatorInfo>,
    staking_pool: &StakingPool,
    active_cap: usize,
) -> Vec<(PublicKey, PublicKey)> {
    let mut swaps = Vec::new();

    loop {
        // Gather active and standby validators.
        let active: Vec<ValidatorInfo> = validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .cloned()
            .collect();

        let standby: Vec<ValidatorInfo> = validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Standby)
            .cloned()
            .collect();

        if active.is_empty() || standby.is_empty() {
            break;
        }

        // Find the weakest active validator.
        let active_ranked = sort_by_stake(&active, staking_pool);
        let weakest_active = active_ranked.last().unwrap();

        // Find the strongest standby validator.
        let standby_ranked = sort_by_stake(&standby, staking_pool);
        let strongest_standby = standby_ranked.first().unwrap();

        // Only swap if standby is strictly stronger.
        if strongest_standby.effective_stake > weakest_active.effective_stake {
            // Demote the weakest active validator.
            if let Some(v) = validators.get_mut(&weakest_active.pubkey) {
                v.status = ValidatorStatus::Standby;
            }
            // Promote the strongest standby validator.
            if let Some(v) = validators.get_mut(&strongest_standby.pubkey) {
                v.status = ValidatorStatus::Active;
            }
            swaps.push((strongest_standby.pubkey, weakest_active.pubkey));
        } else {
            break;
        }
    }

    // If active count is below cap, promote standby validators by stake order.
    let active_count = validators
        .values()
        .filter(|v| v.status == ValidatorStatus::Active)
        .count();

    if active_count < active_cap {
        let standby: Vec<ValidatorInfo> = validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Standby)
            .cloned()
            .collect();

        let standby_ranked = sort_by_stake(&standby, staking_pool);
        let slots_available = active_cap - active_count;

        for ranked in standby_ranked.iter().take(slots_available) {
            if let Some(v) = validators.get_mut(&ranked.pubkey) {
                v.status = ValidatorStatus::Active;
            }
        }
    }

    swaps
}

/// Jail a validator — move them to Jailed status regardless of current status.
pub fn jail(
    validators: &mut std::collections::HashMap<PublicKey, ValidatorInfo>,
    pubkey: &PublicKey,
) -> ValidatorSetResult<()> {
    let validator = validators
        .get_mut(pubkey)
        .ok_or(ValidatorSetError::NotFound(*pubkey))?;

    validator.status = ValidatorStatus::Jailed;
    Ok(())
}

/// Unjail a validator — move them from Jailed back to Standby.
pub fn unjail(
    validators: &mut std::collections::HashMap<PublicKey, ValidatorInfo>,
    pubkey: &PublicKey,
) -> ValidatorSetResult<()> {
    let validator = validators
        .get_mut(pubkey)
        .ok_or(ValidatorSetError::NotFound(*pubkey))?;

    if validator.status != ValidatorStatus::Jailed {
        return Err(ValidatorSetError::NotJailed);
    }

    validator.status = ValidatorStatus::Standby;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use trv1_staking::LockTier;

    fn pubkey(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    fn make_validator(n: u8, stake: u64, status: ValidatorStatus) -> ValidatorInfo {
        ValidatorInfo {
            pubkey: pubkey(n),
            stake,
            commission_rate: 500,
            status,
            performance_score: 10_000,
            join_height: 0,
        }
    }

    fn setup_pool_with_stakes(entries: &[(u8, u64, LockTier)]) -> StakingPool {
        let mut pool = StakingPool::new();
        for &(n, amount, tier) in entries {
            pool.stake(pubkey(n), amount, tier).unwrap();
        }
        pool
    }

    #[test]
    fn sort_by_stake_descending() {
        let pool = setup_pool_with_stakes(&[
            (1, 100, LockTier::NoLock),
            (2, 300, LockTier::NoLock),
            (3, 200, LockTier::NoLock),
        ]);

        let validators = vec![
            make_validator(1, 100, ValidatorStatus::Active),
            make_validator(2, 300, ValidatorStatus::Active),
            make_validator(3, 200, ValidatorStatus::Active),
        ];

        let ranked = sort_by_stake(&validators, &pool);
        assert_eq!(ranked[0].pubkey, pubkey(2)); // 300
        assert_eq!(ranked[1].pubkey, pubkey(3)); // 200
        assert_eq!(ranked[2].pubkey, pubkey(1)); // 100
    }

    #[test]
    fn rotation_swaps_stronger_standby_for_weaker_active() {
        let pool = setup_pool_with_stakes(&[
            (1, 100, LockTier::NoLock),  // active, weakest
            (2, 200, LockTier::NoLock),  // active
            (3, 500, LockTier::NoLock),  // standby, strongest
        ]);

        let mut validators: HashMap<PublicKey, ValidatorInfo> = HashMap::new();
        validators.insert(pubkey(1), make_validator(1, 100, ValidatorStatus::Active));
        validators.insert(pubkey(2), make_validator(2, 200, ValidatorStatus::Active));
        validators.insert(pubkey(3), make_validator(3, 500, ValidatorStatus::Standby));

        let swaps = rotate(&mut validators, &pool, 2);

        // Validator 3 (500 stake) should replace validator 1 (100 stake).
        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0], (pubkey(3), pubkey(1)));

        assert_eq!(validators[&pubkey(3)].status, ValidatorStatus::Active);
        assert_eq!(validators[&pubkey(1)].status, ValidatorStatus::Standby);
    }

    #[test]
    fn rotation_no_swap_when_all_active_stronger() {
        let pool = setup_pool_with_stakes(&[
            (1, 500, LockTier::NoLock),
            (2, 300, LockTier::NoLock),
            (3, 100, LockTier::NoLock),
        ]);

        let mut validators: HashMap<PublicKey, ValidatorInfo> = HashMap::new();
        validators.insert(pubkey(1), make_validator(1, 500, ValidatorStatus::Active));
        validators.insert(pubkey(2), make_validator(2, 300, ValidatorStatus::Active));
        validators.insert(pubkey(3), make_validator(3, 100, ValidatorStatus::Standby));

        let swaps = rotate(&mut validators, &pool, 2);
        assert!(swaps.is_empty());
    }

    #[test]
    fn rotation_promotes_standby_when_cap_not_full() {
        let pool = setup_pool_with_stakes(&[
            (1, 500, LockTier::NoLock),
            (2, 300, LockTier::NoLock),
        ]);

        let mut validators: HashMap<PublicKey, ValidatorInfo> = HashMap::new();
        validators.insert(pubkey(1), make_validator(1, 500, ValidatorStatus::Active));
        validators.insert(pubkey(2), make_validator(2, 300, ValidatorStatus::Standby));

        let _ = rotate(&mut validators, &pool, 5);

        // Validator 2 should be promoted since cap (5) > active count (1).
        assert_eq!(validators[&pubkey(2)].status, ValidatorStatus::Active);
    }

    #[test]
    fn jail_and_unjail() {
        let mut validators: HashMap<PublicKey, ValidatorInfo> = HashMap::new();
        validators.insert(pubkey(1), make_validator(1, 100, ValidatorStatus::Active));

        jail(&mut validators, &pubkey(1)).unwrap();
        assert_eq!(validators[&pubkey(1)].status, ValidatorStatus::Jailed);

        unjail(&mut validators, &pubkey(1)).unwrap();
        assert_eq!(validators[&pubkey(1)].status, ValidatorStatus::Standby);
    }

    #[test]
    fn unjail_non_jailed_fails() {
        let mut validators: HashMap<PublicKey, ValidatorInfo> = HashMap::new();
        validators.insert(pubkey(1), make_validator(1, 100, ValidatorStatus::Active));

        let result = unjail(&mut validators, &pubkey(1));
        assert_eq!(result, Err(ValidatorSetError::NotJailed));
    }

    #[test]
    fn jail_not_found() {
        let mut validators: HashMap<PublicKey, ValidatorInfo> = HashMap::new();
        let result = jail(&mut validators, &pubkey(99));
        assert_eq!(result, Err(ValidatorSetError::NotFound(pubkey(99))));
    }
}
