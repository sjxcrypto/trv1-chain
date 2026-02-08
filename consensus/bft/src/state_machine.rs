use std::collections::HashMap;

use crate::block::Block;
use crate::round::{RoundState, RoundStep};
use crate::types::*;

/// Pure BFT consensus state machine.
///
/// Takes input events (proposals, votes, timeouts) and returns output
/// messages (votes to cast, blocks to commit, timeouts to schedule).
/// No I/O — the caller is responsible for networking and timers.
pub struct BftStateMachine {
    pub height: Height,
    pub round: Round,
    pub step: RoundStep,
    /// Our validator signing key index in the validator set.
    pub validator_index: Option<usize>,
    /// The ordered set of validators for the current height.
    pub validators: Vec<ValidatorId>,
    /// Current round state (votes collected, proposal seen).
    pub round_state: RoundState,
    /// Locked value: the block hash we have precommitted for.
    pub locked_value: Option<BlockHash>,
    pub locked_round: Option<Round>,
    /// Valid value: the block hash we have seen a polka (2/3+ prevotes) for.
    pub valid_value: Option<BlockHash>,
    pub valid_round: Option<Round>,
    /// Timeout configuration.
    pub timeout_config: TimeoutConfig,
    /// Cache of blocks received with proposals, keyed by block hash.
    pub proposed_blocks: HashMap<BlockHash, Block>,
}

impl BftStateMachine {
    /// Create a new BFT state machine for the given height and validator set.
    pub fn new(
        height: Height,
        validators: Vec<ValidatorId>,
        validator_index: Option<usize>,
        timeout_config: TimeoutConfig,
    ) -> Self {
        let total = validators.len();
        let round = Round(0);
        Self {
            height,
            round,
            step: RoundStep::NewRound,
            validator_index,
            validators,
            round_state: RoundState::new(round, height, total),
            locked_value: None,
            locked_round: None,
            valid_value: None,
            valid_round: None,
            timeout_config,
            proposed_blocks: HashMap::new(),
        }
    }

    /// Start a new round. Returns messages to send (e.g., schedule propose timeout).
    pub fn start_round(&mut self, round: Round) -> Vec<ConsensusMessage> {
        self.round = round;
        self.step = RoundStep::Propose;
        let total = self.validators.len();
        self.round_state = RoundState::new(round, self.height, total);

        let mut out = Vec::new();

        // Schedule propose timeout
        out.push(ConsensusMessage::ScheduleTimeout(TimeoutEvent {
            height: self.height,
            round: self.round,
            step: TimeoutStep::Propose,
        }));

        out
    }

    /// Determine the proposer for a given height and round via simple rotation.
    pub fn proposer_index(&self, height: Height, round: Round) -> usize {
        let n = self.validators.len();
        if n == 0 {
            return 0;
        }
        ((height.0 as usize) + (round.0 as usize)) % n
    }

    /// Whether we are the proposer for the current height/round.
    pub fn is_proposer(&self) -> bool {
        match self.validator_index {
            Some(idx) => self.proposer_index(self.height, self.round) == idx,
            None => false,
        }
    }

    /// Handle an incoming proposal, optionally with the full block data.
    pub fn on_proposal(&mut self, proposal: &Proposal, block: Option<&Block>) -> Vec<ConsensusMessage> {
        let mut out = Vec::new();

        // Validate proposal metadata
        if proposal.height != self.height || proposal.round != self.round {
            return out;
        }
        if self.step != RoundStep::Propose {
            return out;
        }

        // Verify proposer is correct
        let expected_idx = self.proposer_index(self.height, self.round);
        if expected_idx >= self.validators.len() {
            return out;
        }
        if proposal.proposer != self.validators[expected_idx] {
            return out;
        }

        // If a block was provided, verify its hash matches the proposal and cache it
        if let Some(blk) = block {
            if blk.hash() != proposal.block_hash {
                // Block hash mismatch — reject this proposal
                return out;
            }
            self.proposed_blocks.insert(proposal.block_hash, blk.clone());
        }

        self.round_state.proposal = Some(proposal.block_hash);

        // Decide prevote: respect locking rules
        let prevote_hash = if let Some(locked) = self.locked_value {
            // We are locked: only prevote the proposal if it matches our lock
            // OR if the proposal has a valid_round >= our locked_round
            if proposal.block_hash == locked {
                Some(proposal.block_hash)
            } else if let (Some(prop_vr), Some(lr)) = (proposal.valid_round, self.locked_round) {
                if prop_vr >= lr {
                    Some(proposal.block_hash)
                } else {
                    None // prevote nil
                }
            } else {
                None // prevote nil
            }
        } else {
            Some(proposal.block_hash)
        };

        self.step = RoundStep::Prevote;

        // Emit our prevote
        out.push(ConsensusMessage::CastVote(Vote {
            vote_type: VoteType::Prevote,
            height: self.height,
            round: self.round,
            block_hash: prevote_hash,
            // Signature/validator are placeholders — the caller fills them with real signing
            validator: proposal.proposer.clone(),
            signature: proposal.signature,
        }));

        out
    }

