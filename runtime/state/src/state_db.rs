use std::collections::HashMap;

use sha2::{Digest, Sha256};
use tracing::warn;

use crate::accounts::AccountState;
use crate::types::{StateError, TransactionReceipt};

/// In-memory account state database.
#[derive(Debug, Clone)]
pub struct StateDB {
    accounts: HashMap<[u8; 32], AccountState>,
}

impl StateDB {
    /// Create a new, empty state database.
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    /// Look up an account by public key.
    pub fn get_account(&self, pubkey: &[u8; 32]) -> Option<&AccountState> {
        self.accounts.get(pubkey)
    }

    /// Look up an account mutably by public key.
    pub fn get_account_mut(&mut self, pubkey: &[u8; 32]) -> Option<&mut AccountState> {
        self.accounts.get_mut(pubkey)
    }

    /// Get an existing account or create a zero-balance one.
    pub fn get_or_create_account(&mut self, pubkey: &[u8; 32]) -> &mut AccountState {
        self.accounts
            .entry(*pubkey)
            .or_insert_with(AccountState::default)
    }

    /// Insert or overwrite an account.
    pub fn set_account(&mut self, pubkey: [u8; 32], state: AccountState) {
        self.accounts.insert(pubkey, state);
    }

    /// Apply a single transfer: validate nonce, debit sender, credit receiver,
    /// then increment sender nonce.
    pub fn apply_transfer(
        &mut self,
        from: &[u8; 32],
        to: &[u8; 32],
        amount: u64,
        expected_nonce: u64,
    ) -> Result<(), StateError> {
        // Check sender exists
        let sender = self
            .accounts
            .get(from)
            .ok_or(StateError::AccountNotFound)?;

        // Validate nonce
        if sender.nonce != expected_nonce {
            return Err(StateError::InvalidNonce {
                expected: sender.nonce,
                got: expected_nonce,
            });
        }

        // Check balance (don't mutate yet in case credit fails for self-transfers)
        if sender.balance < amount {
            return Err(StateError::InsufficientBalance {
                needed: amount,
                available: sender.balance,
            });
        }

        // Handle self-transfer: just increment nonce
        if from == to {
            let sender = self.accounts.get_mut(from).unwrap();
            sender.increment_nonce();
            return Ok(());
        }

        // Debit sender
        let sender = self.accounts.get_mut(from).unwrap();
        sender.debit(amount)?;
        sender.increment_nonce();

        // Credit receiver (create if needed)
        let receiver = self.get_or_create_account(to);
        receiver.credit(amount)?;

        Ok(())
    }

