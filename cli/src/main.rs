use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::path::PathBuf;

use trv1_genesis::builder::GenesisBuilder;
use trv1_genesis::{GenesisConfig, GenesisValidator};
use trv1_staking::LockTier;

/// TRv1 Blockchain CLI
#[derive(Parser)]
#[command(name = "trv1", version, about = "TRv1 blockchain command-line interface")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new Ed25519 keypair
    Keygen {
        /// Output file for the secret key
        #[arg(short, long, default_value = "validator.key")]
        output: PathBuf,
    },

    /// Genesis configuration commands
    Genesis {
        #[command(subcommand)]
        command: GenesisCommands,
    },

    /// Display staking tier information
    Stake {
        /// Amount to stake (in smallest token unit)
        #[arg(long)]
        amount: u64,

        /// Staking tier name (NoLock, ThreeMonth, SixMonth, OneYear, Permanent)
        #[arg(long)]
        tier: String,
    },

    /// Query chain state (placeholder)
    Query {
        #[command(subcommand)]
        command: QueryCommands,
    },

    /// Print version information
    Version,
}

#[derive(Subcommand)]
enum GenesisCommands {
    /// Generate a default genesis file
    Init {
        /// Chain ID for the genesis
        #[arg(long)]
        chain_id: String,

        /// Output path for the genesis JSON file
        #[arg(long, default_value = "genesis.json")]
        output: PathBuf,
    },

    /// Add a validator to an existing genesis file
    AddValidator {
        /// Path to the existing genesis file
        #[arg(long)]
        genesis: PathBuf,

        /// Validator public key (hex-encoded, 64 characters)
        #[arg(long)]
        pubkey: String,

        /// Initial stake amount
        #[arg(long)]
        stake: u64,

        /// Commission rate in basis points (default: 500 = 5%)
        #[arg(long, default_value = "500")]
        commission: u16,
    },
}

#[derive(Subcommand)]
enum QueryCommands {
    /// Query an account balance
    Balance {
        /// Account address (hex-encoded public key)
        #[arg(long)]
        address: String,
    },

    /// Query the active validator set
    Validators,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Keygen { output } => cmd_keygen(output),
        Commands::Genesis { command } => match command {
            GenesisCommands::Init { chain_id, output } => cmd_genesis_init(&chain_id, output),
            GenesisCommands::AddValidator {
                genesis,
                pubkey,
                stake,
                commission,
            } => cmd_genesis_add_validator(genesis, &pubkey, stake, commission),
        },
        Commands::Stake { amount, tier } => cmd_stake(amount, &tier),
        Commands::Query { command } => match command {
            QueryCommands::Balance { address } => cmd_query_balance(&address),
            QueryCommands::Validators => cmd_query_validators(),
        },
        Commands::Version => cmd_version(),
    }
}

fn cmd_keygen(output: PathBuf) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let pubkey_hex = hex::encode(verifying_key.as_bytes());
    let secret_hex = hex::encode(signing_key.to_bytes());

    std::fs::write(&output, &secret_hex).unwrap_or_else(|e| {
        eprintln!("Error writing key file: {e}");
        std::process::exit(1);
    });

    println!("Generated new Ed25519 keypair");
    println!("  Public key: {pubkey_hex}");
    println!("  Secret key saved to: {}", output.display());
}

fn cmd_genesis_init(chain_id: &str, output: PathBuf) {
    let config = GenesisConfig::default_testnet();
    let config = GenesisBuilder::new(chain_id)
        .with_genesis_time(config.genesis_time)
        .with_validator(config.validators[0].pubkey, 10_000_000, 500)
        .with_validator(config.validators[1].pubkey, 10_000_000, 500)
        .with_validator(config.validators[2].pubkey, 10_000_000, 500)
        .with_validator(config.validators[3].pubkey, 10_000_000, 500)
        .with_account(config.accounts[0].pubkey, 100_000_000)
        .with_account(config.accounts[1].pubkey, 100_000_000)
        .with_account(config.accounts[2].pubkey, 100_000_000)
        .with_account(config.accounts[3].pubkey, 100_000_000)
        .build()
        .unwrap_or_else(|e| {
            eprintln!("Error building genesis: {e}");
            std::process::exit(1);
        });

    config.to_file(&output).unwrap_or_else(|e| {
        eprintln!("Error writing genesis file: {e}");
        std::process::exit(1);
    });

    println!("Genesis file created: {}", output.display());
    println!("  Chain ID: {}", config.chain_id);
    println!("  Validators: {}", config.validators.len());
    println!("  Accounts: {}", config.accounts.len());
    println!("  Genesis hash: {}", hex::encode(config.genesis_hash));
}

