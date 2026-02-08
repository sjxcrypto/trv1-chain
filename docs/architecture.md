# TRv1 Technical Architecture

This document describes the internal architecture of TRv1, covering consensus, transaction processing, fee economics, staking, and validator management.

## Crate Dependency Graph

```
                        +-------------------+
                        | trv1-validator    |  (binary: wires everything together)
                        +-------------------+
                       / |    |    |    \    \
                      /  |    |    |     \    \
           +---------+ +----+ +------+ +-----+ +--------+
           |trv1-bft | |net | |rpc   | |cli  | |genesis |
           +---------+ +----+ +------+ +-----+ +--------+
                |         |      |         |        |
                |         |      v         |        v
                |         | +--------+     |   +----------+
                |         | |mempool |     |   | types    |
                |         | +--------+     |   | config   |
                |         |      |         |   | builder  |
                v         v      v         v   +----------+
           +----------+ +-------+ +----------+
           | staking  | | fees  | | rewards  |
           +----------+ +-------+ +----------+
                |            |          |
                v            v          v
         +---------------+ +-------------+ +-----------+
         |validator-set  | | slashing    | | storage   |
         +---------------+ +-------------+ +-----------+
                                  |
                                  v
                            +-----------+
                            |  state    |
                            +-----------+
```

The `trv1-validator` binary is the top-level integration point. It imports every other crate and wires them into a single async process that runs consensus, networking, RPC, and state management.

## Consensus: Tendermint-Style BFT

TRv1 uses a 3-phase BFT consensus protocol modeled on Tendermint. The state machine is implemented as a pure, I/O-free module in `consensus/bft/`.

### Phases

```
 [Propose] --> [Prevote] --> [Precommit] --> [Commit]
     |             |              |
     v             v              v
  Timeout       Timeout        Timeout
 (3000ms)      (1000ms)       (1000ms)
     |             |              |
     +--- Round increment (+500ms per round) ---+
```

1. **Propose** -- The designated proposer for the current `(height, round)` broadcasts a `Proposal` containing the block hash. If a proposer is locked on a previous block from a valid round, it re-proposes that block.

2. **Prevote** -- Each validator evaluates the proposal and broadcasts a `Prevote`. A nil prevote is cast if the proposal is invalid or not received before timeout.

3. **Precommit** -- Once 2/3+ prevotes are collected for the same block hash, validators broadcast a `Precommit`. A nil precommit is cast if the 2/3+ threshold was not met.

4. **Commit** -- Once 2/3+ precommits are collected for the same block hash, the block is committed to the chain. The height increments and the process restarts at round 0.

### Key Types (from `consensus/bft/src/types.rs`)

| Type | Description |
|------|-------------|
| `ValidatorId` | Wraps an Ed25519 `VerifyingKey` for validator identity |
| `BlockHash` | 32-byte SHA-256 hash of a block |
| `Height(u64)` | 0-indexed block height |
| `Round(u32)` | Consensus round within a height |
| `Vote` | A prevote or precommit: includes `vote_type`, `height`, `round`, `block_hash`, `validator`, `signature` |
| `Proposal` | A block proposal: includes `height`, `round`, `block_hash`, `proposer`, `signature`, `valid_round` |
| `ConsensusMessage` | Enum: `ProposeBlock`, `CastVote`, `CommitBlock`, `ScheduleTimeout` |

### Timeout Configuration

| Phase | Base Timeout | Per-Round Increment |
|-------|-------------|---------------------|
| Propose | 3000 ms | +500 ms |
| Prevote | 1000 ms | +500 ms |
| Precommit | 1000 ms | +500 ms |

For example, at round 3, the propose timeout is `3000 + (3 * 500) = 4500ms`.

## Transaction Lifecycle

```
  Client                  Node                    Consensus            State
    |                      |                         |                   |
    |--- submitTx -------->|                         |                   |
    |                      |-- validate + add ------>|                   |
    |                      |   to mempool            |                   |
    |<-- tx_hash ----------|                         |                   |
    |                      |                         |                   |
    |                      |   (proposer selects     |                   |
    |                      |    txs from mempool)    |                   |
    |                      |                         |                   |
    |                      |<--- ProposeBlock -------|                   |
    |                      |---- Prevote ----------->|                   |
    |                      |<--- collect 2/3+ -------|                   |
    |                      |---- Precommit --------->|                   |
    |                      |<--- collect 2/3+ -------|                   |
    |                      |                         |                   |
    |                      |<--- CommitBlock --------|                   |
    |                      |                         |                   |
    |                      |--- apply txs -------------------------------->|
    |                      |   update balances,                          |
    |                      |   increment nonces,                         |
    |                      |   collect fees                              |
    |                      |                                             |
```

