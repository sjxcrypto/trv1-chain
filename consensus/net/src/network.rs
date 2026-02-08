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
use trv1_bft::block::Transaction;

use crate::codec::{self, NetworkMessage};
use crate::peer::PeerManager;

/// The gossipsub topic for consensus messages.
pub const CONSENSUS_TOPIC: &str = "trv1-consensus";

/// The gossipsub topic for transaction gossip.
pub const TRANSACTION_TOPIC: &str = "trv1-transactions";

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

/// Lightweight handle for sending and receiving consensus messages.
///
/// Used by the consensus loop to interact with the P2P network without
/// owning the swarm directly.
pub struct NetworkHandle {
    /// Send outbound consensus messages to the swarm runner.
    broadcast_tx: mpsc::Sender<ConsensusMessage>,
    /// Receive inbound messages from the network.
    msg_rx: mpsc::Receiver<NetworkMessage>,
    /// Send outbound transactions to the swarm runner for gossip publishing.
    tx_broadcast_tx: mpsc::Sender<Transaction>,
    /// Receive inbound transactions from the network.
    tx_msg_rx: mpsc::Receiver<Transaction>,
    local_peer_id: PeerId,
}

impl NetworkHandle {
    /// Our local peer ID.
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Broadcast a consensus message to all peers via gossipsub.
    ///
    /// This sends the message to the `NetworkRunner` over a channel;
    /// the runner publishes it on the gossipsub topic.
    pub async fn broadcast_message(&self, msg: &ConsensusMessage) -> Result<(), NetworkError> {
        self.broadcast_tx
            .send(msg.clone())
            .await
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// Receive the next inbound consensus message (async).
    pub async fn next_message(&mut self) -> Option<NetworkMessage> {
        self.msg_rx.recv().await
    }

    /// Broadcast a transaction to all peers via the transaction gossipsub topic.
    pub async fn broadcast_transaction(&self, tx: &Transaction) -> Result<(), NetworkError> {
        self.tx_broadcast_tx
            .send(tx.clone())
            .await
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// Receive the next inbound transaction from gossip.
    pub async fn next_transaction(&mut self) -> Option<Transaction> {
        self.tx_msg_rx.recv().await
    }

    /// Extract the transaction receiver so it can be polled independently
    /// (e.g., in a separate `select!` arm without conflicting borrows).
    pub fn take_tx_receiver(&mut self) -> mpsc::Receiver<Transaction> {
        let (_, empty_rx) = mpsc::channel(1);
        std::mem::replace(&mut self.tx_msg_rx, empty_rx)
    }
}

/// Owns and drives the libp2p swarm. Spawned as a background task.
pub struct NetworkRunner {
    swarm: Swarm<gossipsub::Behaviour>,
    topic: IdentTopic,
    tx_topic: IdentTopic,
    peer_manager: PeerManager,
    /// Receives outbound broadcast requests from `NetworkHandle`s.
    broadcast_rx: mpsc::Receiver<ConsensusMessage>,
    /// Sends inbound messages to `NetworkHandle`.
    msg_tx: mpsc::Sender<NetworkMessage>,
    /// Receives outbound transaction broadcast requests from `NetworkHandle`s.
    tx_broadcast_rx: mpsc::Receiver<Transaction>,
    /// Sends inbound transactions to `NetworkHandle`.
    tx_msg_tx: mpsc::Sender<Transaction>,
}

impl NetworkRunner {
    /// Start listening on the given address and subscribe to the consensus and transaction topics.
    pub fn start(&mut self, listen_addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm
            .listen_on(listen_addr)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.swarm
            .behaviour_mut()
            .subscribe(&self.topic)
            .map_err(|e| NetworkError::Gossipsub(e.to_string()))?;
        self.swarm
            .behaviour_mut()
            .subscribe(&self.tx_topic)
            .map_err(|e| NetworkError::Gossipsub(e.to_string()))?;
        tracing::info!("consensus network started");
        Ok(())
    }

    /// Dial a remote peer.
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm
            .dial(addr)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        Ok(())
    }

    /// Run the swarm event loop. Consumes self and drives libp2p networking.
    ///
    /// Uses `tokio::select!` to simultaneously:
    /// - Poll the swarm for incoming events (messages, connections)
    /// - Receive outbound broadcast requests from `NetworkHandle`s
    /// - Receive outbound transaction broadcast requests from `NetworkHandle`s
    pub async fn run(mut self) {
        use libp2p::swarm::SwarmEvent;

        let consensus_topic_hash = self.topic.hash();
        let tx_topic_hash = self.tx_topic.hash();

        loop {
            tokio::select! {
                // Poll the swarm for events.
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::Behaviour(gossipsub::Event::Message {
                            propagation_source,
                            message,
                            ..
                        }) => {
                            if message.topic == consensus_topic_hash {
                                match codec::decode_consensus_message(&message.data) {
                                    Ok(consensus_msg) => {
                                        let net_msg = NetworkMessage {
                                            sender: propagation_source.to_bytes(),
                                            message: consensus_msg,
                                        };
                                        if self.msg_tx.send(net_msg).await.is_err() {
                                            tracing::warn!("consensus message channel closed, stopping network loop");
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            peer = %propagation_source,
                                            error = %e,
                                            "failed to decode consensus message"
                                        );
                                        self.peer_manager
                                            .adjust_score(&propagation_source, -10);
                                    }
                                }
                            } else if message.topic == tx_topic_hash {
                                match codec::decode_transaction(&message.data) {
                                    Ok(tx) => {
                                        if self.tx_msg_tx.send(tx).await.is_err() {
                                            tracing::warn!("transaction message channel closed, stopping network loop");
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            peer = %propagation_source,
                                            error = %e,
                                            "failed to decode transaction message"
                                        );
                                        self.peer_manager
                                            .adjust_score(&propagation_source, -10);
                                    }
                                }
                            } else {
                                tracing::debug!(
                                    topic = ?message.topic,
                                    "received message on unknown topic"
                                );
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

                // Receive outbound consensus broadcast requests from handles.
                Some(msg) = self.broadcast_rx.recv() => {
                    match codec::encode_consensus_message(&msg) {
                        Ok(data) => {
                            if let Err(e) = self.swarm
                                .behaviour_mut()
                                .publish(self.topic.clone(), data)
                            {
                                tracing::debug!(error = %e, "failed to publish consensus gossipsub message");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to encode consensus message for broadcast");
                        }
                    }
                }

                // Receive outbound transaction broadcast requests from handles.
                Some(tx) = self.tx_broadcast_rx.recv() => {
                    match codec::encode_transaction(&tx) {
                        Ok(data) => {
                            if let Err(e) = self.swarm
                                .behaviour_mut()
                                .publish(self.tx_topic.clone(), data)
                            {
                                tracing::debug!(error = %e, "failed to publish transaction gossipsub message");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to encode transaction for broadcast");
                        }
                    }
                }

                else => {
                    tracing::info!("all channels closed, stopping network runner");
                    return;
                }
            }
        }
    }
}

/// Create a new consensus network, returning a handle and a runner.
///
/// - `NetworkHandle` is used by the consensus loop to send/receive messages.
/// - `NetworkRunner` drives the libp2p swarm and must be spawned as a task.
pub struct ConsensusNetwork;

impl ConsensusNetwork {
    pub fn new(
        keypair: Keypair,
        config: NetworkConfig,
    ) -> Result<(NetworkHandle, NetworkRunner), NetworkError> {
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
        let tx_topic = IdentTopic::new(TRANSACTION_TOPIC);
        let peer_manager = PeerManager::new(config.peer_ban_threshold);

        // Channel for inbound consensus messages: runner -> handle
        let (msg_tx, msg_rx) = mpsc::channel(256);
        // Channel for outbound consensus broadcasts: handle -> runner
        let (broadcast_tx, broadcast_rx) = mpsc::channel(256);
        // Channel for inbound transactions: runner -> handle
        let (tx_msg_tx, tx_msg_rx) = mpsc::channel(256);
        // Channel for outbound transaction broadcasts: handle -> runner
        let (tx_broadcast_tx, tx_broadcast_rx) = mpsc::channel(256);

        let handle = NetworkHandle {
            broadcast_tx,
            msg_rx,
            tx_broadcast_tx,
            tx_msg_rx,
            local_peer_id,
        };

        let runner = NetworkRunner {
            swarm,
            topic,
            tx_topic,
            peer_manager,
            broadcast_rx,
            msg_tx,
            tx_broadcast_rx,
            tx_msg_tx,
        };

        Ok((handle, runner))
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
        let result = ConsensusNetwork::new(keypair, config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_local_peer_id() {
        let keypair = Keypair::generate_ed25519();
        let expected_id = PeerId::from(keypair.public());
        let config = NetworkConfig::default();
        let (handle, _runner) = ConsensusNetwork::new(keypair, config).unwrap();
        assert_eq!(handle.local_peer_id(), expected_id);
    }

    #[test]
    fn test_consensus_topic_constant() {
        assert_eq!(CONSENSUS_TOPIC, "trv1-consensus");
    }

    #[test]
    fn test_transaction_topic_constant() {
        assert_eq!(TRANSACTION_TOPIC, "trv1-transactions");
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

    #[tokio::test]
    async fn test_handle_broadcast_and_runner_receives() {
        let keypair = Keypair::generate_ed25519();
        let config = NetworkConfig::default();
        let (handle, mut runner) = ConsensusNetwork::new(keypair, config).unwrap();

        let msg = ConsensusMessage::CommitBlock {
            height: Height(1),
            block_hash: BlockHash([0xAA; 32]),
        };

        // Send a message through the handle
        handle.broadcast_message(&msg).await.unwrap();

        // The runner's broadcast_rx should receive it
        let received = runner.broadcast_rx.recv().await.unwrap();
        match received {
            ConsensusMessage::CommitBlock { height, .. } => {
                assert_eq!(height, Height(1));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn test_handle_tx_broadcast_and_runner_receives() {
        let keypair = Keypair::generate_ed25519();
        let config = NetworkConfig::default();
        let (handle, mut runner) = ConsensusNetwork::new(keypair, config).unwrap();

        let tx = Transaction {
            from: [1u8; 32],
            to: [2u8; 32],
            amount: 500,
            nonce: 42,
            signature: vec![0u8; 64],
            data: vec![10, 20, 30],
        };

        // Send a transaction through the handle
        handle.broadcast_transaction(&tx).await.unwrap();

        // The runner's tx_broadcast_rx should receive it
        let received = runner.tx_broadcast_rx.recv().await.unwrap();
        assert_eq!(received.from, [1u8; 32]);
        assert_eq!(received.to, [2u8; 32]);
        assert_eq!(received.amount, 500);
        assert_eq!(received.nonce, 42);
        assert_eq!(received.data, vec![10, 20, 30]);
    }
}
