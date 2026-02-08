# TRv1 Testnet Setup Guide

This guide walks through setting up a local multi-node TRv1 testnet with four validators.

## Prerequisites

- **Rust 1.75+** -- Install via [rustup](https://rustup.rs/)
- **Linux or macOS**
- **curl** and **jq** (for interacting with the JSON-RPC API)

Verify your Rust installation:

```bash
rustc --version   # should be 1.75.0 or later
cargo --version
```

## 1. Build the Binaries

Clone the repository and build release binaries:

```bash
cd trv1-chain
cargo build --workspace --release
```

This produces two binaries in `target/release/`:

| Binary | Purpose |
|--------|---------|
| `trv1` | CLI tool for key generation, genesis management, and queries |
| `trv1-validator` | Validator node binary |

For convenience, you can add them to your PATH:

```bash
export PATH="$PWD/target/release:$PATH"
```

## 2. Generate Validator Keys

Generate four Ed25519 keypairs, one per validator:

```bash
trv1 keygen --output validator-0.key
trv1 keygen --output validator-1.key
trv1 keygen --output validator-2.key
trv1 keygen --output validator-3.key
```

Each command prints the **public key** (64 hex characters) and saves the **secret key** to the specified file. Record the public keys -- you will need them for genesis configuration.

Example output:

```
Generated new Ed25519 keypair
  Public key: 3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29
  Secret key saved to: validator-0.key
```

## 3. Create Genesis Configuration

### Initialize genesis

```bash
trv1 genesis init \
  --chain-id trv1-testnet-1 \
  --output genesis.json
```

This creates a genesis file with default chain parameters:

| Parameter | Default Value |
|-----------|---------------|
| Epoch length | 100 blocks |
| Block time | 2000 ms |
| Max validators | 200 |
| Base fee floor | 1 |
| Fee split | 40% burn / 30% validator / 20% treasury / 10% developer |
| Staking base APY | 5% (500 bps) |
| Double-sign slash | 5% (500 bps) |
| Downtime slash | 1% (100 bps) |

The default genesis includes 4 placeholder validators and 4 funded accounts. For a custom testnet, you can add your own validators.

### Add custom validators (optional)

If you want to add your generated keys as validators instead of using the defaults:

```bash
trv1 genesis add-validator \
  --genesis genesis.json \
  --pubkey <VALIDATOR_0_PUBKEY> \
  --stake 10000000 \
  --commission 500

trv1 genesis add-validator \
  --genesis genesis.json \
  --pubkey <VALIDATOR_1_PUBKEY> \
  --stake 10000000 \
  --commission 500
```

Parameters:
- `--pubkey` -- Hex-encoded Ed25519 public key (64 characters)
- `--stake` -- Initial stake in smallest token unit
- `--commission` -- Commission rate in basis points (500 = 5%)

## 4. Launch a Single-Node Testnet

Start one validator with default settings:

```bash
trv1-validator \
  --genesis genesis.json \
  --validator-key validator-0.key \
  --data-dir /tmp/trv1-node0 \
  --listen /ip4/0.0.0.0/tcp/30333 \
  --rpc-port 9944
```

Verify it is running:

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_health","params":[]}' | jq
```

Expected output:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "status": "ok",
    "current_height": 0,
    "validator_count": 4,
    "version": "0.1.0"
  }
}
```

## 5. Launch a Multi-Node Testnet (4 Validators)

Open four separate terminal windows (or use `tmux`/`screen`). Each validator uses a distinct P2P port, RPC port, and data directory.

### Port allocation

| Validator | P2P Port | RPC Port | Data Directory |
|-----------|----------|----------|----------------|
| Validator 0 | 30333 | 9944 | /tmp/trv1-node0 |
| Validator 1 | 30334 | 9945 | /tmp/trv1-node1 |
| Validator 2 | 30335 | 9946 | /tmp/trv1-node2 |
| Validator 3 | 30336 | 9947 | /tmp/trv1-node3 |

### Start Validator 0 (seed node)

```bash
trv1-validator \
  --genesis genesis.json \
  --validator-key validator-0.key \
  --data-dir /tmp/trv1-node0 \
  --listen /ip4/0.0.0.0/tcp/30333 \
  --rpc-port 9944
```

When Validator 0 starts, note the **libp2p peer ID** printed in the logs. It will look like:

```
Local peer id: 12D3KooW...
```

Construct the multiaddr for other nodes to connect to:

```
/ip4/127.0.0.1/tcp/30333/p2p/12D3KooW...
```

### Start Validator 1

```bash
trv1-validator \
  --genesis genesis.json \
  --validator-key validator-1.key \
  --data-dir /tmp/trv1-node1 \
  --listen /ip4/0.0.0.0/tcp/30334 \
  --rpc-port 9945 \
  --peers /ip4/127.0.0.1/tcp/30333/p2p/<VALIDATOR_0_PEER_ID>
```

### Start Validator 2

```bash
trv1-validator \
  --genesis genesis.json \
  --validator-key validator-2.key \
  --data-dir /tmp/trv1-node2 \
  --listen /ip4/0.0.0.0/tcp/30335 \
  --rpc-port 9946 \
  --peers /ip4/127.0.0.1/tcp/30333/p2p/<VALIDATOR_0_PEER_ID>
```

### Start Validator 3

```bash
trv1-validator \
  --genesis genesis.json \
  --validator-key validator-3.key \
  --data-dir /tmp/trv1-node3 \
  --listen /ip4/0.0.0.0/tcp/30336 \
  --rpc-port 9947 \
  --peers /ip4/127.0.0.1/tcp/30333/p2p/<VALIDATOR_0_PEER_ID>
```

Replace `<VALIDATOR_0_PEER_ID>` with the actual peer ID from Validator 0's logs.

### Start an observer node (no validator key)

To run a non-validating observer that syncs the chain and serves RPC:

```bash
trv1-validator \
  --genesis genesis.json \
  --data-dir /tmp/trv1-observer \
  --listen /ip4/0.0.0.0/tcp/30337 \
  --rpc-port 9948 \
  --peers /ip4/127.0.0.1/tcp/30333/p2p/<VALIDATOR_0_PEER_ID>
```

Omitting `--validator-key` causes the node to run in observer mode.

## 6. Interacting with the Testnet

### Check node health

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_health","params":[]}' | jq
```

### Get the latest block

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getLatestBlock","params":[]}' | jq
```

### Get a block at a specific height

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getBlock","params":[5]}' | jq
```

### Query the validator set

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getValidators","params":[]}' | jq
```

### Query an account balance

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getAccount","params":["<PUBKEY_HEX>"]}' | jq
```

### Get fee market info

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getFeeInfo","params":[]}' | jq
```

### Check staking info for a validator

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getStakingInfo","params":["<PUBKEY_HEX>"]}' | jq
```

### Submit a transaction

See the [RPC Reference](rpc-reference.md#trv1_submittransaction) for the full transaction signing protocol and submission format.

## 7. Viewing Staking Tier Information

Use the CLI to preview staking reward calculations:

```bash
trv1 stake --amount 1000000 --tier NoLock
trv1 stake --amount 1000000 --tier ThreeMonth
trv1 stake --amount 1000000 --tier SixMonth
trv1 stake --amount 1000000 --tier OneYear
trv1 stake --amount 1000000 --tier Permanent
```

Valid tier names (case-insensitive): `NoLock`, `ThreeMonth` (or `3month`), `SixMonth` (or `6month`), `OneYear` (or `1year`), `Permanent` (or `perm`).

## 8. Stopping the Testnet

Press `Ctrl+C` in each validator terminal to gracefully shut down the nodes. Data is stored in the respective `--data-dir` directories and can be removed to start fresh:

```bash
rm -rf /tmp/trv1-node0 /tmp/trv1-node1 /tmp/trv1-node2 /tmp/trv1-node3
```

## 9. Troubleshooting

### Nodes cannot find each other

- Verify that the `--peers` multiaddr is correct, including the peer ID suffix
- Ensure the seed node (Validator 0) is fully started before launching other validators
- Check that firewall rules allow TCP traffic on ports 30333-30336

### Consensus is not progressing

- BFT consensus requires 2/3+ of voting power to agree. With 4 validators, at least 3 must be online
- Check logs for timeout messages -- if you see repeated `ScheduleTimeout` entries, a validator may be unreachable
- Verify all nodes are using the same `genesis.json` file (genesis hashes must match)

### RPC returns connection refused

- Confirm the node is running and the `--rpc-port` matches your curl command
- The RPC server binds to `0.0.0.0`, so it should be reachable from localhost

### "block at height N not yet committed" error

- The requested block height has not been produced yet. Use `trv1_getLatestBlock` to find the current height

### Validator key errors

- The key file must contain a 64-character hex-encoded Ed25519 secret key (32 bytes)
- Ensure there are no extra whitespace or newline issues in the key file

### Log verbosity

Set the `RUST_LOG` environment variable for detailed logs:

```bash
RUST_LOG=debug trv1-validator --genesis genesis.json --validator-key validator-0.key
```

Useful log filters:

```bash
RUST_LOG=trv1_bft=debug,trv1_net=info    # Consensus debug, networking info
RUST_LOG=trv1_rpc=debug                   # RPC request/response details
RUST_LOG=trv1_mempool=debug               # Transaction pool activity
```
