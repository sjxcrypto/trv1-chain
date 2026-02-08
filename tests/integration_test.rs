//! Integration smoke tests for the TRv1 blockchain.
//!
//! These tests verify end-to-end flows across multiple crates:
//! genesis configuration, state initialization, transaction signing,
//! mempool acceptance, and state transitions.

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

use trv1_bft::block::Transaction;
use trv1_genesis::GenesisConfig;
use trv1_mempool::{MempoolConfig, TransactionPool};
use trv1_state::{AccountState, StateDB};

/// Helper: create a signed transaction from the given key.
fn make_signed_tx(
    signing_key: &SigningKey,
    to: [u8; 32],
    amount: u64,
    nonce: u64,
) -> Transaction {
    let from = signing_key.verifying_key().to_bytes();
    let mut tx = Transaction {
        from,
        to,
        amount,
        nonce,
        signature: vec![],
        data: vec![],
    };
    tx.sign(signing_key);
    tx
}

// ---------------------------------------------------------------------------
// Genesis tests
// ---------------------------------------------------------------------------

#[test]
fn genesis_default_testnet_is_valid() {
    let genesis = GenesisConfig::default_testnet();
    genesis.validate().expect("default testnet genesis should be valid");
    assert!(!genesis.chain_id.is_empty());
    assert!(!genesis.validators.is_empty());
    assert_ne!(genesis.genesis_hash, [0u8; 32]);
}

#[test]
fn genesis_roundtrip_through_file() {
    let genesis = GenesisConfig::default_testnet();
    let tmp = std::env::temp_dir().join("trv1_test_genesis.json");

    genesis.to_file(&tmp).expect("genesis write should succeed");
    let loaded = GenesisConfig::from_file(&tmp).expect("genesis read should succeed");

    assert_eq!(genesis.chain_id, loaded.chain_id);
    assert_eq!(genesis.validators.len(), loaded.validators.len());
    assert_eq!(genesis.genesis_hash, loaded.genesis_hash);

    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// State DB tests
// ---------------------------------------------------------------------------

#[test]
fn state_db_initialized_from_genesis_accounts() {
    let genesis = GenesisConfig::default_testnet();
    let mut state_db = StateDB::new();

    for acct in &genesis.accounts {
        state_db.set_account(acct.pubkey, AccountState::new(acct.balance));
    }

    assert_eq!(state_db.account_count(), genesis.accounts.len());

    for acct in &genesis.accounts {
        let stored = state_db.get_account(&acct.pubkey).expect("account should exist");
        assert_eq!(stored.balance, acct.balance);
        assert_eq!(stored.nonce, 0);
    }
}

#[test]
fn state_db_total_supply_matches_genesis() {
    let genesis = GenesisConfig::default_testnet();
    let mut state_db = StateDB::new();

    let expected_supply: u64 = genesis.accounts.iter().map(|a| a.balance).sum();

    for acct in &genesis.accounts {
        state_db.set_account(acct.pubkey, AccountState::new(acct.balance));
    }

    assert_eq!(state_db.total_supply(), expected_supply);
}

// ---------------------------------------------------------------------------
// Transaction signing & verification tests
// ---------------------------------------------------------------------------

#[test]
fn transaction_sign_and_verify() {
    let sk = SigningKey::generate(&mut OsRng);
    let tx = make_signed_tx(&sk, [2u8; 32], 500, 0);

    assert!(
        tx.verify_signature(),
        "freshly signed transaction should verify"
    );
}

#[test]
fn transaction_tampered_amount_fails_verification() {
    let sk = SigningKey::generate(&mut OsRng);
    let mut tx = make_signed_tx(&sk, [2u8; 32], 500, 0);

    // Tamper with the amount after signing.
    tx.amount = 999;
    assert!(
        !tx.verify_signature(),
        "tampered transaction should fail verification"
    );
}

#[test]
fn transaction_different_keys_produce_different_hashes() {
    let sk1 = SigningKey::generate(&mut OsRng);
    let sk2 = SigningKey::generate(&mut OsRng);
    let tx1 = make_signed_tx(&sk1, [2u8; 32], 100, 0);
    let tx2 = make_signed_tx(&sk2, [2u8; 32], 100, 0);

    assert_ne!(tx1.hash(), tx2.hash());
}

// ---------------------------------------------------------------------------
// Mempool tests
// ---------------------------------------------------------------------------

#[test]
fn mempool_accepts_valid_signed_transaction() {
    let mut pool = TransactionPool::new(MempoolConfig::default());
    let sk = SigningKey::generate(&mut OsRng);
    let tx = make_signed_tx(&sk, [2u8; 32], 100, 0);

    pool.add_transaction(tx).expect("valid tx should be accepted");
    assert_eq!(pool.pending_count(), 1);
}

#[test]
fn mempool_rejects_duplicate_transaction() {
    let mut pool = TransactionPool::new(MempoolConfig::default());
    let sk = SigningKey::generate(&mut OsRng);
    let tx = make_signed_tx(&sk, [2u8; 32], 100, 0);

    pool.add_transaction(tx.clone()).expect("first should succeed");
    let result = pool.add_transaction(tx);
    assert!(result.is_err(), "duplicate should be rejected");
    assert_eq!(pool.pending_count(), 1);
}

#[test]
fn mempool_multiple_senders() {
    let mut pool = TransactionPool::new(MempoolConfig::default());

    for _ in 0..5 {
        let sk = SigningKey::generate(&mut OsRng);
        let tx = make_signed_tx(&sk, [2u8; 32], 50, 0);
        pool.add_transaction(tx).expect("should accept from different senders");
    }

    assert_eq!(pool.pending_count(), 5);
}

// ---------------------------------------------------------------------------
// State transition tests
// ---------------------------------------------------------------------------

#[test]
fn state_transition_transfer_updates_balances() {
    let mut state_db = StateDB::new();

    let sender_sk = SigningKey::generate(&mut OsRng);
    let sender_pk = sender_sk.verifying_key().to_bytes();
    let recipient = [2u8; 32];

    // Fund the sender account.
    state_db.set_account(sender_pk, AccountState::new(10_000));
    state_db.set_account(recipient, AccountState::new(0));

    // Create a signed transfer.
    let tx = make_signed_tx(&sender_sk, recipient, 3_000, 0);

    // Apply the block.
    let receipts = state_db.apply_block(&[tx]);
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].success, "transfer should succeed");

    // Check resulting balances.
    let sender_acct = state_db.get_account(&sender_pk).expect("sender should exist");
    let recv_acct = state_db.get_account(&recipient).expect("recipient should exist");

    assert_eq!(sender_acct.balance, 7_000);
    assert_eq!(recv_acct.balance, 3_000);
    assert_eq!(sender_acct.nonce, 1, "sender nonce should increment");
}

