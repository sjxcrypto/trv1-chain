use serde::{Deserialize, Serialize};
use thiserror::Error;
use trv1_bft::ConsensusMessage;

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
}
