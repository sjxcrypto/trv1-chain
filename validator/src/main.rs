use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use libp2p::identity::Keypair;
use libp2p::Multiaddr;
use tokio::signal;
use tokio::sync::mpsc;

use trv1_bft::block::{Block, BlockHeader, Transaction};
use trv1_bft::{
    BftStateMachine, BlockHash, ConsensusMessage, Height, Proposal, Round, TimeoutConfig,
    TimeoutEvent, TimeoutStep, ValidatorId, Vote, VoteType,
};
use trv1_fees::{FeeConfig, FeeMarket};
use trv1_genesis::GenesisConfig;

use trv1_net::network::NetworkConfig;
use trv1_net::{ConsensusNetwork, NetworkHandle};
use trv1_rewards::DeveloperRewards;
use trv1_rpc::server::{RpcServer, RpcState};
use trv1_rpc::types::{BlockResponse, ValidatorResponse};
use trv1_slashing::SlashingEngine;
use trv1_staking::StakingPool;
use trv1_state::{AccountState, StateDB};
use trv1_storage::{StorageConfig, TieredStorage};
use trv1_validator_set::{ValidatorSetConfig, ValidatorSetManager};

/// TRv1 Validator Node
#[derive(Parser)]
#[command(name = "trv1-validator", version, about = "TRv1 validator node")]
struct Args {
    /// Path to the genesis file
    #[arg(long, default_value = "genesis.json")]
    genesis: PathBuf,

    /// Data directory for storage
    #[arg(long, default_value = "/tmp/trv1-data")]
    data_dir: PathBuf,

    /// P2P listen address (libp2p multiaddr format)
    #[arg(long, default_value = "/ip4/0.0.0.0/tcp/30333")]
    listen: String,

    /// JSON-RPC server port
    #[arg(long, default_value = "9944")]
    rpc_port: u16,

    /// Path to validator signing key file (hex-encoded ed25519 secret key).
    /// If absent, the node runs in observer mode.
    #[arg(long)]
    validator_key: Option<PathBuf>,

    /// Comma-separated list of peer multiaddrs to dial on startup.
    #[arg(long, value_delimiter = ',')]
    peers: Vec<String>,
}

/// Format a byte slice as a hex string.
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Get the current Unix timestamp in seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Load an ed25519 signing key from a hex-encoded file.
fn load_signing_key(path: &PathBuf) -> Result<SigningKey, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)?;
    let hex_str = contents.trim();
    let bytes = hex::decode(hex_str)?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "signing key must be exactly 32 bytes")?;
    Ok(SigningKey::from_bytes(&key_bytes))
}

/// Build a block from pending transactions.
fn build_block(
    height: Height,
    parent_hash: BlockHash,
    proposer: &ValidatorId,
    transactions: Vec<Transaction>,
    state_db: &StateDB,
) -> Block {
    let tx_merkle_root = Block::compute_tx_merkle_root(&transactions);
    let state_root = state_db.compute_state_root();

    Block {
        header: BlockHeader {
            height,
            timestamp: now_secs(),
            parent_hash,
            proposer: proposer.clone(),
            state_root,
            tx_merkle_root,
        },
        transactions,
    }
}

/// Compute the block hash for a proposal.
fn compute_block_hash(block: &Block) -> BlockHash {
    block.hash()
}

/// Sign a proposal for the given block.
fn sign_proposal(
    height: Height,
    round: Round,
    block_hash: BlockHash,
    signing_key: &SigningKey,
) -> Proposal {
    let proposer = ValidatorId(signing_key.verifying_key());
    let msg = format!(
        "propose:{}:{}:{}",
        height.0,
        round.0,
        to_hex(&block_hash.0)
    );
    let sig = signing_key.sign(msg.as_bytes());

    Proposal {
        height,
        round,
        block_hash,
        proposer,
        signature: sig,
        valid_round: None,
    }
}

/// Sign a vote (prevote or precommit).
fn sign_vote(
    vote_type: VoteType,
    height: Height,
    round: Round,
    block_hash: Option<BlockHash>,
    signing_key: &SigningKey,
) -> Vote {
    Vote::new(vote_type, height, round, block_hash, signing_key)
}

