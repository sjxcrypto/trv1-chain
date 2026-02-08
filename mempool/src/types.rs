use serde::{Deserialize, Serialize};
use thiserror::Error;
use trv1_bft::block::Transaction;

/// Errors that can occur in the mempool.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MempoolError {
    #[error("duplicate transaction")]
    DuplicateTransaction,

    #[error("mempool is full")]
    PoolFull,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("nonce too low: expected {expected}, got {got}")]
    NonceTooLow { expected: u64, got: u64 },

    #[error("insufficient balance")]
    InsufficientBalance,

    #[error("invalid transaction: {0}")]
    InvalidTransaction(String),
}

/// Configuration for the transaction pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolConfig {
    /// Maximum number of transactions in the pool.
    pub max_size: usize,
    /// Maximum number of pending transactions per account.
    pub max_tx_per_account: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10_000,
            max_tx_per_account: 100,
        }
    }
}

/// A transaction waiting in the mempool with metadata.
#[derive(Debug, Clone)]
pub struct PendingTransaction {
    pub tx: Transaction,
    pub added_at: u64,
    pub fee_priority: u64,
}

impl PendingTransaction {
    /// Wrap a transaction with mempool metadata.
    pub fn new(tx: Transaction, added_at: u64) -> Self {
        let fee_priority = tx.amount;
        Self {
            tx,
            added_at,
            fee_priority,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MempoolConfig::default();
        assert_eq!(config.max_size, 10_000);
        assert_eq!(config.max_tx_per_account, 100);
    }

    #[test]
    fn test_pending_transaction_fee_priority() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 500,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        };
        let ptx = PendingTransaction::new(tx, 1000);
        assert_eq!(ptx.fee_priority, 500);
        assert_eq!(ptx.added_at, 1000);
    }

    #[test]
    fn test_mempool_error_display() {
        let err = MempoolError::NonceTooLow {
            expected: 5,
            got: 3,
        };
        assert!(err.to_string().contains("expected 5"));

        let err = MempoolError::InvalidTransaction("bad data".into());
        assert!(err.to_string().contains("bad data"));
    }
}
