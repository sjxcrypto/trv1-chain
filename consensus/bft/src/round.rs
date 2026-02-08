use serde::{Deserialize, Serialize};

use crate::types::{BlockHash, Round, TimeoutConfig, TimeoutStep};
use crate::vote::VoteSet;

/// The current step/phase within a consensus round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundStep {
    /// Waiting for a proposal from the designated proposer.
    NewRound,
    /// Proposer broadcasts block; validators wait to receive it.
    Propose,
    /// Validators exchange prevotes.
    Prevote,
    /// Validators exchange precommits.
    Precommit,
    /// Block committed, advancing to next height.
    Commit,
}

/// Tracks the state of a single consensus round.
#[derive(Debug, Clone)]
pub struct RoundState {
    pub round: Round,
    pub step: RoundStep,
    pub proposal: Option<BlockHash>,
    pub prevotes: VoteSet,
    pub precommits: VoteSet,
}

impl RoundState {
    pub fn new(
        round: Round,
        height: crate::types::Height,
        total_validators: usize,
    ) -> Self {
        Self {
            round,
            step: RoundStep::NewRound,
            proposal: None,
            prevotes: VoteSet::new(
                crate::types::VoteType::Prevote,
                height,
                round,
                total_validators,
            ),
            precommits: VoteSet::new(
                crate::types::VoteType::Precommit,
                height,
                round,
                total_validators,
            ),
        }
    }

    /// Compute the propose timeout for this round.
    pub fn propose_timeout(&self, config: &TimeoutConfig) -> u64 {
        config.timeout_for(TimeoutStep::Propose, self.round)
    }

    /// Compute the prevote timeout for this round.
    pub fn prevote_timeout(&self, config: &TimeoutConfig) -> u64 {
        config.timeout_for(TimeoutStep::Prevote, self.round)
    }

    /// Compute the precommit timeout for this round.
    pub fn precommit_timeout(&self, config: &TimeoutConfig) -> u64 {
        config.timeout_for(TimeoutStep::Precommit, self.round)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Height;

    #[test]
    fn test_round_state_initial() {
        let rs = RoundState::new(Round(0), Height(1), 4);
        assert_eq!(rs.step, RoundStep::NewRound);
        assert!(rs.proposal.is_none());
        assert_eq!(rs.prevotes.count(), 0);
        assert_eq!(rs.precommits.count(), 0);
    }

    #[test]
    fn test_timeouts_increase_with_round() {
        let config = TimeoutConfig::default();
        let r0 = RoundState::new(Round(0), Height(1), 4);
        let r1 = RoundState::new(Round(1), Height(1), 4);
        let r5 = RoundState::new(Round(5), Height(1), 4);

        assert!(r1.propose_timeout(&config) > r0.propose_timeout(&config));
        assert!(r5.propose_timeout(&config) > r1.propose_timeout(&config));
    }

    #[test]
    fn test_timeout_values() {
        let config = TimeoutConfig {
            propose_ms: 3000,
            prevote_ms: 1000,
            precommit_ms: 1000,
            increment_ms: 500,
        };
        let r0 = RoundState::new(Round(0), Height(1), 4);
        assert_eq!(r0.propose_timeout(&config), 3000);
        assert_eq!(r0.prevote_timeout(&config), 1000);
        assert_eq!(r0.precommit_timeout(&config), 1000);

        let r2 = RoundState::new(Round(2), Height(1), 4);
        assert_eq!(r2.propose_timeout(&config), 4000); // 3000 + 2*500
        assert_eq!(r2.prevote_timeout(&config), 2000); // 1000 + 2*500
    }
}