#[test]
fn state_transition_insufficient_balance_fails() {
    let mut state_db = StateDB::new();

    let sender_sk = SigningKey::generate(&mut OsRng);
    let sender_pk = sender_sk.verifying_key().to_bytes();
    let recipient = [3u8; 32];

    // Fund sender with less than transfer amount.
    state_db.set_account(sender_pk, AccountState::new(100));

    let tx = make_signed_tx(&sender_sk, recipient, 500, 0);
    let receipts = state_db.apply_block(&[tx]);

    assert_eq!(receipts.len(), 1);
    assert!(!receipts[0].success, "transfer with insufficient balance should fail");

    // Sender balance should be unchanged.
    let sender_acct = state_db.get_account(&sender_pk).expect("sender should exist");
    assert_eq!(sender_acct.balance, 100);
}

#[test]
fn state_transition_multiple_transactions_in_block() {
    let mut state_db = StateDB::new();

    let sk_a = SigningKey::generate(&mut OsRng);
    let pk_a = sk_a.verifying_key().to_bytes();
    let sk_b = SigningKey::generate(&mut OsRng);
    let pk_b = sk_b.verifying_key().to_bytes();
    let pk_c = [0xcc; 32];

    state_db.set_account(pk_a, AccountState::new(10_000));
    state_db.set_account(pk_b, AccountState::new(5_000));

    // A sends 2000 to C, B sends 1000 to C.
    let tx1 = make_signed_tx(&sk_a, pk_c, 2_000, 0);
    let tx2 = make_signed_tx(&sk_b, pk_c, 1_000, 0);

    let receipts = state_db.apply_block(&[tx1, tx2]);
    assert_eq!(receipts.len(), 2);

    let acct_a = state_db.get_account(&pk_a).unwrap();
    let acct_b = state_db.get_account(&pk_b).unwrap();
    let acct_c = state_db.get_account(&pk_c).unwrap();

    assert_eq!(acct_a.balance, 8_000);
    assert_eq!(acct_b.balance, 4_000);
    assert_eq!(acct_c.balance, 3_000);
}

#[test]
fn state_root_changes_after_block() {
    let mut state_db = StateDB::new();

    let sk = SigningKey::generate(&mut OsRng);
    let pk = sk.verifying_key().to_bytes();
    state_db.set_account(pk, AccountState::new(10_000));

    let root_before = state_db.compute_state_root();

    let tx = make_signed_tx(&sk, [5u8; 32], 1_000, 0);
    state_db.apply_block(&[tx]);

    let root_after = state_db.compute_state_root();
    assert_ne!(root_before, root_after, "state root should change after a block");
}
