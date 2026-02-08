use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use trv1_bft::block::Transaction;

use crate::types::MempoolError;

/// Basic structural validation of a transaction.
pub fn validate_transaction(tx: &Transaction) -> Result<(), MempoolError> {
    if tx.from == [0u8; 32] {
        return Err(MempoolError::InvalidTransaction(
            "sender cannot be zero address".into(),
        ));
    }

    if tx.signature.is_empty() {
        return Err(MempoolError::InvalidTransaction(
            "signature cannot be empty".into(),
        ));
    }

    Ok(())
}

/// Build the message that must be signed for a transaction.
/// Message = SHA256(from ++ to ++ amount.to_le_bytes() ++ nonce.to_le_bytes() ++ data)
pub fn build_signing_message(tx: &Transaction) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(tx.from);
    hasher.update(tx.to);
    hasher.update(tx.amount.to_le_bytes());
    hasher.update(tx.nonce.to_le_bytes());
    hasher.update(&tx.data);
    let digest = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest);
    hash
}

/// Verify the ed25519 signature on a transaction.
/// The verifying key is derived from `tx.from` (the 32-byte public key).
pub fn verify_signature(tx: &Transaction) -> Result<(), MempoolError> {
    let verifying_key = VerifyingKey::from_bytes(&tx.from)
        .map_err(|_| MempoolError::InvalidSignature)?;

    let sig_bytes: [u8; 64] = tx
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| MempoolError::InvalidSignature)?;

    let signature = Signature::from_bytes(&sig_bytes);

    let message = build_signing_message(tx);

    verifying_key
        .verify_strict(&message, &signature)
        .map_err(|_| MempoolError::InvalidSignature)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use ed25519_dalek::Signer;
    use rand::rngs::OsRng;

    fn make_signed_tx(signing_key: &SigningKey, to: [u8; 32], amount: u64, nonce: u64) -> Transaction {
        let from: [u8; 32] = signing_key.verifying_key().to_bytes();
        let data = vec![];

        let mut tx = Transaction {
            from,
            to,
            amount,
            nonce,
            signature: vec![],
            data,
        };

        let message = build_signing_message(&tx);
        let sig = signing_key.sign(&message);
        tx.signature = sig.to_bytes().to_vec();
        tx
    }

    #[test]
    fn test_validate_zero_sender() {
        let tx = Transaction {
            from: [0u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        };
        let err = validate_transaction(&tx).unwrap_err();
        assert!(matches!(err, MempoolError::InvalidTransaction(_)));
    }

    #[test]
    fn test_validate_empty_signature() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![],
            data: vec![],
        };
        let err = validate_transaction(&tx).unwrap_err();
        assert!(matches!(err, MempoolError::InvalidTransaction(_)));
    }

    #[test]
    fn test_validate_valid_tx() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        };
        assert!(validate_transaction(&tx).is_ok());
    }

    #[test]
    fn test_verify_signature_valid() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let tx = make_signed_tx(&signing_key, [2u8; 32], 100, 0);
        assert!(verify_signature(&tx).is_ok());
    }

    #[test]
    fn test_verify_signature_tampered_amount() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let mut tx = make_signed_tx(&signing_key, [2u8; 32], 100, 0);
        tx.amount = 999; // tamper
        assert!(matches!(
            verify_signature(&tx),
            Err(MempoolError::InvalidSignature)
        ));
    }

    #[test]
    fn test_verify_signature_wrong_key() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let other_key = SigningKey::generate(&mut OsRng);
        let mut tx = make_signed_tx(&signing_key, [2u8; 32], 100, 0);
        // Replace from with a different key
        tx.from = other_key.verifying_key().to_bytes();
        assert!(matches!(
            verify_signature(&tx),
            Err(MempoolError::InvalidSignature)
        ));
    }

    #[test]
    fn test_verify_signature_bad_length() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 32], // wrong length (should be 64)
            data: vec![],
        };
        assert!(matches!(
            verify_signature(&tx),
            Err(MempoolError::InvalidSignature)
        ));
    }
}
