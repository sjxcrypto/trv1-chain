use ed25519_dalek::{Signer, SigningKey, Verifier};
use std::collections::HashMap;

use crate::types::{BlockHash, Height, Round, ValidatorId, Vote, VoteType};

impl Vote {
    /// Create and sign a vote.
    pub fn new(
        vote_type: VoteType,
        height: Height,
        round: Round,
        block_hash: Option<BlockHash>,
        signing_key: &SigningKey,
    ) -> Self {
        let validator = ValidatorId(signing_key.verifying_key());
        let sign_bytes = Self::sign_bytes(vote_type, height, round, block_hash.as_ref());
        let signature = signing_key.sign(&sign_bytes);
        Self {
            vote_type,
            height,
            round,
            block_hash,
            validator,
            signature,
        }
    }

    /// Canonical bytes to sign / verify.
    fn sign_bytes(
        vote_type: VoteType,
        height: Height,
        round: Round,
        block_hash: Option<&BlockHash>,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(match vote_type {
            VoteType::Prevote => 0x01,
            VoteType::Precommit => 0x02,
        });
        buf.extend_from_slice(&height.0.to_le_bytes());
        buf.extend_from_slice(&round.0.to_le_bytes());
        match block_hash {
            Some(h) => {
                buf.push(0x01);
                buf.extend_from_slice(&h.0);
            }
            None => {
                buf.push(0x00);
            }
        }
        buf
    }

    /// Verify the vote signature against the validator's public key.
    pub fn verify(&self) -> bool {
        let sign_bytes = Self::sign_bytes(
            self.vote_type,
            self.height,
            self.round,
            self.block_hash.as_ref(),
        );
        self.validator.0.verify(&sign_bytes, &self.signature).is_ok()
    }
}

/// Collects votes for a specific height/round/type and checks quorum.
#[derive(Debug, Clone)]
pub struct VoteSet {
    pub vote_type: VoteType,
    pub height: Height,
    pub round: Round,
    /// Total number of validators in the current set.
    pub total_validators: usize,
    /// Votes indexed by validator public key bytes.
    votes: HashMap<[u8; 32], Vote>,
}

impl VoteSet {
    pub fn new(
        vote_type: VoteType,
        height: Height,
        round: Round,
        total_validators: usize,
    ) -> Self {
        Self {
            vote_type,
            height,
            round,
            total_validators,
            votes: HashMap::new(),
        }
    }

    /// Add a vote. Returns true if the vote was newly added (not duplicate).
    /// Rejects votes with wrong height/round/type or invalid signatures.
    pub fn add_vote(&mut self, vote: Vote) -> bool {
        if vote.vote_type != self.vote_type
            || vote.height != self.height
            || vote.round != self.round
        {
            return false;
        }
        if !vote.verify() {
            return false;
        }
        let key = *vote.validator.as_bytes();
        if self.votes.contains_key(&key) {
            return false; // duplicate
        }
        self.votes.insert(key, vote);
        true
    }

    /// Check if there is a 2/3+ quorum for a specific block hash.
    pub fn has_quorum_for(&self, block_hash: &BlockHash) -> bool {
        let count = self
            .votes
            .values()
            .filter(|v| v.block_hash.as_ref() == Some(block_hash))
            .count();
        self.is_quorum(count)
    }

    /// Check if there is a 2/3+ quorum for nil.
    pub fn has_quorum_for_nil(&self) -> bool {
        let count = self
            .votes
            .values()
            .filter(|v| v.block_hash.is_none())
            .count();
        self.is_quorum(count)
    }

    /// Check if any block hash has 2/3+ quorum. Returns the hash if so.
    pub fn quorum_block(&self) -> Option<BlockHash> {
        let mut counts: HashMap<BlockHash, usize> = HashMap::new();
        for vote in self.votes.values() {
            if let Some(hash) = vote.block_hash {
                *counts.entry(hash).or_insert(0) += 1;
            }
        }
        for (hash, count) in counts {
            if self.is_quorum(count) {
                return Some(hash);
            }
        }
        None
    }

    /// Whether we have 2/3+ of total validators having voted (any value).
    pub fn has_two_thirds_any(&self) -> bool {
        self.is_quorum(self.votes.len())
    }

    pub fn count(&self) -> usize {
        self.votes.len()
    }

