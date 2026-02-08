use libp2p::{
    futures::StreamExt,
    gossipsub::{self, IdentTopic, MessageAuthenticity},
    identity::Keypair,
    noise, tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing;
use trv1_bft::ConsensusMessage;

use crate::codec::{self, NetworkMessage};
use crate::peer::PeerManager;

/// The gossipsub topic for consensus messages.
pub const CONSENSUS_TOPIC: &str = "trv1-consensus";

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("gossipsub error: {0}")]
    Gossipsub(String),
    #[error("codec error: {0}")]
    Codec(#[from] crate::codec::CodecError),
    #[error("channel closed")]
    ChannelClosed,
}

/// Configuration for the consensus network.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub listen_address: Multiaddr,
    pub heartbeat_interval: Duration,
    pub peer_ban_threshold: i64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_address: "/ip4/0.0.0.0/tcp/30333".parse().unwrap(),
            heartbeat_interval: Duration::from_secs(1),
            peer_ban_threshold: -100,
        }
    }
}

/// The main P2P networking component for consensus message broadcast.
pub struct ConsensusNetwork {
    swarm: Swarm<gossipsub::Behaviour>,
    topic: IdentTopic,
    peer_manager: PeerManager,
    local_peer_id: PeerId,
    /// Channel for delivering received messages to the consumer.
    msg_tx: mpsc::Sender<NetworkMessage>,
    msg_rx: mpsc::Receiver<NetworkMessage>,
}

impl ConsensusNetwork {
    /// Create a new ConsensusNetwork.
    pub fn new(keypair: Keypair, config: NetworkConfig) -> Result<Self, NetworkError> {
        let local_peer_id = PeerId::from(keypair.public());

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(config.heartbeat_interval)
            .validation_mode(gossipsub::ValidationMode::Strict)
            .build()
            .map_err(|e| NetworkError::Gossipsub(e.to_string()))?;

        let gossipsub_behaviour = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| NetworkError::Gossipsub(e.to_string()))?;

        let swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| NetworkError::Transport(e.to_string()))?
            .with_behaviour(|_| Ok(gossipsub_behaviour))
            .map_err(|e| NetworkError::Transport(e.to_string()))?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let topic = IdentTopic::new(CONSENSUS_TOPIC);
        let peer_manager = PeerManager::new(config.peer_ban_threshold);
        let (msg_tx, msg_rx) = mpsc::channel(256);

        Ok(Self {
            swarm,
            topic,
            peer_manager,
            local_peer_id,
            msg_tx,
            msg_rx,
        })
    }

    /// Our local peer ID.
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Get a reference to the peer manager.
    pub fn peer_manager(&self) -> &PeerManager {
        &self.peer_manager
    }

    /// Get a mutable reference to the peer manager.
    pub fn peer_manager_mut(&mut self) -> &mut PeerManager {
        &mut self.peer_manager
    }

    /// Start listening on the configured address.
    pub fn start(&mut self, listen_addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm
            .listen_on(listen_addr)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.swarm
            .behaviour_mut()
            .subscribe(&self.topic)
            .map_err(|e| NetworkError::Gossipsub(e.to_string()))?;
        tracing::info!(peer_id = %self.local_peer_id, "consensus network started");
        Ok(())
    }

    /// Dial a remote peer.
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm
            .dial(addr)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        Ok(())
    }

    /// Broadcast a consensus message to all peers via gossipsub.
    pub fn broadcast_message(&mut self, msg: &ConsensusMessage) -> Result<(), NetworkError> {
        let data = codec::encode_consensus_message(msg)?;
        self.swarm
            .behaviour_mut()
            .publish(self.topic.clone(), data)
            .map_err(|e| NetworkError::Gossipsub(e.to_string()))?;
        Ok(())
    }

    /// Receive the next inbound consensus message (async).
    pub async fn next_message(&mut self) -> Option<NetworkMessage> {
        self.msg_rx.recv().await
    }

    /// Run the swarm event loop. This drives the libp2p networking.
    /// Typically spawned as a background task.
    pub async fn run(&mut self) {
        use libp2p::swarm::SwarmEvent;

        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(gossipsub::Event::Message {
                    propagation_source,
                    message,
                    ..
                }) => {
                    match codec::decode_consensus_message(&message.data) {
                        Ok(consensus_msg) => {
                            let net_msg = NetworkMessage {
                                sender: propagation_source.to_bytes(),
                                message: consensus_msg,
                            };
                            if self.msg_tx.send(net_msg).await.is_err() {
                                tracing::warn!("message channel closed, stopping network loop");
                                return;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                peer = %propagation_source,
                                error = %e,
                                "failed to decode consensus message"
                            );
                            // Penalize peer for bad message
                            self.peer_manager
                                .adjust_score(&propagation_source, -10);
                        }
                    }
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.peer_manager.add_peer(peer_id, None, now);
                    tracing::info!(peer = %peer_id, "peer connected");
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    self.peer_manager.remove_peer(&peer_id);
                    tracing::info!(peer = %peer_id, "peer disconnected");
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!(address = %address, "listening on");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trv1_bft::{BlockHash, Height};

    #[test]
    fn test_network_creation() {
        let keypair = Keypair::generate_ed25519();
        let config = NetworkConfig::default();
        let network = ConsensusNetwork::new(keypair, config);
        assert!(network.is_ok());
    }

    #[test]
    fn test_local_peer_id() {
        let keypair = Keypair::generate_ed25519();
        let expected_id = PeerId::from(keypair.public());
        let config = NetworkConfig::default();
        let network = ConsensusNetwork::new(keypair, config).unwrap();
        assert_eq!(network.local_peer_id(), expected_id);
    }

    #[test]
    fn test_consensus_topic_constant() {
        assert_eq!(CONSENSUS_TOPIC, "trv1-consensus");
    }

    #[test]
    fn test_message_encode_decode_roundtrip() {
        let msg = ConsensusMessage::CommitBlock {
            height: Height(99),
            block_hash: BlockHash([0xFF; 32]),
        };
        let encoded = codec::encode_consensus_message(&msg).unwrap();
        let decoded = codec::decode_consensus_message(&encoded).unwrap();
        match decoded {
            ConsensusMessage::CommitBlock {
                height,
                block_hash,
            } => {
                assert_eq!(height, Height(99));
                assert_eq!(block_hash, BlockHash([0xFF; 32]));
            }
            _ => panic!("wrong variant"),
        }
    }
}
