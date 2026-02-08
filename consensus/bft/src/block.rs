use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::{BlockHash, Height, ValidatorId};

/// Block header containing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub height: Height,
    pub timestamp: u64,
    pub parent_hash: BlockHash,
    pub proposer: ValidatorId,
    pub state_root: [u8; 32],
    pub tx_merkle_root: [u8; 32],
}

/// A single transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub from: [u8; 32],
    pub to: [u8; 32],
    pub amount: u64,
    pub nonce: u64,
    pub signature: Vec<u8>,
    pub data: Vec<u8>,
}

impl Transaction {
    /// Compute the signing message for this transaction.
    /// SHA256(from ++ to ++ amount.to_le_bytes() ++ nonce.to_le_bytes() ++ data)
    pub fn signing_message(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.from);
        hasher.update(self.to);
        hasher.update(self.amount.to_le_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(&self.data);
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }

    /// Sign this transaction with the given signing key.
    /// Stores the signature in self.signature.
    pub fn sign(&mut self, signing_key: &SigningKey) {
        use ed25519_dalek::Signer;
        let msg = self.signing_message();
        let sig = signing_key.sign(&msg);
        self.signature = sig.to_bytes().to_vec();
    }

    /// Verify the transaction signature.
    /// Returns true if the signature is valid for self.from as the public key.
    pub fn verify_signature(&self) -> bool {
        if self.signature.len() != 64 {
            return false;
        }
        let sig_bytes: [u8; 64] = match self.signature.as_slice().try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let sig = match Signature::from_bytes(&sig_bytes) {
            sig => sig,
        };
        let vk = match VerifyingKey::from_bytes(&self.from) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let msg = self.signing_message();
        vk.verify(&msg, &sig).is_ok()
    }

    /// Compute a unique hash for this transaction.
    pub fn hash(&self) -> [u8; 32] {
        let encoded = bincode::serialize(self).expect("tx serialization should never fail");
        let digest = Sha256::digest(&encoded);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        hash
    }
}

