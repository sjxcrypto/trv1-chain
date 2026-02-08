use std::collections::HashMap;

use trv1_staking::StakingPool;
use trv1_validator_set::ValidatorSetManager;

use crate::evidence::EvidencePool;
use crate::types::*;

/// The slashing engine — processes evidence and applies penalties.
///
/// CRITICAL INVARIANT: Only the validator's own stake is slashed.
/// Delegator stake is never touched by slashing.
pub struct SlashingEngine {
    config: SlashingConfig,
    evidence_pool: EvidencePool,
    /// History of all slash events keyed by offender pubkey.
    slash_history: HashMap<PublicKey, Vec<SlashEvent>>,
    /// Accumulated treasury balance from slashed stake.
    pub treasury: u64,
}

impl SlashingEngine {
    /// Create a new slashing engine with default config.
    pub fn new() -> Self {
        Self {
            config: SlashingConfig::default(),
            evidence_pool: EvidencePool::new(),
            slash_history: HashMap::new(),
            treasury: 0,
        }
    }

    /// Create with custom config.
    pub fn with_config(config: SlashingConfig) -> Self {
        Self {
            config,
            evidence_pool: EvidencePool::new(),
            slash_history: HashMap::new(),
            treasury: 0,
        }
    }

    /// Submit evidence to the pool.
    pub fn submit_evidence(&mut self, evidence: EvidenceRecord) -> SlashingResult<[u8; 32]> {
        self.evidence_pool.submit_evidence(evidence)
    }

    /// Process all pending evidence, slashing and jailing offenders.
    ///
    /// Returns a list of slash events that were applied.
    pub fn process_all_evidence(
        &mut self,
        validator_set: &mut ValidatorSetManager,
        staking_pool: &mut StakingPool,
    ) -> Vec<SlashEvent> {
        let pending: Vec<EvidenceRecord> = self
            .evidence_pool
            .get_pending_evidence()
            .into_iter()
            .cloned()
            .collect();

        let mut events = Vec::new();
        for evidence in &pending {
            if let Some(event) =
                self.process_single_evidence(evidence, validator_set, staking_pool)
            {
                events.push(event);
            }
            self.evidence_pool.mark_processed(&evidence.hash());
        }
        events
    }

    /// Process a single evidence record.
    fn process_single_evidence(
        &mut self,
        evidence: &EvidenceRecord,
        validator_set: &mut ValidatorSetManager,
        staking_pool: &mut StakingPool,
    ) -> Option<SlashEvent> {
        // Verify the offender is a registered validator.
        let validator = match validator_set.get_validator(&evidence.offender) {
            Some(v) => v.clone(),
            None => {
                tracing::warn!(
                    offender = ?evidence.offender,
                    "evidence for unknown validator, skipping"
                );
                return None;
            }
        };

        // Calculate slash amount based ONLY on the validator's own stake.
        let slash_bps = self.config.slash_bps(&evidence.offense);
        let validator_own_stake = validator.stake;
        let slash_amount = (validator_own_stake as u128 * slash_bps as u128 / 10_000) as u64;

        if slash_amount == 0 {
            tracing::debug!(offender = ?evidence.offender, "slash amount is zero, skipping");
            return None;
        }

        // Apply the slash — reduce the validator's recorded stake.
        let new_stake = validator_own_stake.saturating_sub(slash_amount);
        if let Err(e) = validator_set.update_stake(&evidence.offender, new_stake) {
            tracing::error!(
                offender = ?evidence.offender,
                error = %e,
                "failed to update stake during slash"
            );
            return None;
        }

        // Unstake the slashed amount from the staking pool (validator's own entry).
        // If the validator's stake has an unlocked portion, remove it. If all locked,
        // we still reduce the recorded stake above; the staking pool unstake is best-effort.
        let _ = staking_pool.unstake(evidence.offender, slash_amount);

        // Send slashed amount to treasury.
        self.treasury = self.treasury.saturating_add(slash_amount);

        // Jail the offender.
        if let Err(e) = validator_set.jail(&evidence.offender) {
            tracing::warn!(
                offender = ?evidence.offender,
                error = %e,
                "failed to jail validator (may already be jailed)"
            );
        }

        let event = SlashEvent {
            offender: evidence.offender,
            offense: evidence.offense,
            slash_amount,
            height: evidence.height,
            evidence_hash: evidence.hash(),
        };

        tracing::info!(
            offender = ?evidence.offender,
            offense = %evidence.offense,
            slash_amount,
            height = evidence.height,
            "validator slashed and jailed"
        );

        // Record in history.
        self.slash_history
            .entry(evidence.offender)
            .or_default()
            .push(event.clone());

        Some(event)
    }

