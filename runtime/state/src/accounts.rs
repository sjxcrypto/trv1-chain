use serde::{Deserialize, Serialize};

use crate::types::StateError;

/// Per-account state: balance and nonce.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountState {
    pub balance: u64,
    pub nonce: u64,
}

impl AccountState {
    /// Create a new account with the given balance and nonce 0.
    pub fn new(balance: u64) -> Self {
        Self { balance, nonce: 0 }
    }

    /// Subtract `amount` from balance. Fails if insufficient.
    pub fn debit(&mut self, amount: u64) -> Result<(), StateError> {
        if self.balance < amount {
            return Err(StateError::InsufficientBalance {
                needed: amount,
                available: self.balance,
            });
        }
        self.balance -= amount;
        Ok(())
    }

    /// Add `amount` to balance. Fails on overflow.
    pub fn credit(&mut self, amount: u64) -> Result<(), StateError> {
        self.balance = self.balance.checked_add(amount).ok_or(StateError::Overflow)?;
        Ok(())
    }

    /// Increment the nonce by 1.
    pub fn increment_nonce(&mut self) {
        self.nonce += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_account() {
        let acct = AccountState::new(1000);
        assert_eq!(acct.balance, 1000);
        assert_eq!(acct.nonce, 0);
    }

    #[test]
    fn test_default_account() {
        let acct = AccountState::default();
        assert_eq!(acct.balance, 0);
        assert_eq!(acct.nonce, 0);
    }

    #[test]
    fn test_debit_success() {
        let mut acct = AccountState::new(500);
        assert!(acct.debit(200).is_ok());
        assert_eq!(acct.balance, 300);
    }

    #[test]
    fn test_debit_exact_balance() {
        let mut acct = AccountState::new(500);
        assert!(acct.debit(500).is_ok());
        assert_eq!(acct.balance, 0);
    }

    #[test]
    fn test_debit_insufficient() {
        let mut acct = AccountState::new(100);
        let err = acct.debit(200).unwrap_err();
        assert_eq!(
            err,
            StateError::InsufficientBalance {
                needed: 200,
                available: 100,
            }
        );
        // Balance should be unchanged on failure
        assert_eq!(acct.balance, 100);
    }

    #[test]
    fn test_credit_success() {
        let mut acct = AccountState::new(100);
        assert!(acct.credit(50).is_ok());
        assert_eq!(acct.balance, 150);
    }

    #[test]
    fn test_credit_overflow() {
        let mut acct = AccountState::new(u64::MAX);
        let err = acct.credit(1).unwrap_err();
        assert_eq!(err, StateError::Overflow);
        assert_eq!(acct.balance, u64::MAX);
    }

    #[test]
    fn test_increment_nonce() {
        let mut acct = AccountState::new(0);
        assert_eq!(acct.nonce, 0);
        acct.increment_nonce();
        assert_eq!(acct.nonce, 1);
        acct.increment_nonce();
        assert_eq!(acct.nonce, 2);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let acct = AccountState::new(42);
        let json = serde_json::to_string(&acct).unwrap();
        let deserialized: AccountState = serde_json::from_str(&json).unwrap();
        assert_eq!(acct, deserialized);
    }
}