    /// Handle an incoming prevote.
    pub fn on_prevote(&mut self, vote: &Vote) -> Vec<ConsensusMessage> {
        let mut out = Vec::new();

        if vote.height != self.height || vote.round != self.round {
            return out;
        }
        if vote.vote_type != VoteType::Prevote {
            return out;
        }

        // Add vote to the set (the VoteSet handles dedup and verification)
        self.round_state.prevotes.add_vote(vote.clone());

        // Check for transitions based on current step
        match self.step {
            RoundStep::Prevote => {
                // If we see 2/3+ prevotes for a block, transition to precommit
                if let Some(hash) = self.round_state.prevotes.quorum_block() {
                    // Got a polka for a block
                    self.valid_value = Some(hash);
                    self.valid_round = Some(self.round);

                    // Lock on it and precommit
                    self.locked_value = Some(hash);
                    self.locked_round = Some(self.round);
                    self.step = RoundStep::Precommit;

                    out.push(ConsensusMessage::CastVote(Vote {
                        vote_type: VoteType::Precommit,
                        height: self.height,
                        round: self.round,
                        block_hash: Some(hash),
                        validator: vote.validator.clone(),
                        signature: vote.signature,
                    }));
                } else if self.round_state.prevotes.has_quorum_for_nil() {
                    // 2/3+ nil prevotes → precommit nil
                    self.step = RoundStep::Precommit;
                    out.push(ConsensusMessage::CastVote(Vote {
                        vote_type: VoteType::Precommit,
                        height: self.height,
                        round: self.round,
                        block_hash: None,
                        validator: vote.validator.clone(),
                        signature: vote.signature,
                    }));
                } else if self.round_state.prevotes.has_two_thirds_any() {
                    // 2/3+ voted but no quorum for any single value → schedule prevote timeout
                    out.push(ConsensusMessage::ScheduleTimeout(TimeoutEvent {
                        height: self.height,
                        round: self.round,
                        step: TimeoutStep::Prevote,
                    }));
                }
            }
            RoundStep::Precommit => {
                // Even in precommit step, update valid_value if we see a new polka
                if let Some(hash) = self.round_state.prevotes.quorum_block() {
                    self.valid_value = Some(hash);
                    self.valid_round = Some(self.round);
                }
            }
            _ => {}
        }

        out
    }

    /// Handle an incoming precommit.
    pub fn on_precommit(&mut self, vote: &Vote) -> Vec<ConsensusMessage> {
        let mut out = Vec::new();

        if vote.height != self.height || vote.round != self.round {
            return out;
        }
        if vote.vote_type != VoteType::Precommit {
            return out;
        }

        self.round_state.precommits.add_vote(vote.clone());

        // Check for commit
        if let Some(hash) = self.round_state.precommits.quorum_block() {
            if self.step != RoundStep::Commit {
                self.step = RoundStep::Commit;
                out.push(ConsensusMessage::CommitBlock {
                    height: self.height,
                    block_hash: hash,
                });
            }
        } else if self.round_state.precommits.has_two_thirds_any()
            && self.step == RoundStep::Precommit
        {
            // 2/3+ precommits but no quorum for any block → schedule precommit timeout
            out.push(ConsensusMessage::ScheduleTimeout(TimeoutEvent {
                height: self.height,
                round: self.round,
                step: TimeoutStep::Precommit,
            }));
        }

        out
    }