    /// Check if count constitutes a 2/3+ quorum of total_validators.
    fn is_quorum(&self, count: usize) -> bool {
        // 2/3+ means count * 3 > total * 2
        count * 3 > self.total_validators * 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    fn make_signing_keys(n: usize) -> Vec<SigningKey> {
        (0..n).map(|_| SigningKey::generate(&mut OsRng)).collect()
    }

    #[test]
    fn test_vote_sign_and_verify() {
        let key = SigningKey::generate(&mut OsRng);
        let vote = Vote::new(
            VoteType::Prevote,
            Height(1),
            Round(0),
            Some(BlockHash([0xAB; 32])),
            &key,
        );
        assert!(vote.verify(), "valid vote should verify");
    }

    #[test]
    fn test_nil_vote_sign_and_verify() {
        let key = SigningKey::generate(&mut OsRng);
        let vote = Vote::new(VoteType::Prevote, Height(5), Round(2), None, &key);
        assert!(vote.verify());
        assert!(vote.block_hash.is_none());
    }

    #[test]
    fn test_vote_tampered_fails() {
        let key = SigningKey::generate(&mut OsRng);
        let mut vote = Vote::new(
            VoteType::Prevote,
            Height(1),
            Round(0),
            Some(BlockHash([0xAB; 32])),
            &key,
        );
        // Tamper with the block hash after signing
        vote.block_hash = Some(BlockHash([0xCD; 32]));
        assert!(!vote.verify(), "tampered vote should fail verification");
    }

    #[test]
    fn test_voteset_quorum_4_validators() {
        let keys = make_signing_keys(4);
        let hash = BlockHash([0x11; 32]);
        let mut vs = VoteSet::new(VoteType::Prevote, Height(1), Round(0), 4);

        // 2 of 4 = not quorum (2*3=6, 4*2=8, 6 <= 8)
        for key in &keys[0..2] {
            let vote = Vote::new(VoteType::Prevote, Height(1), Round(0), Some(hash), key);
            assert!(vs.add_vote(vote));
        }
        assert!(!vs.has_quorum_for(&hash));

        // 3 of 4 = quorum (3*3=9 > 4*2=8)
        let vote = Vote::new(VoteType::Prevote, Height(1), Round(0), Some(hash), &keys[2]);
        assert!(vs.add_vote(vote));
        assert!(vs.has_quorum_for(&hash));
    }

    #[test]
    fn test_voteset_quorum_3_validators() {
        // With 3 validators, need 3 for quorum (2*3=6, 3*2=6, not >)
        let keys = make_signing_keys(3);
        let hash = BlockHash([0x22; 32]);
        let mut vs = VoteSet::new(VoteType::Precommit, Height(1), Round(0), 3);

        for key in &keys[0..2] {
            let vote = Vote::new(VoteType::Precommit, Height(1), Round(0), Some(hash), key);
            vs.add_vote(vote);
        }
        assert!(
            !vs.has_quorum_for(&hash),
            "2 of 3 should not be quorum (need strict >2/3)"
        );

        let vote = Vote::new(
            VoteType::Precommit,
            Height(1),
            Round(0),
            Some(hash),
            &keys[2],
        );
        vs.add_vote(vote);
        assert!(vs.has_quorum_for(&hash), "3 of 3 should be quorum");
    }

    #[test]
    fn test_voteset_rejects_duplicate() {
        let key = SigningKey::generate(&mut OsRng);
        let hash = BlockHash([0x33; 32]);
        let mut vs = VoteSet::new(VoteType::Prevote, Height(1), Round(0), 4);

        let vote1 = Vote::new(VoteType::Prevote, Height(1), Round(0), Some(hash), &key);
        let vote2 = Vote::new(VoteType::Prevote, Height(1), Round(0), Some(hash), &key);
        assert!(vs.add_vote(vote1));
        assert!(!vs.add_vote(vote2), "duplicate vote should be rejected");
        assert_eq!(vs.count(), 1);
    }

    #[test]
    fn test_voteset_rejects_wrong_round() {
        let key = SigningKey::generate(&mut OsRng);
        let hash = BlockHash([0x44; 32]);
        let mut vs = VoteSet::new(VoteType::Prevote, Height(1), Round(0), 4);

        let vote = Vote::new(VoteType::Prevote, Height(1), Round(1), Some(hash), &key);
        assert!(!vs.add_vote(vote), "wrong round should be rejected");
    }

    #[test]
    fn test_voteset_nil_quorum() {
        let keys = make_signing_keys(4);
        let mut vs = VoteSet::new(VoteType::Prevote, Height(1), Round(0), 4);

        for key in &keys[0..3] {
            let vote = Vote::new(VoteType::Prevote, Height(1), Round(0), None, key);
            vs.add_vote(vote);
        }
        assert!(vs.has_quorum_for_nil(), "3 of 4 nil should be quorum");
    }

    #[test]
    fn test_voteset_quorum_block() {
        let keys = make_signing_keys(4);
        let hash = BlockHash([0x55; 32]);
        let mut vs = VoteSet::new(VoteType::Precommit, Height(1), Round(0), 4);

        for key in &keys[0..3] {
            let vote = Vote::new(
                VoteType::Precommit,
                Height(1),
                Round(0),
                Some(hash),
                key,
            );
            vs.add_vote(vote);
        }
        assert_eq!(vs.quorum_block(), Some(hash));
    }

    #[test]
    fn test_voteset_split_no_quorum() {
        let keys = make_signing_keys(4);
        let hash_a = BlockHash([0xAA; 32]);
        let hash_b = BlockHash([0xBB; 32]);
        let mut vs = VoteSet::new(VoteType::Prevote, Height(1), Round(0), 4);

        // 2 vote A, 2 vote B → no quorum for either
        for key in &keys[0..2] {
            let vote = Vote::new(VoteType::Prevote, Height(1), Round(0), Some(hash_a), key);
            vs.add_vote(vote);
        }
        for key in &keys[2..4] {
            let vote = Vote::new(VoteType::Prevote, Height(1), Round(0), Some(hash_b), key);
            vs.add_vote(vote);
        }
        assert!(!vs.has_quorum_for(&hash_a));
        assert!(!vs.has_quorum_for(&hash_b));
        assert_eq!(vs.quorum_block(), None);
        // But we have 2/3+ of total having voted
        assert!(vs.has_two_thirds_any());
    }
}
