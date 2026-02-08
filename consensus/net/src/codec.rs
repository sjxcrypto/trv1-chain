use serde::{Deserialize, Serialize};
use thiserror::Error;
use trv1_bft::ConsensusMessage;
use trv1_bft::block::Transaction;

/// Errors during network message encoding/decoding.
#[derive(Debug, Error)]
pub enum CodecError {
    #[error("serialization failed: {0}")]
    Serialize(#[from] bincode::Error),
}

/// A network-level wrapper around a consensus message,
/// including the sender's peer identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMessage {
    /// The libp2p PeerId of the sender, serialized as bytes.
    pub sender: Vec<u8>,
    /// The inner consensus message.
    pub message: ConsensusMessage,
}

impl NetworkMessage {
    /// Encode a NetworkMessage to bytes using bincode.
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        Ok(bincode::serialize(self)?)
    }

    /// Decode a NetworkMessage from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, CodecError> {
        Ok(bincode::deserialize(data)?)
    }
}

/// Encode a raw ConsensusMessage to bytes (for gossipsub publishing).
pub fn encode_consensus_message(msg: &ConsensusMessage) -> Result<Vec<u8>, CodecError> {
    Ok(bincode::serialize(msg)?)
}

/// Decode a raw ConsensusMessage from bytes.
pub fn decode_consensus_message(data: &[u8]) -> Result<ConsensusMessage, CodecError> {
    Ok(bincode::deserialize(data)?)
}

/// Encode a Transaction to bytes (for gossipsub publishing on the tx topic).
pub fn encode_transaction(tx: &Transaction) -> Result<Vec<u8>, CodecError> {
    Ok(bincode::serialize(tx)?)
}

/// Decode a Transaction from bytes.
pub fn decode_transaction(data: &[u8]) -> Result<Transaction, CodecError> {
    Ok(bincode::deserialize(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trv1_bft::{BlockHash, Height, Round, TimeoutEvent, TimeoutStep};

    #[test]
    fn test_network_message_roundtrip() {
        let msg = NetworkMessage {
            sender: vec![1, 2, 3, 4],
            message: ConsensusMessage::CommitBlock {
                height: Height(42),
                block_hash: BlockHash([0xAB; 32]),
            },
        };

        let encoded = msg.encode().expect("encode should succeed");
        let decoded = NetworkMessage::decode(&encoded).expect("decode should succeed");

        assert_eq!(decoded.sender, msg.sender);
        match decoded.message {
            ConsensusMessage::CommitBlock {
                height,
                block_hash,
            } => {
                assert_eq!(height, Height(42));
                assert_eq!(block_hash, BlockHash([0xAB; 32]));
            }
            _ => panic!("unexpected message variant"),
        }
    }

    #[test]
    fn test_consensus_message_roundtrip_timeout() {
        let msg = ConsensusMessage::ScheduleTimeout(TimeoutEvent {
            height: Height(10),
            round: Round(3),
            step: TimeoutStep::Prevote,
        });

        let bytes = encode_consensus_message(&msg).expect("encode");
        let decoded = decode_consensus_message(&bytes).expect("decode");

        match decoded {
            ConsensusMessage::ScheduleTimeout(te) => {
                assert_eq!(te.height, Height(10));
                assert_eq!(te.round, Round(3));
                assert_eq!(te.step, TimeoutStep::Prevote);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_decode_invalid_data() {
        let result = NetworkMessage::decode(&[0xFF, 0x00]);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_sender() {
        let msg = NetworkMessage {
            sender: vec![],
            message: ConsensusMessage::CommitBlock {
                height: Height(0),
                block_hash: BlockHash([0; 32]),
            },
        };

        let encoded = msg.encode().unwrap();
        let decoded = NetworkMessage::decode(&encoded).unwrap();
        assert!(decoded.sender.is_empty());
    }

    #[test]
    fn test_transaction_encode_decode_roundtrip() {
        use trv1_bft::block::Transaction;

        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 1000,
            nonce: 7,
            signature: vec![0xAB; 64],
            data: vec![1, 2, 3, 4],
        };

        let encoded = super::encode_transaction(&tx).expect("encode tx");
        let decoded = super::decode_transaction(&encoded).expect("decode tx");

        assert_eq!(decoded.from, tx.from);
        assert_eq!(decoded.to, tx.to);
        assert_eq!(decoded.amount, tx.amount);
        assert_eq!(decoded.nonce, tx.nonce);
        assert_eq!(decoded.signature, tx.signature);
        assert_eq!(decoded.data, tx.data);
    }

    #[test]
    fn test_transaction_decode_invalid_data() {
        let result = super::decode_transaction(&[0xFF, 0x00]);
        assert!(result.is_err());
    }

    #[test]
    fn test_propose_block_with_block_roundtrip() {
        use ed25519_dalek::{Signer, SigningKey};
        use rand::rngs::OsRng;
        use trv1_bft::block::{Block, BlockHeader, Transaction};
        use trv1_bft::{Proposal, ValidatorId};

        let key = SigningKey::generate(&mut OsRng);
        let proposer = ValidatorId(key.verifying_key());

        let txs = vec![Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 100,
            nonce: 0,
            signature: vec![0u8; 64],
            data: vec![],
        }];

        let block = Block {
            header: BlockHeader {
                height: Height(5),
                timestamp: 1700000000,
                parent_hash: BlockHash([0; 32]),
                proposer: proposer.clone(),
                state_root: [0u8; 32],
                tx_merkle_root: Block::compute_tx_merkle_root(&txs),
            },
            transactions: txs,
        };

        let block_hash = block.hash();
        let sig = key.sign(b"proposal");

        let proposal = Proposal {
            height: Height(5),
            round: Round(0),
            block_hash,
            proposer,
            signature: sig,
            valid_round: None,
        };

        let msg = ConsensusMessage::ProposeBlock {
            proposal: proposal.clone(),
            block: Some(block),
        };

        let bytes = encode_consensus_message(&msg).expect("encode");
        let decoded = decode_consensus_message(&bytes).expect("decode");

        match decoded {
            ConsensusMessage::ProposeBlock {
                proposal: p,
                block: b,
            } => {
                assert_eq!(p.height, Height(5));
                assert_eq!(p.block_hash, block_hash);
                let blk = b.expect("block should be present");
                assert_eq!(blk.hash(), block_hash);
                assert_eq!(blk.transactions.len(), 1);
            }
            _ => panic!("expected ProposeBlock"),
        }
    }

    #[test]
    fn test_propose_block_without_block_roundtrip() {
        use ed25519_dalek::{Signer, SigningKey};
        use rand::rngs::OsRng;
        use trv1_bft::{Proposal, ValidatorId};

        let key = SigningKey::generate(&mut OsRng);
        let sig = key.sign(b"proposal");

        let proposal = Proposal {
            height: Height(1),
            round: Round(0),
            block_hash: BlockHash([0xAA; 32]),
            proposer: ValidatorId(key.verifying_key()),
            signature: sig,
            valid_round: None,
        };

        let msg = ConsensusMessage::ProposeBlock {
            proposal: proposal.clone(),
            block: None,
        };

        let bytes = encode_consensus_message(&msg).expect("encode");
        let decoded = decode_consensus_message(&bytes).expect("decode");

        match decoded {
            ConsensusMessage::ProposeBlock {
                proposal: p,
                block: b,
            } => {
                assert_eq!(p.height, Height(1));
                assert!(b.is_none());
            }
            _ => panic!("expected ProposeBlock"),
        }
    }
}