fn cmd_genesis_add_validator(genesis_path: PathBuf, pubkey_hex: &str, stake: u64, commission: u16) {
    let mut config = GenesisConfig::from_file(&genesis_path).unwrap_or_else(|e| {
        eprintln!("Error reading genesis file: {e}");
        std::process::exit(1);
    });

    let pubkey_bytes = hex::decode(pubkey_hex).unwrap_or_else(|e| {
        eprintln!("Invalid pubkey hex: {e}");
        std::process::exit(1);
    });

    if pubkey_bytes.len() != 32 {
        eprintln!("Public key must be 32 bytes (64 hex characters), got {}", pubkey_bytes.len());
        std::process::exit(1);
    }

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&pubkey_bytes);

    config.validators.push(GenesisValidator {
        pubkey,
        initial_stake: stake,
        commission_rate: commission,
    });

    config.genesis_hash = config.compute_genesis_hash();

    config.validate().unwrap_or_else(|e| {
        eprintln!("Genesis validation failed: {e}");
        std::process::exit(1);
    });

    config.to_file(&genesis_path).unwrap_or_else(|e| {
        eprintln!("Error writing genesis file: {e}");
        std::process::exit(1);
    });

    println!("Added validator to genesis");
    println!("  Pubkey: {pubkey_hex}");
    println!("  Stake: {stake}");
    println!("  Commission: {} bps", commission);
    println!("  Total validators: {}", config.validators.len());
    println!("  New genesis hash: {}", hex::encode(config.genesis_hash));
}

fn cmd_stake(amount: u64, tier_name: &str) {
    let tier = match tier_name.to_lowercase().as_str() {
        "nolock" | "no_lock" | "none" => LockTier::NoLock,
        "threemonth" | "three_month" | "3month" => LockTier::ThreeMonth,
        "sixmonth" | "six_month" | "6month" => LockTier::SixMonth,
        "oneyear" | "one_year" | "1year" => LockTier::OneYear,
        "permanent" | "perm" => LockTier::Permanent,
        _ => {
            eprintln!("Unknown tier: {tier_name}");
            eprintln!("Valid tiers: NoLock, ThreeMonth, SixMonth, OneYear, Permanent");
            std::process::exit(1);
        }
    };

    let base_apy = 5.0;
    let bonus_apy = tier.bonus_apy() * 100.0;
    let total_apy = base_apy + bonus_apy;
    let multiplier = tier.multiplier();
    let vote_weight = tier.vote_weight();

    let lock_duration = match tier.lock_duration_epochs() {
        Some(0) => "None (instant unlock)".to_string(),
        Some(d) => format!("{d} epochs (~{} days)", d),
        None => "Permanent (never unlocks)".to_string(),
    };

    let yearly_reward = (amount as f64 * total_apy / 100.0) as u64;

    println!("Staking Information");
    println!("  Amount: {amount}");
    println!("  Tier: {tier_name}");
    println!("  Lock duration: {lock_duration}");
    println!("  Base APY: {base_apy:.2}%");
    println!("  Bonus APY: {bonus_apy:.2}%");
    println!("  Total APY: {total_apy:.2}%");
    println!("  Reward multiplier: {multiplier:.1}x");
    println!("  Vote weight: {vote_weight:.1}x");
    println!("  Estimated yearly reward: {yearly_reward}");
}

fn cmd_query_balance(address: &str) {
    println!("Query balance for address: {address}");
    println!("  Status: not connected to a node");
    println!("  Use --rpc-url to specify a TRv1 node endpoint");
}

fn cmd_query_validators() {
    println!("Query validator set");
    println!("  Status: not connected to a node");
    println!("  Use --rpc-url to specify a TRv1 node endpoint");
}

fn cmd_version() {
    println!(
        "trv1 {} (TRv1 blockchain CLI)",
        env!("CARGO_PKG_VERSION")
    );
}
