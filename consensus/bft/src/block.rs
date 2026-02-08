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
}