/// A full block: header + body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Compute the SHA-256 hash of the block header.
    pub fn hash(&self) -> BlockHash {
        let encoded = bincode::serialize(&self.header)
            .expect("block header serialization should never fail");
        let digest = Sha256::digest(&encoded);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        BlockHash(hash)
    }

    /// Compute a Merkle root from the block's transactions.
    /// Uses a simple binary Merkle tree with SHA-256.
    pub fn compute_tx_merkle_root(transactions: &[Transaction]) -> [u8; 32] {
        if transactions.is_empty() {
            return [0u8; 32];
        }

        let mut leaves: Vec<[u8; 32]> = transactions
            .iter()
            .map(|tx| {
                let encoded = bincode::serialize(tx).expect("tx serialization should never fail");
                let digest = Sha256::digest(&encoded);
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&digest);
                hash
            })
            .collect();

        while leaves.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in leaves.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(chunk[0]);
                if chunk.len() == 2 {
                    hasher.update(chunk[1]);
                } else {
                    // Odd leaf: duplicate it
                    hasher.update(chunk[0]);
                }
                let digest = hasher.finalize();
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&digest);
                next_level.push(hash);
            }
            leaves = next_level;
        }

        leaves[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Height;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn make_test_block(tx_count: usize) -> Block {
        let signing_key = SigningKey::generate(&mut OsRng);
        let proposer = ValidatorId(signing_key.verifying_key());

        let transactions: Vec<Transaction> = (0..tx_count)
            .map(|i| Transaction {
                from: [i as u8; 32],
                to: [(i + 1) as u8; 32],
                amount: 100 * (i as u64 + 1),
                nonce: i as u64,
                signature: vec![0u8; 64],
                data: vec![],
            })
            .collect();

        let tx_merkle_root = Block::compute_tx_merkle_root(&transactions);

        Block {
            header: BlockHeader {
                height: Height(1),
                timestamp: 1700000000,
                parent_hash: BlockHash::default(),
                proposer,
                state_root: [0u8; 32],
                tx_merkle_root,
            },
            transactions,
        }
    }

    #[test]
    fn test_block_hash_deterministic() {
        let block = make_test_block(2);
        let h1 = block.hash();
        let h2 = block.hash();
        assert_eq!(h1, h2, "same block must produce same hash");
        assert!(!h1.is_zero(), "hash should not be zero");
    }

    #[test]
    fn test_different_blocks_different_hashes() {
        let b1 = make_test_block(1);
        let mut b2 = make_test_block(1);
        b2.header.height = Height(2);
        assert_ne!(b1.hash(), b2.hash());
    }

    #[test]
    fn test_merkle_root_empty() {
        let root = Block::compute_tx_merkle_root(&[]);
        assert_eq!(root, [0u8; 32]);
    }

    #[test]
    fn test_merkle_root_single_tx() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![],
            data: vec![],
        };
        let root = Block::compute_tx_merkle_root(&[tx]);
        assert_ne!(root, [0u8; 32]);
    }

    #[test]
    fn test_merkle_root_deterministic() {
        let txs: Vec<Transaction> = (0..4)
            .map(|i| Transaction {
                from: [i as u8; 32],
                to: [(i + 1) as u8; 32],
                amount: 100,
                nonce: i as u64,
                signature: vec![],
                data: vec![],
            })
            .collect();
        let r1 = Block::compute_tx_merkle_root(&txs);
        let r2 = Block::compute_tx_merkle_root(&txs);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_merkle_root_odd_tx_count() {
        let txs: Vec<Transaction> = (0..3)
            .map(|i| Transaction {
                from: [i as u8; 32],
                to: [(i + 1) as u8; 32],
                amount: 100,
                nonce: i as u64,
                signature: vec![],
                data: vec![],
            })
            .collect();
        let root = Block::compute_tx_merkle_root(&txs);
        assert_ne!(root, [0u8; 32]);
    }

    #[test]
    fn test_sign_and_verify_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pubkey = signing_key.verifying_key();

        let mut tx = Transaction {
            from: pubkey.to_bytes(),
            to: [2u8; 32],
            amount: 500,
            nonce: 0,
            signature: vec![],
            data: vec![1, 2, 3],
        };

        tx.sign(&signing_key);
        assert_eq!(tx.signature.len(), 64);
        assert!(tx.verify_signature(), "valid signature must verify");
    }

    #[test]
    fn test_tampered_tx_fails_verification() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pubkey = signing_key.verifying_key();

        let mut tx = Transaction {
            from: pubkey.to_bytes(),
            to: [2u8; 32],
            amount: 500,
            nonce: 0,
            signature: vec![],
            data: vec![],
        };

        tx.sign(&signing_key);
        assert!(tx.verify_signature());

        // Tamper with the amount
        tx.amount = 999;
        assert!(!tx.verify_signature(), "tampered tx must fail verification");
    }

    #[test]
    fn test_wrong_signer_fails_verification() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let wrong_key = SigningKey::generate(&mut OsRng);

        let mut tx = Transaction {
            from: wrong_key.verifying_key().to_bytes(), // from != signer
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![],
            data: vec![],
        };

        tx.sign(&signing_key); // signed by different key
        assert!(
            !tx.verify_signature(),
            "signature from wrong key must fail"
        );
    }

    #[test]
    fn test_empty_signature_fails() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pubkey = signing_key.verifying_key();

        let tx = Transaction {
            from: pubkey.to_bytes(),
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![], // no signature
            data: vec![],
        };

        assert!(!tx.verify_signature(), "empty signature must fail");
    }

    #[test]
    fn test_tx_hash_uniqueness() {
        let tx1 = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        };

        let tx2 = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 200, // different amount
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        };

        assert_ne!(tx1.hash(), tx2.hash(), "different txs must have different hashes");
    }

    #[test]
    fn test_tx_hash_deterministic() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        };

        assert_eq!(tx.hash(), tx.hash(), "same tx must produce same hash");
    }

    #[test]
    fn test_signing_message_deterministic() {
        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![],
            data: vec![10, 20],
        };

        assert_eq!(
            tx.signing_message(),
            tx.signing_message(),
            "signing message must be deterministic"
        );
    }
}