    /// Handle a timeout event.
    pub fn on_timeout(&mut self, event: TimeoutEvent) -> Vec<ConsensusMessage> {
        let mut out = Vec::new();

        if event.height != self.height || event.round != self.round {
            return out;
        }

        match event.step {
            TimeoutStep::Propose => {
                if self.step == RoundStep::Propose {
                    // Propose timeout: prevote nil
                    self.step = RoundStep::Prevote;
                    out.push(ConsensusMessage::ScheduleTimeout(TimeoutEvent {
                        height: self.height,
                        round: self.round,
                        step: TimeoutStep::Prevote,
                    }));
                }
            }
            TimeoutStep::Prevote => {
                if self.step == RoundStep::Prevote {
                    // Prevote timeout: precommit nil
                    self.step = RoundStep::Precommit;
                    out.push(ConsensusMessage::CastVote(Vote {
                        vote_type: VoteType::Precommit,
                        height: self.height,
                        round: self.round,
                        block_hash: None,
                        // placeholder — caller fills real values
                        validator: self.validators[0].clone(),
                        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    }));
                }
            }
            TimeoutStep::Precommit => {
                if self.step == RoundStep::Precommit {
                    // Precommit timeout: advance to next round
                    let next_round = Round(self.round.0 + 1);
                    out.extend(self.start_round(next_round));
                }
            }
        }

        out
    }

    /// Retrieve a cached block by its hash (e.g., after commit).
    pub fn get_committed_block(&self, hash: &BlockHash) -> Option<&Block> {
        self.proposed_blocks.get(hash)
    }

