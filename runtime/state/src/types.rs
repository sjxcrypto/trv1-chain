use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during state transitions.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StateError {
    #[error("insufficient balance: need {needed}, have {available}")]
    InsufficientBalance { needed: u64, available: u64 },

    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("account not found")]
    AccountNotFound,

    #[error("arithmetic overflow")]
    Overflow,
}

/// Receipt produced after executing a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    pub tx_hash: [u8; 32],
    pub success: bool,
    pub fee_paid: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_error_display() {
        let err = StateError::InsufficientBalance {
            needed: 100,
            available: 50,
        };
        assert!(err.to_string().contains("insufficient balance"));

        let err = StateError::InvalidNonce {
            expected: 1,
            got: 0,
        };
        assert!(err.to_string().contains("expected 1"));
    }

    #[test]
    fn test_receipt_serialization() {
        let receipt = TransactionReceipt {
            tx_hash: [0xab; 32],
            success: true,
            fee_paid: 0,
            error: None,
        };
        let json = serde_json::to_string(&receipt).unwrap();
        let deserialized: TransactionReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.success, true);
        assert_eq!(deserialized.tx_hash, [0xab; 32]);
    }
}