/// Process output messages from the BFT state machine.
/// Returns messages that should be broadcast to the network.
fn process_bft_output(
    msgs: Vec<ConsensusMessage>,
    signing_key: Option<&SigningKey>,
    timeout_tx: &mpsc::Sender<TimeoutEvent>,
    timeout_config: &TimeoutConfig,
) -> Vec<ConsensusMessage> {
    let mut to_broadcast = Vec::new();

    for msg in msgs {
        match msg {
            ConsensusMessage::CastVote(ref vote_template) => {
                if let Some(sk) = signing_key {
                    let signed = sign_vote(
                        vote_template.vote_type,
                        vote_template.height,
                        vote_template.round,
                        vote_template.block_hash,
                        sk,
                    );
                    to_broadcast.push(ConsensusMessage::CastVote(signed));
                }
            }
            ConsensusMessage::ScheduleTimeout(te) => {
                let duration_ms = timeout_config.timeout_for(te.step, te.round);
                let tx = timeout_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
                    let _ = tx.send(te).await;
                });
            }
            ConsensusMessage::CommitBlock { .. } => {
                to_broadcast.push(msg);
            }
            ConsensusMessage::ProposeBlock { .. } => {
                to_broadcast.push(msg);
            }
        }
    }

    to_broadcast
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    tracing::info!("TRv1 Validator starting");

    // --- Load genesis configuration ---
    tracing::info!(path = %args.genesis.display(), "loading genesis");
    let genesis = GenesisConfig::from_file(&args.genesis).unwrap_or_else(|e| {
        tracing::warn!("Could not load genesis file: {e}, using default testnet");
        GenesisConfig::default_testnet()
    });

    genesis.validate().unwrap_or_else(|e| {
        tracing::error!("Genesis validation failed: {e}");
        std::process::exit(1);
    });

    let genesis_hash_hex = to_hex(&genesis.genesis_hash);
    tracing::info!(
        chain_id = %genesis.chain_id,
        validators = genesis.validators.len(),
        genesis_hash = %genesis_hash_hex,
        "genesis loaded"
    );

    // --- Initialize storage ---
    let warm_path = args.data_dir.join("warm");
    let cold_path = args.data_dir.join("cold");
    std::fs::create_dir_all(&warm_path)?;
    std::fs::create_dir_all(&cold_path)?;

    let storage_config = StorageConfig {
        lru_capacity: 10_000,
        nvme_path: warm_path.to_string_lossy().into_owned(),
        archive_path: cold_path.to_string_lossy().into_owned(),
        max_ram_bytes: 512 * 1024 * 1024,
    };
    let storage = TieredStorage::new(&storage_config)?;
    tracing::info!("tiered storage initialized");

    // --- Initialize economics ---
    let mut staking_pool = StakingPool::new();
    let fee_market = FeeMarket::new(FeeConfig::default(), genesis.chain_params.base_fee_floor)?;
    let developer_rewards = DeveloperRewards::new();

    tracing::info!(
        base_fee = fee_market.current_base_fee(),
        "fee market initialized"
    );

    // --- Initialize validator set ---
    let validator_set_config = ValidatorSetConfig {
        active_set_cap: genesis.chain_params.max_validators,
        epoch_length: genesis.chain_params.epoch_length,
        min_stake: 1_000_000,
    };
    let mut validator_set = ValidatorSetManager::with_config(validator_set_config);

    // Register genesis validators into the staking pool and validator set.
    for gv in &genesis.validators {
        if let Err(e) =
            staking_pool.stake(gv.pubkey, gv.initial_stake, trv1_staking::LockTier::Delegator)
        {
            tracing::warn!(
                pubkey = %to_hex(&gv.pubkey),
                error = %e,
                "failed to stake genesis validator"
            );
        }
        match validator_set.register_validator(
            gv.pubkey,
            gv.initial_stake,
            gv.commission_rate,
            0,
        ) {
            Ok(status) => {
                tracing::info!(
                    pubkey = %to_hex(&gv.pubkey),
                    ?status,
                    stake = gv.initial_stake,
                    "registered genesis validator"
                );
            }
            Err(e) => {
                tracing::warn!(
                    pubkey = %to_hex(&gv.pubkey),
                    error = %e,
                    "failed to register genesis validator"
                );
            }
        }
    }

    // --- Initialize slashing ---
    let slashing_engine = SlashingEngine::new();
    tracing::info!("slashing engine initialized");

    // --- Create RPC state with shared mempool, state DB, and real genesis validators ---
    let genesis_validators: Vec<ValidatorResponse> = genesis
        .validators
        .iter()
        .map(|gv| ValidatorResponse {
            pubkey: to_hex(&gv.pubkey),
            stake: gv.initial_stake,
            commission_rate: gv.commission_rate,
            status: "Active".to_string(),
            performance_score: 10_000,
        })
        .collect();

    // Channel for tx gossip: RPC submissions → event loop → P2P broadcast
    let (tx_gossip_tx, mut tx_gossip_rx) = mpsc::channel::<Transaction>(256);

    let rpc_state = Arc::new(
        RpcState::new(
            Arc::new(parking_lot::RwLock::new(trv1_mempool::TransactionPool::new(
                trv1_mempool::MempoolConfig::default(),
            ))),
            Arc::new(parking_lot::RwLock::new(StateDB::new())),
            genesis_validators,
        )
        .with_tx_gossip(tx_gossip_tx),
    );

    // Try to load persisted state, otherwise populate from genesis accounts
    let state_file = args.data_dir.join("state.json");
    if state_file.exists() {
        match StateDB::load_from_file(&state_file) {
            Ok(loaded) => {
                let mut db = rpc_state.state_db.write();
                *db = loaded;
                tracing::info!(
                    accounts = db.account_count(),
                    total_supply = db.total_supply(),
                    "state database restored from {}", state_file.display()
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to load state file, falling back to genesis");
                let mut db = rpc_state.state_db.write();
                for acct in &genesis.accounts {
                    db.set_account(acct.pubkey, AccountState::new(acct.balance));
                }
                tracing::info!(
                    accounts = db.account_count(),
                    total_supply = db.total_supply(),
                    "state database initialized from genesis"
                );
            }
        }
    } else {
        let mut db = rpc_state.state_db.write();
        for acct in &genesis.accounts {
            db.set_account(acct.pubkey, AccountState::new(acct.balance));
        }
        tracing::info!(
            accounts = db.account_count(),
            total_supply = db.total_supply(),
            "state database initialized from genesis"
        );
    }

    tracing::info!("transaction mempool initialized");

    // --- Load validator signing key (if provided) ---
    let signing_key: Option<SigningKey> = match &args.validator_key {
        Some(path) => {
            let sk = load_signing_key(path).unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to load validator key");
                std::process::exit(1);
            });
            let vk = sk.verifying_key();
            tracing::info!(
                pubkey = %to_hex(vk.as_bytes()),
                "validator signing key loaded"
            );
            Some(sk)
        }
        None => {
            tracing::info!("no validator key provided, running in observer mode");
            None
        }
    };

    // --- Initialize BFT consensus ---
    let bft_validators: Vec<ValidatorId> = genesis
        .validators
        .iter()
        .filter_map(|gv| {
            VerifyingKey::from_bytes(&gv.pubkey)
                .ok()
                .map(ValidatorId)
        })
        .collect();

    // Find our index in the validator set
    let our_validator_index: Option<usize> = signing_key.as_ref().and_then(|sk| {
        let our_vk = sk.verifying_key();
        bft_validators.iter().position(|v| v.0 == our_vk)
    });

    if let Some(idx) = our_validator_index {
        tracing::info!(index = idx, "participating in consensus as validator");
    } else if signing_key.is_some() {
        tracing::warn!("signing key loaded but not found in genesis validator set -- observer mode");
    }

    let timeout_config = TimeoutConfig::default();
    let mut bft = BftStateMachine::new(
        Height(0),
        bft_validators.clone(),
        our_validator_index,
        timeout_config,
    );

    let mode = if our_validator_index.is_some() {
        "validator"
    } else {
        "observer"
    };
    tracing::info!(
        bft_validators = bft_validators.len(),
        mode,
        "BFT consensus initialized"
    );

    // --- Initialize P2P networking ---
    let libp2p_keypair = if let Some(ref sk) = signing_key {
        let mut secret_bytes = sk.to_bytes().to_vec();
        Keypair::ed25519_from_bytes(&mut secret_bytes)
            .unwrap_or_else(|_| Keypair::generate_ed25519())
    } else {
        Keypair::generate_ed25519()
    };

    let listen_addr: Multiaddr = args.listen.parse().unwrap_or_else(|e| {
        tracing::error!(error = %format!("{e:?}"), "invalid listen address");
        std::process::exit(1);
    });

    let net_config = NetworkConfig {
        listen_address: listen_addr.clone(),
        ..NetworkConfig::default()
    };

    let (mut handle, mut runner) =
        ConsensusNetwork::new(libp2p_keypair, net_config).unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to create consensus network");
            std::process::exit(1);
        });

    tracing::info!(
        peer_id = %handle.local_peer_id(),
        "P2P network created"
    );

    runner.start(listen_addr).unwrap_or_else(|e| {
        tracing::error!(error = %e, "failed to start P2P listener");
        std::process::exit(1);
    });

    // Dial initial peers
    for peer_addr_str in &args.peers {
        let peer_addr_str = peer_addr_str.trim();
        if peer_addr_str.is_empty() {
            continue;
        }
        match peer_addr_str.parse::<Multiaddr>() {
            Ok(addr) => {
                if let Err(e) = runner.dial(addr.clone()) {
                    tracing::warn!(addr = %addr, error = %e, "failed to dial peer");
                } else {
                    tracing::info!(addr = %addr, "dialing peer");
                }
            }
            Err(e) => {
                tracing::warn!(addr = %peer_addr_str, error = %format!("{e:?}"), "invalid peer address");
            }
        }
    }

    // Spawn the swarm event loop as a background task
    tokio::spawn(runner.run());

    // Extract the transaction receiver so we can poll it independently in select!
    let mut net_tx_rx = handle.take_tx_receiver();

    // --- Wrap remaining mutable state ---
    let fee_market = Arc::new(std::sync::RwLock::new(fee_market));
    let staking_pool = Arc::new(std::sync::RwLock::new(staking_pool));
    let validator_set = Arc::new(std::sync::RwLock::new(validator_set));
    let _slashing_engine = Arc::new(std::sync::RwLock::new(slashing_engine));
    let developer_rewards = Arc::new(std::sync::RwLock::new(developer_rewards));
    let _storage = Arc::new(storage);

    // Track the last committed block hash
    let mut last_block_hash = BlockHash::default();

    // Set initial RPC state
    {
        *rpc_state.current_height.write() = 0;
        *rpc_state.validator_count.write() = bft_validators.len();
        *rpc_state.base_fee.write() = fee_market.read().unwrap().current_base_fee();
    }

    let rpc_server = RpcServer::new(args.rpc_port, rpc_state.clone());
    tracing::info!(port = args.rpc_port, "starting RPC server");

    // --- Timeout channel for BFT timeouts ---
    let (timeout_tx, mut timeout_rx) = mpsc::channel::<TimeoutEvent>(64);

    // --- Start BFT consensus round 0 ---
    let initial_msgs = bft.start_round(Round(0));
    let broadcasts = process_bft_output(
        initial_msgs,
        signing_key.as_ref(),
        &timeout_tx,
        &timeout_config,
    );

    // If we are the proposer for round 0, build and broadcast a block
    if bft.is_proposer() {
        if let Some(ref sk) = signing_key {
            let txs = rpc_state.mempool.read().get_pending_ordered(100);
            let block = build_block(
                Height(0),
                last_block_hash,
                &ValidatorId(sk.verifying_key()),
                txs,
                &rpc_state.state_db.read(),
            );
            let block_hash = compute_block_hash(&block);
            let proposal = sign_proposal(Height(0), Round(0), block_hash, sk);

            tracing::info!(
                height = 0,
                block_hash = %to_hex(&block_hash.0),
                txs = block.transactions.len(),
                "proposing block"
            );

            // Feed the proposal into our own BFT state machine
            bft.on_proposal(&proposal, Some(&block));

            if let Err(e) = handle
                .broadcast_message(&ConsensusMessage::ProposeBlock {
                    proposal,
                    block: Some(block),
                })
                .await
            {
                tracing::warn!(error = %e, "failed to broadcast proposal");
            }
        }
    }

    // Broadcast any initial messages
    for msg in &broadcasts {
        if let Err(e) = handle.broadcast_message(msg).await {
            tracing::debug!(error = %e, "failed to broadcast initial message");
        }
    }

    // --- Main event loop ---
    tracing::info!("entering main event loop");

    tokio::select! {
        // Run the RPC server.
        result = rpc_server.start() => {
            match result {
                Ok(addr) => tracing::info!(%addr, "RPC server stopped"),
                Err(e) => tracing::error!(error = %e, "RPC server error"),
            }
        }

        // Run the consensus + P2P event loop.
        _ = async {
            loop {
                tokio::select! {
                    // Receive messages from the P2P network.
                    net_msg = handle.next_message() => {
                        let Some(net_msg) = net_msg else {
                            tracing::warn!("network message channel closed");
                            break;
                        };

                        let bft_outputs = match net_msg.message {
                            ConsensusMessage::ProposeBlock { proposal, block } => {
                                tracing::debug!(
                                    height = proposal.height.0,
                                    round = proposal.round.0,
                                    proposer = %to_hex(proposal.proposer.as_bytes()),
                                    has_block = block.is_some(),
                                    "received proposal"
                                );
                                bft.on_proposal(&proposal, block.as_ref())
                            }
                            ConsensusMessage::CastVote(ref vote) => {
                                match vote.vote_type {
                                    VoteType::Prevote => {
                                        tracing::debug!(
                                            height = vote.height.0,
                                            round = vote.round.0,
                                            "received prevote"
                                        );
                                        bft.on_prevote(vote)
                                    }
                                    VoteType::Precommit => {
                                        tracing::debug!(
                                            height = vote.height.0,
                                            round = vote.round.0,
                                            "received precommit"
                                        );
                                        bft.on_precommit(vote)
                                    }
                                }
                            }
                            ConsensusMessage::CommitBlock { height, block_hash } => {
                                tracing::info!(
                                    height = height.0,
                                    block_hash = %to_hex(&block_hash.0),
                                    "received commit block from network"
                                );
                                let committed_block = bft.get_committed_block(&block_hash).cloned();
                                apply_commit(
                                    height,
                                    block_hash,
                                    committed_block.as_ref(),
                                    &rpc_state,
                                    &fee_market,
                                    &mut last_block_hash,
                                    &genesis,
                                    &staking_pool,
                                    &developer_rewards,
                                    &validator_set,
                                );

                                let next_height = Height(height.0 + 1);
                                let advance_msgs = bft.advance_height(next_height);
                                let inner_broadcasts = process_bft_output(
                                    advance_msgs,
                                    signing_key.as_ref(),
                                    &timeout_tx,
                                    &timeout_config,
                                );

                                if bft.is_proposer() {
                                    if let Some(ref sk) = signing_key {
                                        propose_block(
                                            &handle,
                                            &mut bft,
                                            sk,
                                            next_height,
                                            Round(0),
                                            last_block_hash,
                                            &rpc_state,
                                        ).await;
                                    }
                                }

                                for msg in &inner_broadcasts {
                                    if let Err(e) = handle.broadcast_message(msg).await {
                                        tracing::debug!(error = %e, "failed to broadcast");
                                    }
                                }

                                vec![] // already handled
                            }
                            ConsensusMessage::ScheduleTimeout(_) => {
                                vec![]
                            }
                        };

                        // Process BFT outputs
                        let broadcasts = process_bft_output(
                            bft_outputs,
                            signing_key.as_ref(),
                            &timeout_tx,
                            &timeout_config,
                        );

                        for msg in &broadcasts {
                            match msg {
                                ConsensusMessage::CommitBlock { height, block_hash } => {
                                    tracing::info!(
                                        height = height.0,
                                        block_hash = %to_hex(&block_hash.0),
                                        "committing block"
                                    );

                                    let committed_block = bft.get_committed_block(block_hash).cloned();
                                    apply_commit(
                                        *height,
                                        *block_hash,
                                        committed_block.as_ref(),
                                        &rpc_state,
                                        &fee_market,
                                        &mut last_block_hash,
                                        &genesis,
                                        &staking_pool,
                                        &developer_rewards,
                                        &validator_set,
                                    );

                                    let next_height = Height(height.0 + 1);
                                    let advance_msgs = bft.advance_height(next_height);
                                    let advance_broadcasts = process_bft_output(
                                        advance_msgs,
                                        signing_key.as_ref(),
                                        &timeout_tx,
                                        &timeout_config,
                                    );

                                    if bft.is_proposer() {
                                        if let Some(ref sk) = signing_key {
                                            propose_block(
                                                &handle,
                                                &mut bft,
                                                sk,
                                                next_height,
                                                Round(0),
                                                last_block_hash,
                                                &rpc_state,
                                            ).await;
                                        }
                                    }

                                    for adv_msg in &advance_broadcasts {
                                        if let Err(e) = handle.broadcast_message(adv_msg).await {
                                            tracing::debug!(error = %e, "failed to broadcast");
                                        }
                                    }
                                }
                                _ => {
                                    if let Err(e) = handle.broadcast_message(msg).await {
                                        tracing::debug!(error = %e, "failed to broadcast");
                                    }
                                }
                            }
                        }
                    }

                    // Handle BFT timeout events.
                    Some(timeout_event) = timeout_rx.recv() => {
                        tracing::debug!(
                            height = timeout_event.height.0,
                            round = timeout_event.round.0,
                            step = ?timeout_event.step,
                            "timeout fired"
                        );

                        let bft_outputs = bft.on_timeout(timeout_event);
                        let broadcasts = process_bft_output(
                            bft_outputs,
                            signing_key.as_ref(),
                            &timeout_tx,
                            &timeout_config,
                        );

                        // After a precommit timeout advances to a new round, check if we are proposer
                        if bft.is_proposer()
                            && timeout_event.step == TimeoutStep::Precommit
                        {
                            if let Some(ref sk) = signing_key {
                                let h = bft.height;
                                let r = bft.round;
                                propose_block(
                                    &handle,
                                    &mut bft,
                                    sk,
                                    h,
                                    r,
                                    last_block_hash,
                                    &rpc_state,
                                ).await;
                            }
                        }

                        for msg in &broadcasts {
                            match msg {
                                ConsensusMessage::CommitBlock { height, block_hash } => {
                                    let committed_block = bft.get_committed_block(block_hash).cloned();
                                    apply_commit(
                                        *height,
                                        *block_hash,
                                        committed_block.as_ref(),
                                        &rpc_state,
                                        &fee_market,
                                        &mut last_block_hash,
                                        &genesis,
                                        &staking_pool,
                                        &developer_rewards,
                                        &validator_set,
                                    );

                                    let next_height = Height(height.0 + 1);
                                    let advance_msgs = bft.advance_height(next_height);
                                    let advance_broadcasts = process_bft_output(
                                        advance_msgs,
                                        signing_key.as_ref(),
                                        &timeout_tx,
                                        &timeout_config,
                                    );

                                    if bft.is_proposer() {
                                        if let Some(ref sk) = signing_key {
                                            propose_block(
                                                &handle,
                                                &mut bft,
                                                sk,
                                                next_height,
                                                Round(0),
                                                last_block_hash,
                                                &rpc_state,
                                            ).await;
                                        }
                                    }

                                    for adv_msg in &advance_broadcasts {
                                        if let Err(e) = handle.broadcast_message(adv_msg).await {
                                            tracing::debug!(error = %e, "failed to broadcast");
                                        }
                                    }
                                }
                                _ => {
                                    if let Err(e) = handle.broadcast_message(msg).await {
                                        tracing::debug!(error = %e, "failed to broadcast");
                                    }
                                }
                            }
                        }
                    }

                    // Receive gossiped transactions from other nodes.
                    Some(tx) = net_tx_rx.recv() => {
                        tracing::debug!(
                            from = %to_hex(&tx.from),
                            to = %to_hex(&tx.to),
                            amount = tx.amount,
                            nonce = tx.nonce,
                            "received gossiped transaction"
                        );

                        match rpc_state.mempool.write().add_transaction(tx) {
                            Ok(_) => {
                                tracing::debug!("gossiped transaction added to mempool");
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "rejected gossiped transaction");
                            }
                        }
                    }

                    // Broadcast locally submitted transactions (from RPC) to P2P network.
                    Some(tx) = tx_gossip_rx.recv() => {
                        if let Err(e) = handle.broadcast_transaction(&tx).await {
                            tracing::debug!(error = %e, "failed to gossip transaction to network");
                        }
                    }
                }
            }
        } => {}

        // Wait for shutdown signal (SIGINT/SIGTERM).
        _ = signal::ctrl_c() => {
            tracing::info!("received shutdown signal");
        }
    }

    // Persist state to disk before exiting
    {
        let db = rpc_state.state_db.read();
        if let Err(e) = db.save_to_file(&state_file) {
            tracing::error!(error = %e, "failed to save state to disk");
        } else {
            tracing::info!(
                accounts = db.account_count(),
                path = %state_file.display(),
                "state saved to disk"
            );
        }
    }

    tracing::info!("TRv1 Validator shutting down gracefully");
    Ok(())
}

