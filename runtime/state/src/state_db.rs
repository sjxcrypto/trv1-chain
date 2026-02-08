use std::collections::HashMap;
use std::path::Path;

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

    /// Serialize the state database to JSON and write it to a file.
    ///
    /// Account keys (`[u8; 32]`) are stored as hex strings so the output is
    /// valid JSON with string keys.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), StateError> {
        let hex_map: HashMap<String, &AccountState> = self
            .accounts
            .iter()
            .map(|(k, v)| (hex::encode(k), v))
            .collect();

        let json = serde_json::to_string_pretty(&hex_map).map_err(|e| StateError::Json(e.to_string()))?;
        std::fs::write(path, json).map_err(|e| StateError::Io(e.to_string()))?;
        Ok(())
    }

    /// Read a JSON file written by [`save_to_file`](Self::save_to_file) and
    /// reconstruct a `StateDB` from it.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, StateError> {
        let data = std::fs::read_to_string(path).map_err(|e| StateError::Io(e.to_string()))?;
        let hex_map: HashMap<String, AccountState> =
            serde_json::from_str(&data).map_err(|e| StateError::Json(e.to_string()))?;

        let mut accounts = HashMap::with_capacity(hex_map.len());
        for (hex_key, state) in hex_map {
            let bytes = hex::decode(&hex_key).map_err(|e| StateError::Json(e.to_string()))?;
            if bytes.len() != 32 {
                return Err(StateError::Json(format!(
                    "invalid key length: expected 32 bytes, got {}",
                    bytes.len()
                )));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            accounts.insert(key, state);
        }

        Ok(Self { accounts })
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

    // --- Persistence tests ---

    /// Helper: create a unique temp file path for persistence tests.
    fn temp_state_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "trv1_state_test_{}_{}.json",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let path = temp_state_path("roundtrip");
        let db = setup_funded_state();

        db.save_to_file(&path).unwrap();
        let loaded = StateDB::load_from_file(&path).unwrap();

        // Verify all accounts survived the roundtrip
        assert_eq!(loaded.account_count(), db.account_count());
        assert_eq!(loaded.total_supply(), db.total_supply());

        let alice_acct = loaded.get_account(&alice()).unwrap();
        assert_eq!(alice_acct.balance, 1000);
        assert_eq!(alice_acct.nonce, 0);

        let bob_acct = loaded.get_account(&bob()).unwrap();
        assert_eq!(bob_acct.balance, 500);
        assert_eq!(bob_acct.nonce, 0);

        // State roots must match to ensure deterministic reconstruction
        assert_eq!(loaded.compute_state_root(), db.compute_state_root());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = StateDB::load_from_file("/tmp/trv1_definitely_does_not_exist.json");
        assert!(result.is_err());
        match result.unwrap_err() {
            StateError::Io(msg) => assert!(msg.contains("No such file"), "unexpected: {msg}"),
            other => panic!("expected Io error, got: {other:?}"),
        }
    }

    #[test]
    fn test_save_load_preserves_nonces() {
        let path = temp_state_path("nonces");

        let mut db = StateDB::new();
        db.set_account(alice(), AccountState::new(1000));
        db.set_account(bob(), AccountState::new(500));

        // Perform some transfers to bump nonces
        db.apply_transfer(&alice(), &bob(), 100, 0).unwrap();
        db.apply_transfer(&alice(), &bob(), 50, 1).unwrap();
        db.apply_transfer(&bob(), &alice(), 25, 0).unwrap();

        // At this point: alice nonce=2, bob nonce=1
        assert_eq!(db.get_account(&alice()).unwrap().nonce, 2);
        assert_eq!(db.get_account(&bob()).unwrap().nonce, 1);

        db.save_to_file(&path).unwrap();
        let loaded = StateDB::load_from_file(&path).unwrap();

        assert_eq!(loaded.get_account(&alice()).unwrap().nonce, 2);
        assert_eq!(loaded.get_account(&alice()).unwrap().balance, 875);
        assert_eq!(loaded.get_account(&bob()).unwrap().nonce, 1);
        assert_eq!(loaded.get_account(&bob()).unwrap().balance, 625);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_load_empty_state() {
        let path = temp_state_path("empty");
        let db = StateDB::new();

        db.save_to_file(&path).unwrap();
        let loaded = StateDB::load_from_file(&path).unwrap();

        assert_eq!(loaded.account_count(), 0);
        assert_eq!(loaded.total_supply(), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_load_invalid_json() {
        let path = temp_state_path("invalid_json");
        std::fs::write(&path, "this is not json").unwrap();

        let result = StateDB::load_from_file(&path);
        assert!(matches!(result, Err(StateError::Json(_))));

        let _ = std::fs::remove_file(&path);
    }
}