    /// Slash a validator directly (without evidence submission).
    pub fn slash_validator(
        &mut self,
        pubkey: &PublicKey,
        offense: SlashingOffense,
        height: u64,
        validator_set: &mut ValidatorSetManager,
        staking_pool: &mut StakingPool,
    ) -> SlashingResult<SlashEvent> {
        let evidence = EvidenceRecord {
            offense,
            offender: *pubkey,
            height,
            data: b"direct_slash".to_vec(),
            processed: false,
        };

        let event = self
            .process_single_evidence(&evidence, validator_set, staking_pool)
            .ok_or(SlashingError::ValidatorNotFound(*pubkey))?;

        Ok(event)
    }

    /// Get slash history for a specific validator.
    pub fn get_slash_history(&self, pubkey: &PublicKey) -> &[SlashEvent] {
        self.slash_history
            .get(pubkey)
            .map_or(&[], |v| v.as_slice())
    }

    /// Access the evidence pool.
    pub fn evidence_pool(&self) -> &EvidencePool {
        &self.evidence_pool
    }

    /// Access the slashing config.
    pub fn config(&self) -> &SlashingConfig {
        &self.config
    }
}

impl Default for SlashingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trv1_staking::LockTier;
    use trv1_validator_set::{ValidatorSetConfig, ValidatorStatus};

    fn pubkey(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    fn setup() -> (SlashingEngine, ValidatorSetManager, StakingPool) {
        let engine = SlashingEngine::new();
        let mut vs = ValidatorSetManager::with_config(ValidatorSetConfig {
            active_set_cap: 200,
            epoch_length: 100,
            min_stake: 100,
        });
        let mut pool = StakingPool::new();

        // Register validator 1 with 10,000 stake.
        pool.stake(pubkey(1), 10_000, LockTier::Delegator).unwrap();
        vs.register_validator(pubkey(1), 10_000, 500, 0).unwrap();

        // Register validator 2 with 20,000 stake.
        pool.stake(pubkey(2), 20_000, LockTier::Delegator).unwrap();
        vs.register_validator(pubkey(2), 20_000, 500, 0).unwrap();

        // Delegator 3 delegates to validator 1.
        pool.delegate(pubkey(3), pubkey(1), 5_000, LockTier::Delegator)
            .unwrap();

        (engine, vs, pool)
    }

    #[test]
    fn slash_double_sign_5_percent() {
        let (mut engine, mut vs, mut pool) = setup();

        let event = engine
            .slash_validator(&pubkey(1), SlashingOffense::DoubleSign, 100, &mut vs, &mut pool)
            .unwrap();

        // 10,000 * 5% = 500 slashed.
        assert_eq!(event.slash_amount, 500);
        assert_eq!(event.offense, SlashingOffense::DoubleSign);

        // Validator's recorded stake should be reduced.
        assert_eq!(vs.get_validator(&pubkey(1)).unwrap().stake, 9_500);

        // Slashed amount goes to treasury.
        assert_eq!(engine.treasury, 500);
    }

    #[test]
    fn slash_downtime_1_percent() {
        let (mut engine, mut vs, mut pool) = setup();

        let event = engine
            .slash_validator(&pubkey(2), SlashingOffense::Downtime, 50, &mut vs, &mut pool)
            .unwrap();

        // 20,000 * 1% = 200 slashed.
        assert_eq!(event.slash_amount, 200);
        assert_eq!(vs.get_validator(&pubkey(2)).unwrap().stake, 19_800);
        assert_eq!(engine.treasury, 200);
    }

    #[test]
    fn slash_invalid_block_10_percent() {
        let (mut engine, mut vs, mut pool) = setup();

        let event = engine
            .slash_validator(&pubkey(1), SlashingOffense::InvalidBlock, 200, &mut vs, &mut pool)
            .unwrap();

        // 10,000 * 10% = 1,000 slashed.
        assert_eq!(event.slash_amount, 1_000);
        assert_eq!(vs.get_validator(&pubkey(1)).unwrap().stake, 9_000);
        assert_eq!(engine.treasury, 1_000);
    }

    #[test]
    fn delegators_are_never_slashed() {
        let (mut engine, mut vs, mut pool) = setup();

        // Delegator 3 has 5,000 delegated to validator 1.
        let delegator_power_before = pool.get_voting_power(&pubkey(3));

        // Slash validator 1.
        engine
            .slash_validator(&pubkey(1), SlashingOffense::DoubleSign, 100, &mut vs, &mut pool)
            .unwrap();

        // Delegator's voting power should be unchanged.
        let delegator_power_after = pool.get_voting_power(&pubkey(3));
        assert_eq!(delegator_power_before, delegator_power_after);

        // Delegator 3 is not a validator, so not in validator set — not affected.
        // The key invariant: only validator 1's OWN stake was slashed, not
        // the 5,000 that delegator 3 delegated to them.
    }

    #[test]
    fn validator_jailed_after_slash() {
        let (mut engine, mut vs, mut pool) = setup();

        engine
            .slash_validator(&pubkey(1), SlashingOffense::DoubleSign, 100, &mut vs, &mut pool)
            .unwrap();

        assert_eq!(
            vs.get_validator(&pubkey(1)).unwrap().status,
            ValidatorStatus::Jailed
        );
    }

    #[test]
    fn slash_history_recorded() {
        let (mut engine, mut vs, mut pool) = setup();

        engine
            .slash_validator(&pubkey(1), SlashingOffense::DoubleSign, 100, &mut vs, &mut pool)
            .unwrap();

        let history = engine.get_slash_history(&pubkey(1));
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].offense, SlashingOffense::DoubleSign);
        assert_eq!(history[0].slash_amount, 500);
    }

    #[test]
    fn process_evidence_via_pool() {
        let (mut engine, mut vs, mut pool) = setup();

        let evidence = EvidenceRecord {
            offense: SlashingOffense::DoubleSign,
            offender: pubkey(1),
            height: 100,
            data: b"vote_a:vote_b".to_vec(),
            processed: false,
        };

        engine.submit_evidence(evidence).unwrap();
        let events = engine.process_all_evidence(&mut vs, &mut pool);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].slash_amount, 500);
        assert_eq!(
            vs.get_validator(&pubkey(1)).unwrap().status,
            ValidatorStatus::Jailed
        );

        // Evidence should now be marked as processed.
        assert_eq!(engine.evidence_pool().get_pending_evidence().len(), 0);
    }

    #[test]
    fn slash_unknown_validator_fails() {
        let (mut engine, mut vs, mut pool) = setup();

        let result = engine.slash_validator(
            &pubkey(99),
            SlashingOffense::DoubleSign,
            100,
            &mut vs,
            &mut pool,
        );
        assert!(matches!(result, Err(SlashingError::ValidatorNotFound(_))));
    }

    #[test]
    fn multiple_slashes_accumulate() {
        let (mut engine, mut vs, mut pool) = setup();

        // First slash: 10,000 * 5% = 500.
        engine
            .slash_validator(&pubkey(1), SlashingOffense::DoubleSign, 100, &mut vs, &mut pool)
            .unwrap();

        // Unjail so we can slash again.
        vs.unjail(&pubkey(1)).unwrap();

        // Second slash: 9,500 * 1% = 95.
        engine
            .slash_validator(&pubkey(1), SlashingOffense::Downtime, 200, &mut vs, &mut pool)
            .unwrap();

        assert_eq!(vs.get_validator(&pubkey(1)).unwrap().stake, 9_405);
        assert_eq!(engine.treasury, 595); // 500 + 95
        assert_eq!(engine.get_slash_history(&pubkey(1)).len(), 2);
    }

    #[test]
    fn treasury_accumulates_from_different_validators() {
        let (mut engine, mut vs, mut pool) = setup();

        engine
            .slash_validator(&pubkey(1), SlashingOffense::DoubleSign, 100, &mut vs, &mut pool)
            .unwrap();
        engine
            .slash_validator(&pubkey(2), SlashingOffense::Downtime, 100, &mut vs, &mut pool)
            .unwrap();

        // Validator 1: 10,000 * 5% = 500.
        // Validator 2: 20,000 * 1% = 200.
        assert_eq!(engine.treasury, 700);
    }
}