    /// Advance to the next height after a commit.
    pub fn advance_height(&mut self, new_height: Height) -> Vec<ConsensusMessage> {
        self.height = new_height;
        self.locked_value = None;
        self.locked_round = None;
        self.valid_value = None;
        self.valid_round = None;
        self.proposed_blocks.clear();
        self.start_round(Round(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_validators(n: usize) -> (Vec<SigningKey>, Vec<ValidatorId>) {
        let keys: Vec<SigningKey> = (0..n).map(|_| SigningKey::generate(&mut OsRng)).collect();
        let ids: Vec<ValidatorId> = keys.iter().map(|k| ValidatorId(k.verifying_key())).collect();
        (keys, ids)
    }

    fn make_proposal(
        height: Height,
        round: Round,
        block_hash: BlockHash,
        signing_key: &SigningKey,
    ) -> Proposal {
        let msg = b"proposal";
        let sig = signing_key.sign(msg);
        Proposal {
            height,
            round,
            block_hash,
            proposer: ValidatorId(signing_key.verifying_key()),
            signature: sig,
            valid_round: None,
        }
    }

    fn make_signed_vote(
        vote_type: VoteType,
        height: Height,
        round: Round,
        block_hash: Option<BlockHash>,
        key: &SigningKey,
    ) -> Vote {
        Vote::new(vote_type, height, round, block_hash, key)
    }

    #[test]
    fn test_proposer_rotation() {
        let (_keys, ids) = make_validators(4);
        let sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());

        assert_eq!(sm.proposer_index(Height(0), Round(0)), 0);
        assert_eq!(sm.proposer_index(Height(0), Round(1)), 1);
        assert_eq!(sm.proposer_index(Height(1), Round(0)), 1);
        assert_eq!(sm.proposer_index(Height(3), Round(3)), 2); // (3+3)%4=2
    }

    #[test]
    fn test_start_round_schedules_timeout() {
        let (_keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(1), ids, Some(0), TimeoutConfig::default());

        let msgs = sm.start_round(Round(0));
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            ConsensusMessage::ScheduleTimeout(te) => {
                assert_eq!(te.step, TimeoutStep::Propose);
                assert_eq!(te.round, Round(0));
            }
            _ => panic!("expected ScheduleTimeout"),
        }
        assert_eq!(sm.step, RoundStep::Propose);
    }

    #[test]
    fn test_on_proposal_valid() {
        let (keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());
        sm.start_round(Round(0));

        let hash = BlockHash([0xAA; 32]);
        // Proposer for height=0, round=0 is index 0
        let proposal = make_proposal(Height(0), Round(0), hash, &keys[0]);
        let msgs = sm.on_proposal(&proposal, None);

        assert_eq!(sm.step, RoundStep::Prevote);
        assert_eq!(sm.round_state.proposal, Some(hash));
        assert!(msgs.len() == 1);
        match &msgs[0] {
            ConsensusMessage::CastVote(v) => {
                assert_eq!(v.vote_type, VoteType::Prevote);
                assert_eq!(v.block_hash, Some(hash));
            }
            _ => panic!("expected CastVote prevote"),
        }
    }

    #[test]
    fn test_on_proposal_wrong_proposer_ignored() {
        let (keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());
        sm.start_round(Round(0));

        let hash = BlockHash([0xBB; 32]);
        // keys[1] is not the proposer for height=0, round=0
        let proposal = make_proposal(Height(0), Round(0), hash, &keys[1]);
        let msgs = sm.on_proposal(&proposal, None);

        assert!(msgs.is_empty(), "wrong proposer should be ignored");
        assert_eq!(sm.step, RoundStep::Propose);
    }

    #[test]
    fn test_prevote_quorum_transitions_to_precommit() {
        let (keys, ids) = make_validators(4);
        let hash = BlockHash([0xCC; 32]);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Prevote;

        // Feed 3 prevotes for the block (quorum = 3 of 4)
        for key in &keys[0..3] {
            let vote = make_signed_vote(VoteType::Prevote, Height(0), Round(0), Some(hash), key);
            sm.on_prevote(&vote);
        }

        assert_eq!(sm.step, RoundStep::Precommit);
        assert_eq!(sm.locked_value, Some(hash));
        assert_eq!(sm.valid_value, Some(hash));
    }

    #[test]
    fn test_nil_prevote_quorum_transitions_to_precommit() {
        let (keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Prevote;

        // 3 nil prevotes
        for key in &keys[0..3] {
            let vote = make_signed_vote(VoteType::Prevote, Height(0), Round(0), None, key);
            sm.on_prevote(&vote);
        }

        assert_eq!(sm.step, RoundStep::Precommit);
        // locked_value should not change on nil
        assert_eq!(sm.locked_value, None);
    }

    #[test]
    fn test_precommit_quorum_commits() {
        let (keys, ids) = make_validators(4);
        let hash = BlockHash([0xDD; 32]);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Precommit;

        let mut committed = false;
        for key in &keys[0..3] {
            let vote =
                make_signed_vote(VoteType::Precommit, Height(0), Round(0), Some(hash), key);
            let msgs = sm.on_precommit(&vote);
            for msg in &msgs {
                if let ConsensusMessage::CommitBlock {
                    height,
                    block_hash,
                } = msg
                {
                    assert_eq!(*height, Height(0));
                    assert_eq!(*block_hash, hash);
                    committed = true;
                }
            }
        }

        assert!(committed, "should have committed the block");
        assert_eq!(sm.step, RoundStep::Commit);
    }

    #[test]
    fn test_precommit_no_quorum_no_commit() {
        let (keys, ids) = make_validators(4);
        let hash = BlockHash([0xEE; 32]);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Precommit;

        // Only 2 of 4 precommits — not enough
        for key in &keys[0..2] {
            let vote =
                make_signed_vote(VoteType::Precommit, Height(0), Round(0), Some(hash), key);
            let msgs = sm.on_precommit(&vote);
            for msg in &msgs {
                if matches!(msg, ConsensusMessage::CommitBlock { .. }) {
                    panic!("should not commit with only 2 of 4");
                }
            }
        }
        assert_ne!(sm.step, RoundStep::Commit);
    }

    #[test]
    fn test_timeout_propose_moves_to_prevote() {
        let (_keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(1), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        assert_eq!(sm.step, RoundStep::Propose);

        let msgs = sm.on_timeout(TimeoutEvent {
            height: Height(1),
            round: Round(0),
            step: TimeoutStep::Propose,
        });

        assert_eq!(sm.step, RoundStep::Prevote);
        // Should schedule a prevote timeout
        assert!(msgs.iter().any(|m| matches!(
            m,
            ConsensusMessage::ScheduleTimeout(TimeoutEvent {
                step: TimeoutStep::Prevote,
                ..
            })
        )));
    }

    #[test]
    fn test_timeout_prevote_moves_to_precommit() {
        let (_keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(1), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Prevote;

        let msgs = sm.on_timeout(TimeoutEvent {
            height: Height(1),
            round: Round(0),
            step: TimeoutStep::Prevote,
        });

        assert_eq!(sm.step, RoundStep::Precommit);
        assert!(msgs.iter().any(|m| matches!(
            m,
            ConsensusMessage::CastVote(Vote {
                vote_type: VoteType::Precommit,
                block_hash: None,
                ..
            })
        )));
    }

    #[test]
    fn test_timeout_precommit_advances_round() {
        let (_keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(1), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Precommit;

        let msgs = sm.on_timeout(TimeoutEvent {
            height: Height(1),
            round: Round(0),
            step: TimeoutStep::Precommit,
        });

        assert_eq!(sm.round, Round(1));
        assert_eq!(sm.step, RoundStep::Propose);
        assert!(msgs.iter().any(|m| matches!(
            m,
            ConsensusMessage::ScheduleTimeout(TimeoutEvent {
                step: TimeoutStep::Propose,
                round: Round(1),
                ..
            })
        )));
    }

    #[test]
    fn test_advance_height_resets_state() {
        let (_keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());
        sm.locked_value = Some(BlockHash([0xFF; 32]));
        sm.locked_round = Some(Round(2));
        sm.valid_value = Some(BlockHash([0xFF; 32]));
        sm.valid_round = Some(Round(2));

        let msgs = sm.advance_height(Height(1));

        assert_eq!(sm.height, Height(1));
        assert_eq!(sm.round, Round(0));
        assert!(sm.locked_value.is_none());
        assert!(sm.locked_round.is_none());
        assert!(sm.valid_value.is_none());
        assert!(sm.valid_round.is_none());
        assert!(!msgs.is_empty());
    }

    #[test]
    fn test_full_happy_path() {
        // Simulate a complete consensus round: propose → prevote → precommit → commit
        let (keys, ids) = make_validators(4);
        let hash = BlockHash([0x42; 32]);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());
        sm.start_round(Round(0));

        // 1. Proposal from validator 0 (the proposer for h=0, r=0)
        let proposal = make_proposal(Height(0), Round(0), hash, &keys[0]);
        let msgs = sm.on_proposal(&proposal, None);
        assert_eq!(sm.step, RoundStep::Prevote);
        assert!(!msgs.is_empty());

        // 2. Prevotes from 3 validators
        for key in &keys[0..3] {
            let vote = make_signed_vote(VoteType::Prevote, Height(0), Round(0), Some(hash), key);
            sm.on_prevote(&vote);
        }
        assert_eq!(sm.step, RoundStep::Precommit);

        // 3. Precommits from 3 validators
        let mut committed = false;
        for key in &keys[0..3] {
            let vote =
                make_signed_vote(VoteType::Precommit, Height(0), Round(0), Some(hash), key);
            let msgs = sm.on_precommit(&vote);
            for msg in &msgs {
                if let ConsensusMessage::CommitBlock { block_hash, .. } = msg {
                    assert_eq!(*block_hash, hash);
                    committed = true;
                }
            }
        }
        assert!(committed);
        assert_eq!(sm.step, RoundStep::Commit);
    }

    #[test]
    fn test_stale_timeout_ignored() {
        let (_keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(1), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(1));

        // Timeout for old round should be ignored
        let msgs = sm.on_timeout(TimeoutEvent {
            height: Height(1),
            round: Round(0),
            step: TimeoutStep::Propose,
        });
        assert!(msgs.is_empty());
        assert_eq!(sm.round, Round(1));
    }

    #[test]
    fn test_locking_respects_prior_lock() {
        let (keys, ids) = make_validators(4);
        let hash_a = BlockHash([0xAA; 32]);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());

        // Lock on hash_a in round 0
        sm.locked_value = Some(hash_a);
        sm.locked_round = Some(Round(0));

        // New round, different proposal
        sm.start_round(Round(1));
        let hash_b = BlockHash([0xBB; 32]);
        // Proposer for h=0,r=1 is validator index 1
        let proposal = make_proposal(Height(0), Round(1), hash_b, &keys[1]);
        let msgs = sm.on_proposal(&proposal, None);

        // Should prevote nil because we're locked on hash_a and proposal has no valid_round
        assert!(!msgs.is_empty());
        match &msgs[0] {
            ConsensusMessage::CastVote(v) => {
                assert_eq!(v.vote_type, VoteType::Prevote);
                assert_eq!(v.block_hash, None, "should prevote nil when locked on different block");
            }
            _ => panic!("expected prevote"),
        }
    }

    #[test]
    fn test_split_vote_schedules_prevote_timeout() {
        let (keys, ids) = make_validators(4);
        let hash_a = BlockHash([0xAA; 32]);
        let hash_b = BlockHash([0xBB; 32]);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(0), TimeoutConfig::default());
        sm.start_round(Round(0));
        sm.step = RoundStep::Prevote;

        // 2 vote A, 1 vote B, 1 vote nil → 4 total = 2/3+any but no quorum for anything
        let v1 = make_signed_vote(VoteType::Prevote, Height(0), Round(0), Some(hash_a), &keys[0]);
        let v2 = make_signed_vote(VoteType::Prevote, Height(0), Round(0), Some(hash_a), &keys[1]);
        let v3 = make_signed_vote(VoteType::Prevote, Height(0), Round(0), Some(hash_b), &keys[2]);
        let v4 = make_signed_vote(VoteType::Prevote, Height(0), Round(0), None, &keys[3]);

        sm.on_prevote(&v1);
        sm.on_prevote(&v2);
        sm.on_prevote(&v3);
        let msgs = sm.on_prevote(&v4);

        // Should still be in Prevote (no quorum for any single value)
        // but should schedule a prevote timeout since 2/3+ have voted
        assert!(msgs.iter().any(|m| matches!(
            m,
            ConsensusMessage::ScheduleTimeout(TimeoutEvent {
                step: TimeoutStep::Prevote,
                ..
            })
        )));
    }

    #[test]
    fn test_on_proposal_with_block_caches_it() {
        use crate::block::{Block, BlockHeader, Transaction};

        let (keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());
        sm.start_round(Round(0));

        let proposer = ValidatorId(keys[0].verifying_key());
        let txs = vec![Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        }];
        let block = Block {
            header: BlockHeader {
                height: Height(0),
                timestamp: 1700000000,
                parent_hash: BlockHash::default(),
                proposer: proposer.clone(),
                state_root: [0u8; 32],
                tx_merkle_root: Block::compute_tx_merkle_root(&txs),
            },
            transactions: txs,
        };
        let hash = block.hash();
        let proposal = make_proposal(Height(0), Round(0), hash, &keys[0]);

        let msgs = sm.on_proposal(&proposal, Some(&block));
        assert!(!msgs.is_empty(), "should have emitted a prevote");
        assert_eq!(sm.round_state.proposal, Some(hash));

        // Block should be cached
        let cached = sm.get_committed_block(&hash);
        assert!(cached.is_some(), "block should be cached after proposal");
        assert_eq!(cached.unwrap().hash(), hash);
    }

    #[test]
    fn test_on_proposal_with_mismatched_block_rejected() {
        use crate::block::{Block, BlockHeader};

        let (keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());
        sm.start_round(Round(0));

        let proposer = ValidatorId(keys[0].verifying_key());
        let block = Block {
            header: BlockHeader {
                height: Height(0),
                timestamp: 1700000000,
                parent_hash: BlockHash::default(),
                proposer: proposer.clone(),
                state_root: [0u8; 32],
                tx_merkle_root: [0u8; 32],
            },
            transactions: vec![],
        };

        // Proposal claims a different block hash
        let wrong_hash = BlockHash([0xFF; 32]);
        let proposal = make_proposal(Height(0), Round(0), wrong_hash, &keys[0]);

        let msgs = sm.on_proposal(&proposal, Some(&block));
        assert!(msgs.is_empty(), "mismatched block hash should reject the proposal");
        assert!(sm.round_state.proposal.is_none());
    }

    #[test]
    fn test_advance_height_clears_proposed_blocks() {
        use crate::block::{Block, BlockHeader};

        let (keys, ids) = make_validators(4);
        let mut sm = BftStateMachine::new(Height(0), ids, Some(1), TimeoutConfig::default());

        let proposer = ValidatorId(keys[0].verifying_key());
        let block = Block {
            header: BlockHeader {
                height: Height(0),
                timestamp: 1700000000,
                parent_hash: BlockHash::default(),
                proposer,
                state_root: [0u8; 32],
                tx_merkle_root: [0u8; 32],
            },
            transactions: vec![],
        };
        let hash = block.hash();
        sm.proposed_blocks.insert(hash, block);
        assert!(sm.get_committed_block(&hash).is_some());

        sm.advance_height(Height(1));
        assert!(sm.proposed_blocks.is_empty(), "proposed_blocks should be cleared on height advance");
    }
}