    /// Apply all transactions in a block, returning a receipt for each.
    /// Failed transactions produce a receipt with `success=false` but do not
    /// revert other successful transactions.
    pub fn apply_block(
        &mut self,
        transactions: &[trv1_bft::block::Transaction],
    ) -> Vec<TransactionReceipt> {
        let mut receipts = Vec::with_capacity(transactions.len());

        for tx in transactions {
            let tx_hash = Self::hash_transaction(tx);

            match self.apply_transfer(&tx.from, &tx.to, tx.amount, tx.nonce) {
                Ok(()) => {
                    receipts.push(TransactionReceipt {
                        tx_hash,
                        success: true,
                        fee_paid: 0,
                        error: None,
                    });
                }
                Err(e) => {
                    warn!(
                        tx_hash = ?tx_hash,
                        error = %e,
                        "transaction failed during block execution"
                    );
                    receipts.push(TransactionReceipt {
                        tx_hash,
                        success: false,
                        fee_paid: 0,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        receipts
    }

    /// Compute a deterministic state root by hashing sorted serialized accounts.
    pub fn compute_state_root(&self) -> [u8; 32] {
        let mut entries: Vec<_> = self.accounts.iter().collect();
        entries.sort_by_key(|(k, _)| *k);

        let mut hasher = Sha256::new();
        for (pubkey, state) in &entries {
            hasher.update(pubkey);
            let serialized =
                serde_json::to_vec(state).expect("account state serialization should never fail");
            hasher.update(&serialized);
        }

        let digest = hasher.finalize();
        let mut root = [0u8; 32];
        root.copy_from_slice(&digest);
        root
    }

    /// Number of accounts in the state.
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Sum of all account balances.
    pub fn total_supply(&self) -> u64 {
        self.accounts.values().map(|a| a.balance).sum()
    }

    /// SHA-256 hash of a transaction (used for receipts).
    fn hash_transaction(tx: &trv1_bft::block::Transaction) -> [u8; 32] {
        let encoded =
            serde_json::to_vec(tx).expect("transaction serialization should never fail");
        let digest = Sha256::digest(&encoded);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        hash
    }
}

impl Default for StateDB {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trv1_bft::block::Transaction;

    fn alice() -> [u8; 32] {
        [1u8; 32]
    }

    fn bob() -> [u8; 32] {
        [2u8; 32]
    }

    fn charlie() -> [u8; 32] {
        [3u8; 32]
    }

    fn setup_funded_state() -> StateDB {
        let mut db = StateDB::new();
        db.set_account(alice(), AccountState::new(1000));
        db.set_account(bob(), AccountState::new(500));
        db
    }

    // --- Basic account operations ---

    #[test]
    fn test_new_state_db_is_empty() {
        let db = StateDB::new();
        assert_eq!(db.account_count(), 0);
        assert_eq!(db.total_supply(), 0);
    }

    #[test]
    fn test_set_and_get_account() {
        let mut db = StateDB::new();
        db.set_account(alice(), AccountState::new(100));
        let acct = db.get_account(&alice()).unwrap();
        assert_eq!(acct.balance, 100);
        assert_eq!(acct.nonce, 0);
    }

    #[test]
    fn test_get_nonexistent_account() {
        let db = StateDB::new();
        assert!(db.get_account(&alice()).is_none());
    }

    #[test]
    fn test_get_or_create_account() {
        let mut db = StateDB::new();
        let acct = db.get_or_create_account(&alice());
        assert_eq!(acct.balance, 0);
        assert_eq!(acct.nonce, 0);
        assert_eq!(db.account_count(), 1);
    }

    #[test]
    fn test_get_or_create_existing() {
        let mut db = StateDB::new();
        db.set_account(alice(), AccountState::new(999));
        let acct = db.get_or_create_account(&alice());
        assert_eq!(acct.balance, 999);
    }

    // --- Transfer tests ---

    #[test]
    fn test_transfer_happy_path() {
        let mut db = setup_funded_state();
        let result = db.apply_transfer(&alice(), &bob(), 200, 0);
        assert!(result.is_ok());

        assert_eq!(db.get_account(&alice()).unwrap().balance, 800);
        assert_eq!(db.get_account(&alice()).unwrap().nonce, 1);
        assert_eq!(db.get_account(&bob()).unwrap().balance, 700);
    }

    #[test]
    fn test_transfer_creates_recipient() {
        let mut db = StateDB::new();
        db.set_account(alice(), AccountState::new(500));
        let result = db.apply_transfer(&alice(), &charlie(), 100, 0);
        assert!(result.is_ok());
        assert_eq!(db.get_account(&charlie()).unwrap().balance, 100);
        assert_eq!(db.account_count(), 2);
    }

    #[test]
    fn test_transfer_insufficient_balance() {
        let mut db = setup_funded_state();
        let result = db.apply_transfer(&alice(), &bob(), 2000, 0);
        assert!(matches!(result, Err(StateError::InsufficientBalance { .. })));
        // State should be unchanged
        assert_eq!(db.get_account(&alice()).unwrap().balance, 1000);
        assert_eq!(db.get_account(&alice()).unwrap().nonce, 0);
        assert_eq!(db.get_account(&bob()).unwrap().balance, 500);
    }

    #[test]
    fn test_transfer_wrong_nonce() {
        let mut db = setup_funded_state();
        let result = db.apply_transfer(&alice(), &bob(), 100, 5);
        assert!(matches!(result, Err(StateError::InvalidNonce { .. })));
        // State should be unchanged
        assert_eq!(db.get_account(&alice()).unwrap().balance, 1000);
        assert_eq!(db.get_account(&alice()).unwrap().nonce, 0);
    }

    #[test]
    fn test_transfer_from_nonexistent() {
        let mut db = StateDB::new();
        let result = db.apply_transfer(&alice(), &bob(), 100, 0);
        assert!(matches!(result, Err(StateError::AccountNotFound)));
    }

    #[test]
    fn test_sequential_transfers_nonce_increments() {
        let mut db = setup_funded_state();
        assert!(db.apply_transfer(&alice(), &bob(), 100, 0).is_ok());
        assert!(db.apply_transfer(&alice(), &bob(), 100, 1).is_ok());
        assert!(db.apply_transfer(&alice(), &bob(), 100, 2).is_ok());
        assert_eq!(db.get_account(&alice()).unwrap().nonce, 3);
        assert_eq!(db.get_account(&alice()).unwrap().balance, 700);
        assert_eq!(db.get_account(&bob()).unwrap().balance, 800);
    }

    #[test]
    fn test_self_transfer() {
        let mut db = setup_funded_state();
        let result = db.apply_transfer(&alice(), &alice(), 100, 0);
        assert!(result.is_ok());
        // Balance stays the same, nonce increments
        assert_eq!(db.get_account(&alice()).unwrap().balance, 1000);
        assert_eq!(db.get_account(&alice()).unwrap().nonce, 1);
    }

    // --- State root tests ---

    #[test]
    fn test_state_root_deterministic() {
        let db1 = setup_funded_state();
        let db2 = setup_funded_state();
        assert_eq!(db1.compute_state_root(), db2.compute_state_root());
    }

    #[test]
    fn test_state_root_changes_after_transfer() {
        let mut db = setup_funded_state();
        let root_before = db.compute_state_root();
        db.apply_transfer(&alice(), &bob(), 100, 0).unwrap();
        let root_after = db.compute_state_root();
        assert_ne!(root_before, root_after);
    }

    #[test]
    fn test_state_root_empty_db() {
        let db = StateDB::new();
        let root = db.compute_state_root();
        // Empty state should still produce a valid hash
        assert_ne!(root.len(), 0);
    }

    #[test]
    fn test_state_root_insertion_order_independent() {
        let mut db1 = StateDB::new();
        db1.set_account(alice(), AccountState::new(100));
        db1.set_account(bob(), AccountState::new(200));

        let mut db2 = StateDB::new();
        db2.set_account(bob(), AccountState::new(200));
        db2.set_account(alice(), AccountState::new(100));

        assert_eq!(db1.compute_state_root(), db2.compute_state_root());
    }

    // --- apply_block tests ---

    #[test]
    fn test_apply_block_all_success() {
        let mut db = setup_funded_state();
        let txs = vec![
            Transaction {
                from: alice(),
                to: bob(),
                amount: 100,
                nonce: 0,
                signature: vec![],
                data: vec![],
            },
            Transaction {
                from: bob(),
                to: charlie(),
                amount: 50,
                nonce: 0,
                signature: vec![],
                data: vec![],
            },
        ];

        let receipts = db.apply_block(&txs);
        assert_eq!(receipts.len(), 2);
        assert!(receipts[0].success);
        assert!(receipts[1].success);
        assert_eq!(db.get_account(&alice()).unwrap().balance, 900);
        assert_eq!(db.get_account(&bob()).unwrap().balance, 550);
        assert_eq!(db.get_account(&charlie()).unwrap().balance, 50);
    }

    #[test]
    fn test_apply_block_mixed_success_failure() {
        let mut db = setup_funded_state();
        let txs = vec![
            // Good tx: Alice -> Bob 100
            Transaction {
                from: alice(),
                to: bob(),
                amount: 100,
                nonce: 0,
                signature: vec![],
                data: vec![],
            },
            // Bad tx: Alice -> Bob with wrong nonce (should be 1, using 0)
            Transaction {
                from: alice(),
                to: bob(),
                amount: 50,
                nonce: 0, // wrong nonce, should be 1
                signature: vec![],
                data: vec![],
            },
            // Good tx: Bob -> Charlie
            Transaction {
                from: bob(),
                to: charlie(),
                amount: 10,
                nonce: 0,
                signature: vec![],
                data: vec![],
            },
        ];

        let receipts = db.apply_block(&txs);
        assert_eq!(receipts.len(), 3);
        assert!(receipts[0].success);
        assert!(!receipts[1].success); // wrong nonce
        assert!(receipts[1].error.is_some());
        assert!(receipts[2].success);

        // Only the successful txs should have taken effect
        assert_eq!(db.get_account(&alice()).unwrap().balance, 900);
        assert_eq!(db.get_account(&bob()).unwrap().balance, 590);
        assert_eq!(db.get_account(&charlie()).unwrap().balance, 10);
    }

    #[test]
    fn test_apply_block_overdraft_failure() {
        let mut db = StateDB::new();
        db.set_account(alice(), AccountState::new(50));

        let txs = vec![Transaction {
            from: alice(),
            to: bob(),
            amount: 1000,
            nonce: 0,
            signature: vec![],
            data: vec![],
        }];

        let receipts = db.apply_block(&txs);
        assert_eq!(receipts.len(), 1);
        assert!(!receipts[0].success);
        // Alice balance unchanged
        assert_eq!(db.get_account(&alice()).unwrap().balance, 50);
    }

    #[test]
    fn test_apply_block_empty() {
        let mut db = setup_funded_state();
        let receipts = db.apply_block(&[]);
        assert!(receipts.is_empty());
    }

    // --- total_supply ---

    #[test]
    fn test_total_supply() {
        let db = setup_funded_state();
        assert_eq!(db.total_supply(), 1500);
    }

    #[test]
    fn test_total_supply_after_transfer() {
        let mut db = setup_funded_state();
        db.apply_transfer(&alice(), &bob(), 100, 0).unwrap();
        // Total supply shouldn't change from transfers
        assert_eq!(db.total_supply(), 1500);
    }
}