### Transaction Structure (from `consensus/bft/src/block.rs`)

```rust
pub struct Transaction {
    pub from: [u8; 32],       // sender Ed25519 public key
    pub to: [u8; 32],         // recipient Ed25519 public key
    pub amount: u64,          // transfer amount
    pub nonce: u64,           // sender nonce (monotonically increasing)
    pub signature: Vec<u8>,   // Ed25519 signature (64 bytes)
    pub data: Vec<u8>,        // arbitrary payload
}
```

### Signing Protocol

The signing message is: `SHA-256(from ++ to ++ amount.to_le_bytes() ++ nonce.to_le_bytes() ++ data)`

The sender signs this 32-byte digest with their Ed25519 private key.

### Block Structure (from `consensus/bft/src/block.rs`)

```rust
pub struct BlockHeader {
    pub height: Height,
    pub timestamp: u64,         // Unix seconds
    pub parent_hash: BlockHash,
    pub proposer: ValidatorId,
    pub state_root: [u8; 32],
    pub tx_merkle_root: [u8; 32],
}
```

## Fee Market: EIP-1559

TRv1 implements an EIP-1559 dynamic fee market. The base fee adjusts per block based on gas utilization relative to a target.

### Configuration (from `economics/fees/src/types.rs`)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `target_gas_per_block` | 15,000,000 | Equilibrium gas usage |
| `max_gas_per_block` | 30,000,000 | Maximum gas per block |
| `base_fee_floor` | 1 | Minimum base fee (never goes below this) |
| `elasticity_multiplier` | 8 | Fee adjustment denominator (12.5% max change per block) |

### 4-Way Fee Split

All base fees are split into four destinations:

```
Total Fee
    |
    +--- 40% --> Burn (removed from circulation)
    |
    +--- 30% --> Block Validator (proposer reward)
    |
    +--- 20% --> Protocol Treasury
    |
    +--- 10% --> Developer Rewards (contract deployers)
```

The split ratios are configurable via `SplitConfig` (in basis points, must sum to 10,000):

| Destination | Default (bps) | Default (%) |
|-------------|---------------|-------------|
| Burn | 4000 | 40% |
| Validator | 3000 | 30% |
| Treasury | 2000 | 20% |
| Developer | 1000 | 10% |

Any integer rounding remainder goes to the burn bucket, ensuring the total is always conserved exactly.

## Staking

### Base Economics

- **Base APY:** 5% (500 basis points) -- flat per-coin annual yield
- **1 epoch = 1 day** (365 epochs per year)

### Tiered Lock Bonuses (from `economics/staking/src/tiers.rs`)

Stakers choose a lock tier when staking, trading liquidity for higher rewards:

| Tier | Lock Duration | Bonus APY | Total APY | Reward Multiplier | Vote Weight |
|------|---------------|-----------|-----------|-------------------|-------------|
| NoLock | 0 epochs (instant) | +0% | 5.00% | 1.0x | 1.0x |
| ThreeMonth | 90 epochs | +1% | 6.00% | 1.2x | 1.5x |
| SixMonth | 180 epochs | +2% | 7.00% | 1.5x | 2.0x |
| OneYear | 365 epochs | +3% | 8.00% | 2.0x | 3.0x |
| Permanent | Forever | +5% | 10.00% | 3.0x | 5.0x |

The **reward multiplier** scales staking reward calculations. The **vote weight** multiplier scales voting power in consensus and governance.

### Effective Stake

A validator's effective stake (used for rotation ranking) is:

```
effective_stake = raw_stake * vote_weight_bps / 1000
```

For example, a validator with 10,000,000 raw stake in the OneYear tier has:
```
effective_stake = 10,000,000 * 3000 / 1000 = 30,000,000
```

## Validator Set Management