/// Apply a committed block: execute transactions, update mempool, update RPC state.
///
/// `committed_block` is the actual block from the BFT proposal cache. If available,
/// we use its transactions instead of blindly pulling from the mempool — this ensures
/// we execute exactly the transactions the proposer included.
fn apply_commit(
    height: Height,
    block_hash: BlockHash,
    committed_block: Option<&Block>,
    rpc_state: &Arc<RpcState>,
    fee_market: &Arc<std::sync::RwLock<FeeMarket>>,
    last_block_hash: &mut BlockHash,
    genesis: &GenesisConfig,
    staking_pool: &Arc<std::sync::RwLock<StakingPool>>,
    _developer_rewards: &Arc<std::sync::RwLock<DeveloperRewards>>,
    validator_set: &Arc<std::sync::RwLock<ValidatorSetManager>>,
) {
    // Use the committed block's transactions if available, else fall back to mempool
    let (txs, proposer_hex) = match committed_block {
        Some(block) => {
            let proposer = to_hex(block.header.proposer.as_bytes());
            (block.transactions.clone(), proposer)
        }
        None => {
            let txs = rpc_state.mempool.read().get_pending_ordered(100);
            (txs, String::new())
        }
    };

    let receipts = {
        let mut db = rpc_state.state_db.write();
        db.apply_block(&txs)
    };

    let success_count = receipts.iter().filter(|r| r.success).count();
    let fail_count = receipts.len() - success_count;

    tracing::info!(
        height = height.0,
        block_hash = %to_hex(&block_hash.0),
        total_txs = txs.len(),
        success = success_count,
        failed = fail_count,
        "block committed"
    );

    // Remove committed transactions from mempool
    let committed_hashes: Vec<[u8; 32]> = txs
        .iter()
        .map(|tx: &Transaction| tx.hash())
        .collect();
    rpc_state.mempool.write().remove_committed(&committed_hashes);

    // Update fee market
    {
        let mut fm = fee_market.write().unwrap();
        let gas_used = (txs.len() as u64) * 21_000;
        fm.update_base_fee(gas_used);
    }

    // Update RPC state
    *rpc_state.current_height.write() = height.0;
    *rpc_state.base_fee.write() = fee_market.read().unwrap().current_base_fee();

    // Store committed block for RPC queries
    rpc_state.block_store.write().push(BlockResponse {
        height: height.0,
        timestamp: now_secs(),
        parent_hash: to_hex(&last_block_hash.0),
        proposer: proposer_hex,
        tx_count: txs.len(),
        block_hash: to_hex(&block_hash.0),
    });

    // Update last block hash
    *last_block_hash = block_hash;

    // Epoch handling
    let epoch_length = genesis.chain_params.epoch_length;
    if epoch_length > 0 && height.0 > 0 && height.0 % epoch_length == 0 {
        let epoch = height.0 / epoch_length;
        tracing::info!(epoch, "epoch boundary reached");

        let pool = staking_pool.read().unwrap();
        let swaps = validator_set.write().unwrap().epoch_rotation(&pool);
        tracing::info!(
            swaps = swaps.len(),
            "validator set rotated"
        );

        let rewards = pool.distribute_epoch_rewards();
        let total_rewards: u64 = rewards.iter().map(|(_, r)| r).sum();
        tracing::info!(total_rewards, recipients = rewards.len(), "epoch rewards distributed");
    }
}

