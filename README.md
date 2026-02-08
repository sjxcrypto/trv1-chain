# TRv1 Chain — Sovereign L1 Blockchain

A sovereign Layer 1 blockchain built from scratch, designed to use the Solana Virtual Machine (SVM) for execution with its own consensus, economics, and storage layers.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Validator Binary                       │
├──────────────┬──────────────┬──────────────┬─────────────┤
│   Consensus  │   Economics  │   Runtime    │ Integration │
├──────────────┼──────────────┼──────────────┼─────────────┤
│ BFT State    │ Staking      │ Validator    │ CLI         │
│ Machine      │ (5% + tiers) │ Set (200cap) │             │
├──────────────┼──────────────┼──────────────┼─────────────┤
│ P2P Network  │ EIP-1559     │ Slashing     │ JSON-RPC    │
│ (libp2p)     │ Fee Market   │ (val-only)   │ Server      │
├──────────────┼──────────────┼──────────────┼─────────────┤
│              │ Developer    │ Tiered       │ Genesis     │
│              │ Rewards      │ Storage      │ Config      │
└──────────────┴──────────────┴──────────────┴─────────────┘
```

## Key Features

- **Tendermint-Style BFT Consensus** — 3-phase commit (Propose → Prevote → Precommit) with 2/3+ quorum
- **Flat 5% Staking Rewards** — Per-coin annual yield with tiered lock bonuses
- **Tiered Passive Staking** — 5 lock tiers (NoLock → Permanent) with increasing APY and vote weight
- **EIP-1559 Fee Market** — Dynamic base fee with 4-way split (40% burn, 30% validator, 20% treasury, 10% developer)
- **200 Validator Soft Cap** — Active set with unlimited standby and epoch-based rotation
- **Validator-Only Slashing** — Delegators are never slashed; offenses: double-sign (5%), downtime (1%), invalid block (10%)
- **Developer Rewards** — Automatic fee sharing to smart contract deployers
- **Tiered Storage** — RAM LRU cache → NVMe warm store → cold archive

## Crate Overview

| Crate | Description |
|-------|-------------|
| `trv1-bft` | Pure BFT consensus state machine (no I/O) |
| `trv1-net` | P2P networking via libp2p gossipsub |
| `trv1-staking` | Staking pool with flat 5% APY + tiered lock bonuses |
| `trv1-fees` | EIP-1559 dynamic base fee + 4-way fee split |
| `trv1-rewards` | Developer reward distribution from contract fees |
| `trv1-validator-set` | 200-cap validator set with epoch rotation |
| `trv1-slashing` | Validator-only slashing with evidence pool |
| `trv1-storage` | 3-tier storage: RAM LRU → NVMe → cold archive |
| `trv1-genesis` | Genesis configuration with builder pattern |
| `trv1-rpc` | JSON-RPC server (jsonrpsee) |
| `trv1-cli` | Command-line interface for key generation, genesis, and queries |
| `trv1-validator` | Validator node binary wiring all components together |

## Staking Tiers

| Tier | Lock Period | Bonus APY | Total APY | Vote Weight |
|------|-------------|-----------|-----------|-------------|
| NoLock | None | +0% | 5.00% | 1.0x |
| ThreeMonth | 3 months | +1% | 6.00% | 1.5x |
| SixMonth | 6 months | +2% | 7.00% | 2.0x |
| OneYear | 1 year | +3% | 8.00% | 3.0x |
| Permanent | Forever | +5% | 10.00% | 5.0x |

## Building

```bash
cargo build --workspace
```

## Testing

```bash
cargo test --workspace
```

## Quick Start

```bash
# Generate a validator keypair
cargo run --bin trv1 -- keygen --output my-validator.key

# Initialize a genesis file
cargo run --bin trv1 -- genesis init --chain-id trv1-testnet-1 --output genesis.json

# Add a validator to genesis
cargo run --bin trv1 -- genesis add-validator \
  --genesis genesis.json \
  --pubkey <hex-pubkey> \
  --stake 10000000

# View staking tier info
cargo run --bin trv1 -- stake --amount 1000000 --tier OneYear

# Start a validator node
cargo run --bin trv1-validator -- --genesis genesis.json --rpc-port 9944
```

## License

MIT
