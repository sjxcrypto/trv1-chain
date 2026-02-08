use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Wrapper around an ed25519 public key identifying a validator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ValidatorId(pub VerifyingKey);

impl ValidatorId {
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

/// SHA-256 block hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct BlockHash(pub [u8; 32]);

impl BlockHash {
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }
}

impl std::fmt::Display for BlockHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

/// Block height (0-indexed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Height(pub u64);

/// Consensus round within a height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Round(pub u32);

/// A vote cast by a validator (prevote or precommit).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub vote_type: VoteType,
    pub height: Height,
    pub round: Round,
    /// None means a nil vote (no block proposed or timeout).
    pub block_hash: Option<BlockHash>,
    pub validator: ValidatorId,
    pub signature: Signature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteType {
    Prevote,
    Precommit,
}

/// A block proposal from the round's designated proposer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub height: Height,
    pub round: Round,
    pub block_hash: BlockHash,
    pub proposer: ValidatorId,
    pub signature: Signature,
    /// If set, the proposal is for a previously locked block.
    pub valid_round: Option<Round>,
}

/// Messages produced and consumed by the BFT state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    ProposeBlock(Proposal),
    CastVote(Vote),
    CommitBlock {
        height: Height,
        block_hash: BlockHash,
    },
    ScheduleTimeout(TimeoutEvent),
}

/// Timeout events fed back into the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutEvent {
    pub height: Height,
    pub round: Round,
    pub step: TimeoutStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeoutStep {
    Propose,
    Prevote,
    Precommit,
}

/// Timeout durations for each BFT phase.
#[derive(Debug, Clone, Copy)]
pub struct TimeoutConfig {
    pub propose_ms: u64,
    pub prevote_ms: u64,
    pub precommit_ms: u64,
    /// Additional ms per round increment (linear backoff).
    pub increment_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            propose_ms: 3000,
            prevote_ms: 1000,
            precommit_ms: 1000,
            increment_ms: 500,
        }
    }
}

impl TimeoutConfig {
    /// Compute the timeout for a given step and round, applying linear backoff.
    pub fn timeout_for(&self, step: TimeoutStep, round: Round) -> u64 {
        let base = match step {
            TimeoutStep::Propose => self.propose_ms,
            TimeoutStep::Prevote => self.prevote_ms,
            TimeoutStep::Precommit => self.precommit_ms,
        };
        base + self.increment_ms * round.0 as u64
    }
}