/// Propose a new block as the round's designated proposer.
///
/// Also feeds the proposal into the local BFT state machine so the proposer
/// caches its own block for later retrieval on commit.
async fn propose_block(
    handle: &NetworkHandle,
    bft: &mut BftStateMachine,
    signing_key: &SigningKey,
    height: Height,
    round: Round,
    parent_hash: BlockHash,
    rpc_state: &Arc<RpcState>,
) {
    let txs = rpc_state.mempool.read().get_pending_ordered(100);
    let proposer_id = ValidatorId(signing_key.verifying_key());
    let block = build_block(
        height,
        parent_hash,
        &proposer_id,
        txs,
        &rpc_state.state_db.read(),
    );
    let block_hash = compute_block_hash(&block);
    let proposal = sign_proposal(height, round, block_hash, signing_key);

    tracing::info!(
        height = height.0,
        round = round.0,
        block_hash = %to_hex(&block_hash.0),
        txs = block.transactions.len(),
        "proposing block"
    );

    // Feed into local BFT so it caches the block
    bft.on_proposal(&proposal, Some(&block));

    if let Err(e) = handle
        .broadcast_message(&ConsensusMessage::ProposeBlock {
            proposal,
            block: Some(block),
        })
        .await
    {
        tracing::warn!(error = %e, "failed to broadcast proposal");
    }
}
