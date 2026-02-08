use std::collections::HashMap;

use crate::types::*;

/// Pool for accumulating and managing evidence of validator misbehavior.
#[derive(Debug, Clone, Default)]
pub struct EvidencePool {
    /// Evidence records keyed by their hash for deduplication.
    records: HashMap<[u8; 32], EvidenceRecord>,
}

impl EvidencePool {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Submit a new piece of evidence. Returns the evidence hash on success.
    pub fn submit_evidence(&mut self, evidence: EvidenceRecord) -> SlashingResult<[u8; 32]> {
        // Basic validation.
        if evidence.data.is_empty() {
            return Err(SlashingError::InvalidEvidence(
                "evidence data must not be empty".into(),
            ));
        }

        let hash = evidence.hash();

        // Dedup check.
        if self.records.contains_key(&hash) {
            return Err(SlashingError::DuplicateEvidence);
        }

        tracing::info!(
            offender = ?evidence.offender,
            offense = %evidence.offense,
            height = evidence.height,
            "evidence submitted"
        );

        self.records.insert(hash, evidence);
        Ok(hash)
    }

    /// Get all pending (unprocessed) evidence records.
    pub fn get_pending_evidence(&self) -> Vec<&EvidenceRecord> {
        self.records
            .values()
            .filter(|e| !e.processed)
            .collect()
    }

    /// Mark an evidence record as processed.
    pub fn mark_processed(&mut self, hash: &[u8; 32]) -> bool {
        if let Some(record) = self.records.get_mut(hash) {
            record.processed = true;
            true
        } else {
            false
        }
    }

    /// Get an evidence record by hash.
    pub fn get(&self, hash: &[u8; 32]) -> Option<&EvidenceRecord> {
        self.records.get(hash)
    }

    /// Total evidence count.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pubkey(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    fn make_evidence(n: u8, offense: SlashingOffense, height: u64) -> EvidenceRecord {
        EvidenceRecord {
            offense,
            offender: pubkey(n),
            height,
            data: vec![1, 2, 3, n],
            processed: false,
        }
    }

    #[test]
    fn submit_and_retrieve() {
        let mut pool = EvidencePool::new();
        let evidence = make_evidence(1, SlashingOffense::DoubleSign, 100);
        let hash = pool.submit_evidence(evidence.clone()).unwrap();

        let pending = pool.get_pending_evidence();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].offender, pubkey(1));

        let record = pool.get(&hash).unwrap();
        assert!(!record.processed);
    }

    #[test]
    fn duplicate_evidence_rejected() {
        let mut pool = EvidencePool::new();
        let evidence = make_evidence(1, SlashingOffense::DoubleSign, 100);
        pool.submit_evidence(evidence.clone()).unwrap();

        let result = pool.submit_evidence(evidence);
        assert_eq!(result, Err(SlashingError::DuplicateEvidence));
    }

    #[test]
    fn empty_data_rejected() {
        let mut pool = EvidencePool::new();
        let evidence = EvidenceRecord {
            offense: SlashingOffense::Downtime,
            offender: pubkey(1),
            height: 50,
            data: vec![], // empty
            processed: false,
        };
        let result = pool.submit_evidence(evidence);
        assert!(matches!(result, Err(SlashingError::InvalidEvidence(_))));
    }

    #[test]
    fn mark_processed() {
        let mut pool = EvidencePool::new();
        let evidence = make_evidence(1, SlashingOffense::Downtime, 50);
        let hash = pool.submit_evidence(evidence).unwrap();

        assert_eq!(pool.get_pending_evidence().len(), 1);

        pool.mark_processed(&hash);
        assert_eq!(pool.get_pending_evidence().len(), 0);
        assert!(pool.get(&hash).unwrap().processed);
    }

    #[test]
    fn multiple_evidence_for_different_validators() {
        let mut pool = EvidencePool::new();
        pool.submit_evidence(make_evidence(1, SlashingOffense::DoubleSign, 100)).unwrap();
        pool.submit_evidence(make_evidence(2, SlashingOffense::Downtime, 100)).unwrap();
        pool.submit_evidence(make_evidence(3, SlashingOffense::InvalidBlock, 100)).unwrap();

        assert_eq!(pool.len(), 3);
        assert_eq!(pool.get_pending_evidence().len(), 3);
    }
}
