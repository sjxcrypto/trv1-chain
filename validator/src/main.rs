use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::signal;
use tokio::sync::RwLock;

use trv1_bft::{BftStateMachine, Height, TimeoutConfig, ValidatorId};
use trv1_fees::{FeeConfig, FeeMarket};
use trv1_genesis::GenesisConfig;
use trv1_rewards::DeveloperRewards;
use trv1_rpc::RpcServer;
use trv1_slashing::SlashingEngine;
use trv1_staking::StakingPool;
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
}

/// Format a byte slice as a hex string.
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
            staking_pool.stake(gv.pubkey, gv.initial_stake, trv1_staking::LockTier::NoLock)
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

    // --- Initialize BFT consensus ---
    let bft_validators: Vec<ValidatorId> = genesis
        .validators
        .iter()
        .filter_map(|gv| {
            ed25519_dalek::VerifyingKey::from_bytes(&gv.pubkey)
                .ok()
                .map(ValidatorId)
        })
        .collect();

    tracing::info!(
        bft_validators = bft_validators.len(),
        "BFT validator set created"
    );

    // This node runs as a non-voting observer by default.
    // To participate in consensus, the operator provides a signing key
    // and the node identifies its index in the validator set.
    let bft = BftStateMachine::new(
        Height(0),
        bft_validators,
        None, // observer mode
        TimeoutConfig::default(),
    );
    tracing::info!("BFT consensus initialized in observer mode");

    // --- Initialize P2P networking ---
    // The ConsensusNetwork from trv1_net requires libp2p types (Keypair, Multiaddr)
    // which are not direct dependencies of this binary. In a production setup,
    // we would add libp2p to the validator's Cargo.toml or have trv1_net
    // provide a higher-level builder. For now, we log the intended configuration.
    tracing::info!(
        listen = %args.listen,
        "P2P networking configured (deferred initialization)"
    );

    // --- Start RPC server ---
    let rpc_state = Arc::new(trv1_rpc::server::RpcState::new());
    {
        *rpc_state.current_height.write() = 0;
        *rpc_state.validator_count.write() = genesis.validators.len();
        *rpc_state.base_fee.write() = fee_market.current_base_fee();
    }

    let rpc_server = RpcServer::new(args.rpc_port, rpc_state.clone());
    tracing::info!(port = args.rpc_port, "starting RPC server");

    // Wrap mutable state for concurrent access across async tasks.
    let _staking_pool = Arc::new(RwLock::new(staking_pool));
    let _fee_market = Arc::new(RwLock::new(fee_market));
    let _validator_set = Arc::new(RwLock::new(validator_set));
    let _slashing_engine = Arc::new(RwLock::new(slashing_engine));
    let _developer_rewards = Arc::new(RwLock::new(developer_rewards));
    let _bft = Arc::new(RwLock::new(bft));
    let _storage = Arc::new(storage);

    // --- Main event loop ---
    // In production, this would also include:
    // - The P2P network event loop (network.run())
    // - Consensus message processing
    // - Block production
    // - Epoch reward distribution
    // - Periodic state archival
    tokio::select! {
        // Run the RPC server.
        result = rpc_server.start() => {
            match result {
                Ok(addr) => tracing::info!(%addr, "RPC server stopped"),
                Err(e) => tracing::error!(error = %e, "RPC server error"),
            }
        }

        // Wait for shutdown signal (SIGINT/SIGTERM).
        _ = signal::ctrl_c() => {
            tracing::info!("received shutdown signal");
        }
    }

    tracing::info!("TRv1 Validator shutting down gracefully");
    Ok(())
}
