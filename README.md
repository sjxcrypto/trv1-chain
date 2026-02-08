# TRv1 Chain

A sovereign Layer 1 blockchain built from scratch in Rust. TRv1 implements Tendermint-style BFT consensus, an EIP-1559 fee market, tiered staking with lock bonuses, validator-only slashing, and developer fee sharing.

## Key Features

- **Tendermint-Style BFT Consensus** -- 3-phase commit (Propose, Prevote, Precommit) with 2/3+ supermajority quorum and linear timeout backoff
- **EIP-1559 Fee Market** -- Dynamic base fee with a 4-way split: 40% burn, 30% validator, 20% treasury, 10% developer rewards
- **Tiered Staking** -- Flat 5% base APY with five lock tiers that increase rewards up to 10% APY and 5x vote weight
- **200 Validator Cap** -- Active set capped at 200 with unlimited standby and epoch-based rotation
- **Validator-Only Slashing** -- Delegators are never slashed; offenses: double-sign (5%), downtime (1%), invalid block (10%)
- **Developer Rewards** -- 10% of all transaction fees flow automatically to contract deployers
- **Tiered Storage** -- RAM LRU cache, NVMe warm store, cold archive
- **Ed25519 Cryptography** -- All keys, signatures, and identities use Ed25519

## Architecture

```
+-------------------------------------------------------------+
|                      Validator Binary                        |
+---------------+--------------+--------------+----------------+
|   Consensus   |   Economics  |   Runtime    |  Integration   |
+---------------+--------------+--------------+----------------+
| BFT State     | Staking      | Validator    | CLI            |
| Machine       | (5% + tiers) | Set (200cap) |                |
+---------------+--------------+--------------+----------------+
| P2P Network   | EIP-1559     | Slashing     | JSON-RPC       |
| (libp2p)      | Fee Market   | (val-only)   | Server         |
+---------------+--------------+--------------+----------------+
|               | Developer    | Tiered       | Genesis        |
|               | Rewards      | Storage      | Config         |
+---------------+--------------+--------------+----------------+
| Mempool       |              | State DB     |                |
+---------------+--------------+--------------+----------------+
```

## Crate Map

| Crate | Path | Description |
|-------|------|-------------|
| `trv1-bft` | `consensus/bft` | Pure BFT consensus state machine (no I/O) |
| `trv1-net` | `consensus/net` | P2P networking via libp2p gossipsub |
| `trv1-staking` | `economics/staking` | Staking pool with 5% base APY + tiered lock bonuses |
| `trv1-fees` | `economics/fees` | EIP-1559 dynamic base fee + 4-way fee split |
| `trv1-rewards` | `economics/rewards` | Developer reward distribution from transaction fees |
| `trv1-validator-set` | `runtime/validator-set` | 200-cap validator set with epoch rotation |
| `trv1-slashing` | `runtime/slashing` | Validator-only slashing with evidence pool |
| `trv1-storage` | `runtime/storage` | 3-tier storage: RAM LRU, NVMe, cold archive |
| `trv1-state` | `runtime/state` | Account state database |
| `trv1-mempool` | `mempool` | Transaction mempool |
| `trv1-genesis` | `genesis` | Genesis configuration with builder pattern |
| `trv1-rpc` | `rpc` | JSON-RPC server (jsonrpsee) |
| `trv1-cli` | `cli` | CLI for key generation, genesis management, queries |
| `trv1-validator` | `validator` | Validator node binary wiring all components together |

## Staking Tiers

| Tier | Lock Period | Bonus APY | Total APY | Reward Multiplier | Vote Weight |
|------|-------------|-----------|-----------|-------------------|-------------|
| NoLock | None (instant unlock) | +0% | 5.00% | 1.0x | 1.0x |
| ThreeMonth | 90 epochs (~90 days) | +1% | 6.00% | 1.2x | 1.5x |
| SixMonth | 180 epochs (~180 days) | +2% | 7.00% | 1.5x | 2.0x |
| OneYear | 365 epochs (~365 days) | +3% | 8.00% | 2.0x | 3.0x |
| Permanent | Forever (cannot unstake) | +5% | 10.00% | 3.0x | 5.0x |

## Building from Source

### Prerequisites

- Rust 1.75 or later
- Linux or macOS
- `cargo` (included with Rust)

### Build

```bash
# Build the entire workspace (debug)
cargo build --workspace

# Build optimized release binaries
cargo build --workspace --release
```

Release binaries are placed in `target/release/`:
- `trv1` -- CLI tool
- `trv1-validator` -- Validator node

### Test

```bash
cargo test --workspace
```

## Quick Start

```bash
# 1. Generate a validator keypair
cargo run --release --bin trv1 -- keygen --output my-validator.key

# 2. Initialize genesis configuration
cargo run --release --bin trv1 -- genesis init \
  --chain-id trv1-testnet-1 \
  --output genesis.json

# 3. Start a validator node
cargo run --release --bin trv1-validator -- \
  --genesis genesis.json \
  --validator-key my-validator.key \
  --rpc-port 9944

# 4. Check node health
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_health","params":[]}' | jq
```

For multi-node testnet setup, see [docs/testnet-guide.md](docs/testnet-guide.md).

For the complete JSON-RPC API, see [docs/rpc-reference.md](docs/rpc-reference.md).

For technical architecture details, see [docs/architecture.md](docs/architecture.md).

## License

MIT
