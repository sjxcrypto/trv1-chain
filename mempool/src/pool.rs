use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use tracing::debug;
use trv1_bft::block::Transaction;

use crate::types::{MempoolConfig, MempoolError, PendingTransaction};
use crate::validation::{validate_transaction, verify_signature};

/// Transaction pool storing pending transactions awaiting inclusion in a block.
#[derive(Debug)]
pub struct TransactionPool {
    config: MempoolConfig,
    /// Transactions grouped by sender pubkey.
    by_sender: HashMap<[u8; 32], Vec<PendingTransaction>>,
    /// Set of known tx hashes for dedup.
    known_hashes: HashSet<[u8; 32]>,
    /// Total number of pending transactions.
    total_count: usize,
}

impl TransactionPool {
    /// Create a new transaction pool with the given configuration.
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            config,
            by_sender: HashMap::new(),
            known_hashes: HashSet::new(),
            total_count: 0,
        }
    }

    /// Add a transaction to the pool after validation.
    pub fn add_transaction(&mut self, tx: Transaction) -> Result<(), MempoolError> {
        // Basic structural validation
        validate_transaction(&tx)?;

        // Cryptographic signature verification
        verify_signature(&tx)?;

        // Check pool capacity
        if self.total_count >= self.config.max_size {
            return Err(MempoolError::PoolFull);
        }

        // Compute hash for dedup
        let tx_hash = compute_tx_hash(&tx);
        if self.known_hashes.contains(&tx_hash) {
            return Err(MempoolError::DuplicateTransaction);
        }

        // Check per-account limit
        let sender_txs = self.by_sender.entry(tx.from).or_default();
        if sender_txs.len() >= self.config.max_tx_per_account {
            return Err(MempoolError::PoolFull);
        }

        // Use a simple incrementing timestamp (in production, use real time)
        let added_at = self.total_count as u64;
        let pending = PendingTransaction::new(tx, added_at);

        sender_txs.push(pending);
        self.known_hashes.insert(tx_hash);
        self.total_count += 1;

        debug!(tx_hash = ?tx_hash, total = self.total_count, "transaction added to mempool");

        Ok(())
    }

    /// Remove transactions that have been committed in a block.
    pub fn remove_committed(&mut self, tx_hashes: &[[u8; 32]]) {
        let remove_set: HashSet<[u8; 32]> = tx_hashes.iter().copied().collect();

        for sender_txs in self.by_sender.values_mut() {
            let before = sender_txs.len();
            sender_txs.retain(|ptx| {
                let hash = compute_tx_hash(&ptx.tx);
                !remove_set.contains(&hash)
            });
            self.total_count -= before - sender_txs.len();
        }

        // Clean up empty sender entries
        self.by_sender.retain(|_, txs| !txs.is_empty());

        // Remove from known hashes
        for hash in tx_hashes {
            self.known_hashes.remove(hash);
        }
    }

    /// Get pending transactions ordered by fee priority (highest first), up to `max_count`.
    pub fn get_pending_ordered(&self, max_count: usize) -> Vec<Transaction> {
        let mut all_pending: Vec<&PendingTransaction> = self
            .by_sender
            .values()
            .flat_map(|txs| txs.iter())
            .collect();

        // Sort by fee_priority descending, then by added_at ascending for tie-breaking
        all_pending.sort_by(|a, b| {
            b.fee_priority
                .cmp(&a.fee_priority)
                .then(a.added_at.cmp(&b.added_at))
        });

        all_pending
            .into_iter()
            .take(max_count)
            .map(|ptx| ptx.tx.clone())
            .collect()
    }

    /// Total number of pending transactions.
    pub fn pending_count(&self) -> usize {
        self.total_count
    }

    /// Check if a transaction hash is already in the pool.
    pub fn contains(&self, tx_hash: &[u8; 32]) -> bool {
        self.known_hashes.contains(tx_hash)
    }

    /// Remove all pending transactions.
    pub fn clear(&mut self) {
        self.by_sender.clear();
        self.known_hashes.clear();
        self.total_count = 0;
    }
}