### Configuration (from `runtime/validator-set/src/types.rs`)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `active_set_cap` | 200 | Maximum active validators at any time |
| `epoch_length` | 100 blocks | Rotation happens at epoch boundaries |
| `min_stake` | 1,000,000 | Minimum stake to register as a validator |

### Validator Statuses

| Status | Description |
|--------|-------------|
| `Active` | Participating in consensus and earning rewards |
| `Standby` | Registered but waiting for a rotation opportunity |
| `Jailed` | Penalized and excluded from consensus and rotation |

### Epoch-Based Rotation

At each epoch boundary (every `epoch_length` blocks), the validator set is re-evaluated. Standby validators with higher effective stake can replace lower-ranked active validators, up to the `active_set_cap`.

## Slashing

TRv1 uses **validator-only slashing** -- delegators are never slashed. Only the validator's own stake is at risk.

### Slashable Offenses (from `runtime/slashing/src/types.rs`)

| Offense | Slash (bps) | Slash (%) | Trigger |
|---------|-------------|-----------|---------|
| DoubleSign | 500 | 5% | Validator signed two different blocks at the same height/round |
| Downtime | 100 | 1% | Validator missed 100+ consecutive blocks |
| InvalidBlock | 1000 | 10% | Validator proposed an invalid block |

### Slashing Flow

1. Evidence (e.g., conflicting votes) is submitted to the `SlashingEngine`
2. The evidence is deduplicated and validated
3. If valid, the offending validator's stake is reduced by the slash percentage
4. The validator is moved to `Jailed` status
5. A `SlashEvent` is recorded with the offender pubkey, offense type, amount, height, and evidence hash

## Networking: libp2p

TRv1 uses **libp2p** for peer-to-peer communication with the following protocols:

- **Transport:** TCP with Noise encryption and Yamux multiplexing
- **Discovery:** Identify protocol for peer information exchange
- **Message Propagation:** Gossipsub for consensus messages and transaction gossip

### Network Messages

Consensus messages (`Proposal`, `Vote`) and transactions are serialized and broadcast over gossipsub topics. Validators subscribe to consensus topics relevant to their current height and round.

## Genesis Configuration

The genesis file is a JSON document that defines the initial chain state.

### Structure (from `genesis/src/config.rs`)

```json
{
  "chain_id": "trv1-testnet-1",
  "genesis_time": "2025-01-01T00:00:00Z",
  "chain_params": {
    "chain_id": "trv1-testnet-1",
    "epoch_length": 100,
    "block_time_ms": 2000,
    "max_validators": 200,
    "base_fee_floor": 1,
    "fee_burn_bps": 4000,
    "fee_validator_bps": 3000,
    "fee_treasury_bps": 2000,
    "fee_developer_bps": 1000,
    "slash_double_sign_bps": 5000,
    "slash_downtime_bps": 100,
    "staking_base_apy": 500
  },
  "validators": [
    {
      "pubkey": "0100000000...64 hex chars",
      "initial_stake": 10000000,
      "commission_rate": 500
    }
  ],
  "accounts": [
    {
      "pubkey": "0100000000...64 hex chars",
      "balance": 100000000
    }
  ],
  "genesis_hash": "aabb...64 hex chars"
}
```

### Genesis Hash

The `genesis_hash` is a SHA-256 hash of the canonical JSON representation of all genesis fields (excluding the hash itself). It provides a unique fingerprint to ensure all nodes are starting from the same state. All validators in a network must agree on the same genesis hash.

### Validation Rules

The genesis configuration is validated on load:

- Must have at least one validator
- No duplicate validator public keys
- All validator stakes must be greater than 0
- Commission rates must be at most 10,000 bps (100%)
- Fee split ratios must sum to exactly 10,000 bps
- Epoch length, block time, and max validators must be greater than 0

## Storage Architecture

TRv1 uses a 3-tier storage architecture:

| Tier | Medium | Purpose |
|------|--------|---------|
| Hot | RAM (LRU cache) | Recently accessed state and blocks |
| Warm | NVMe / SSD | Frequently accessed historical data |
| Cold | Archive storage | Old blocks and state snapshots |

The `trv1-storage` crate manages automatic promotion and demotion of data between tiers based on access patterns.