/// Compute the SHA-256 hash of a serialized transaction.
pub fn compute_tx_hash(tx: &Transaction) -> [u8; 32] {
    let encoded = serde_json::to_vec(tx).expect("transaction serialization should never fail");
    let digest = Sha256::digest(&encoded);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_tx(from: [u8; 32], to: [u8; 32], amount: u64, nonce: u64) -> Transaction {
        Transaction {
            from,
            to,
            amount,
            nonce,
            signature: vec![0u8; 64], // dummy sig for non-pool tests (hashing, structural rejection)
            data: vec![],
        }
    }

    fn make_real_signed_tx(
        signing_key: &SigningKey,
        to: [u8; 32],
        amount: u64,
        nonce: u64,
    ) -> Transaction {
        let from = signing_key.verifying_key().to_bytes();
        let data = vec![];

        let mut tx = Transaction {
            from,
            to,
            amount,
            nonce,
            signature: vec![],
            data,
        };

        let message = crate::validation::build_signing_message(&tx);
        let sig = signing_key.sign(&message);
        tx.signature = sig.to_bytes().to_vec();
        tx
    }

    fn default_pool() -> TransactionPool {
        TransactionPool::new(MempoolConfig::default())
    }

    fn small_pool() -> TransactionPool {
        TransactionPool::new(MempoolConfig {
            max_size: 3,
            max_tx_per_account: 2,
        })
    }

    #[test]
    fn test_add_and_count() {
        let mut pool = default_pool();
        let sk = SigningKey::generate(&mut OsRng);
        let tx = make_real_signed_tx(&sk, [2u8; 32], 100, 0);
        assert!(pool.add_transaction(tx).is_ok());
        assert_eq!(pool.pending_count(), 1);
    }

    #[test]
    fn test_dedup_detection() {
        let mut pool = default_pool();
        let sk = SigningKey::generate(&mut OsRng);
        let tx = make_real_signed_tx(&sk, [2u8; 32], 100, 0);
        assert!(pool.add_transaction(tx.clone()).is_ok());
        let err = pool.add_transaction(tx).unwrap_err();
        assert_eq!(err, MempoolError::DuplicateTransaction);
        assert_eq!(pool.pending_count(), 1);
    }

    #[test]
    fn test_pool_full() {
        let mut pool = small_pool();
        let sk1 = SigningKey::generate(&mut OsRng);
        let sk2 = SigningKey::generate(&mut OsRng);
        let sk3 = SigningKey::generate(&mut OsRng);
        let sk4 = SigningKey::generate(&mut OsRng);
        pool.add_transaction(make_real_signed_tx(&sk1, [2u8; 32], 100, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk2, [3u8; 32], 200, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk3, [4u8; 32], 300, 0)).unwrap();

        let err = pool
            .add_transaction(make_real_signed_tx(&sk4, [5u8; 32], 400, 0))
            .unwrap_err();
        assert_eq!(err, MempoolError::PoolFull);
    }

    #[test]
    fn test_per_account_limit() {
        let mut pool = small_pool();
        let sk = SigningKey::generate(&mut OsRng);
        pool.add_transaction(make_real_signed_tx(&sk, [2u8; 32], 100, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk, [2u8; 32], 200, 1)).unwrap();

        // Third tx from same sender exceeds per-account limit of 2
        let err = pool
            .add_transaction(make_real_signed_tx(&sk, [2u8; 32], 300, 2))
            .unwrap_err();
        assert_eq!(err, MempoolError::PoolFull);
    }

    #[test]
    fn test_ordering_by_priority() {
        let mut pool = default_pool();
        let sk1 = SigningKey::generate(&mut OsRng);
        let sk2 = SigningKey::generate(&mut OsRng);
        let sk3 = SigningKey::generate(&mut OsRng);
        pool.add_transaction(make_real_signed_tx(&sk1, [2u8; 32], 10, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk2, [3u8; 32], 500, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk3, [4u8; 32], 100, 0)).unwrap();

        let ordered = pool.get_pending_ordered(10);
        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0].amount, 500); // highest priority first
        assert_eq!(ordered[1].amount, 100);
        assert_eq!(ordered[2].amount, 10);
    }

    #[test]
    fn test_ordering_max_count() {
        let mut pool = default_pool();
        let sk1 = SigningKey::generate(&mut OsRng);
        let sk2 = SigningKey::generate(&mut OsRng);
        let sk3 = SigningKey::generate(&mut OsRng);
        pool.add_transaction(make_real_signed_tx(&sk1, [2u8; 32], 10, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk2, [3u8; 32], 500, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk3, [4u8; 32], 100, 0)).unwrap();

        let ordered = pool.get_pending_ordered(2);
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].amount, 500);
        assert_eq!(ordered[1].amount, 100);
    }

    #[test]
    fn test_contains() {
        let mut pool = default_pool();
        let sk = SigningKey::generate(&mut OsRng);
        let tx = make_real_signed_tx(&sk, [2u8; 32], 100, 0);
        let hash = compute_tx_hash(&tx);
        assert!(!pool.contains(&hash));
        pool.add_transaction(tx).unwrap();
        assert!(pool.contains(&hash));
    }

    #[test]
    fn test_remove_committed() {
        let mut pool = default_pool();
        let sk1 = SigningKey::generate(&mut OsRng);
        let sk2 = SigningKey::generate(&mut OsRng);
        let tx1 = make_real_signed_tx(&sk1, [2u8; 32], 100, 0);
        let tx2 = make_real_signed_tx(&sk2, [3u8; 32], 200, 0);
        let hash1 = compute_tx_hash(&tx1);
        let hash2 = compute_tx_hash(&tx2);

        pool.add_transaction(tx1).unwrap();
        pool.add_transaction(tx2).unwrap();
        assert_eq!(pool.pending_count(), 2);

        pool.remove_committed(&[hash1]);
        assert_eq!(pool.pending_count(), 1);
        assert!(!pool.contains(&hash1));
        assert!(pool.contains(&hash2));
    }

    #[test]
    fn test_clear() {
        let mut pool = default_pool();
        let sk1 = SigningKey::generate(&mut OsRng);
        let sk2 = SigningKey::generate(&mut OsRng);
        pool.add_transaction(make_real_signed_tx(&sk1, [2u8; 32], 100, 0)).unwrap();
        pool.add_transaction(make_real_signed_tx(&sk2, [3u8; 32], 200, 0)).unwrap();
        assert_eq!(pool.pending_count(), 2);

        pool.clear();
        assert_eq!(pool.pending_count(), 0);
    }

    #[test]
    fn test_reject_zero_sender() {
        let mut pool = default_pool();
        let tx = make_tx([0u8; 32], [2u8; 32], 100, 0);
        let err = pool.add_transaction(tx).unwrap_err();
        assert!(matches!(err, MempoolError::InvalidTransaction(_)));
    }

    #[test]
    fn test_reject_empty_signature() {
        let mut pool = default_pool();
        let mut tx = make_tx([1u8; 32], [2u8; 32], 100, 0);
        tx.signature = vec![]; // empty
        let err = pool.add_transaction(tx).unwrap_err();
        assert!(matches!(err, MempoolError::InvalidTransaction(_)));
    }

    #[test]
    fn test_reject_invalid_signature() {
        let mut pool = default_pool();
        let sk = SigningKey::generate(&mut OsRng);
        let mut tx = make_real_signed_tx(&sk, [2u8; 32], 100, 0);
        tx.amount = 999; // tamper with transaction after signing
        let err = pool.add_transaction(tx).unwrap_err();
        assert_eq!(err, MempoolError::InvalidSignature);
    }

    #[test]
    fn test_compute_tx_hash_deterministic() {
        let tx = make_tx([1u8; 32], [2u8; 32], 100, 0);
        let h1 = compute_tx_hash(&tx);
        let h2 = compute_tx_hash(&tx);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_tx_hash_different_txs() {
        let tx1 = make_tx([1u8; 32], [2u8; 32], 100, 0);
        let tx2 = make_tx([1u8; 32], [2u8; 32], 200, 0);
        assert_ne!(compute_tx_hash(&tx1), compute_tx_hash(&tx2));
    }

    #[test]
    fn test_real_signed_tx_in_pool() {
        let mut pool = default_pool();
        let sk = SigningKey::generate(&mut OsRng);
        let tx = make_real_signed_tx(&sk, [2u8; 32], 100, 0);
        assert!(pool.add_transaction(tx).is_ok());
        assert_eq!(pool.pending_count(), 1);
    }

    #[test]
    fn test_remove_nonexistent_is_noop() {
        let mut pool = default_pool();
        let sk = SigningKey::generate(&mut OsRng);
        pool.add_transaction(make_real_signed_tx(&sk, [2u8; 32], 100, 0)).unwrap();
        let fake_hash = [0xffu8; 32];
        pool.remove_committed(&[fake_hash]);
        assert_eq!(pool.pending_count(), 1);
    }
}
