# TRv1 JSON-RPC API Reference

TRv1 exposes a JSON-RPC 2.0 API over HTTP. The default RPC port is **9944**. All requests use `POST` with `Content-Type: application/json`.

## Base URL

```
http://localhost:9944
```

## Request Format

All requests follow the JSON-RPC 2.0 specification:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "<method_name>",
  "params": [<params>]
}
```

---

## Methods

| Method | Description |
|--------|-------------|
| [`trv1_health`](#trv1_health) | Node health check |
| [`trv1_getBlock`](#trv1_getblock) | Get a block at a specific height |
| [`trv1_getLatestBlock`](#trv1_getlatestblock) | Get the most recent block |
| [`trv1_getValidators`](#trv1_getvalidators) | Get the active validator set |
| [`trv1_getStakingInfo`](#trv1_getstakinginfo) | Get staking info for a public key |
| [`trv1_getFeeInfo`](#trv1_getfeeinfo) | Get current fee market parameters |
| [`trv1_submitTransaction`](#trv1_submittransaction) | Submit a signed transaction |
| [`trv1_getAccount`](#trv1_getaccount) | Get account balance and nonce |

---

### `trv1_health`

Returns the node health status, current block height, validator count, and software version.

**Parameters:** None

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_health","params":[]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "status": "ok",
    "current_height": 142,
    "validator_count": 4,
    "version": "0.1.0"
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Node status, always `"ok"` when the RPC is reachable |
| `current_height` | integer | Latest committed block height |
| `validator_count` | integer | Number of validators in the active set |
| `version` | string | Software version from Cargo.toml |

---

### `trv1_getBlock`

Returns block data at a specific height.

**Parameters:**

| Position | Type | Description |
|----------|------|-------------|
| 0 | integer | Block height to query |

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getBlock","params":[5]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "height": 5,
    "timestamp": 1700000010,
    "parent_hash": "aabbccdd...64 hex chars",
    "proposer": "11223344...64 hex chars",
    "tx_count": 3,
    "block_hash": "eeff0011...64 hex chars"
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `height` | integer | Block height |
| `timestamp` | integer | Unix timestamp in seconds |
| `parent_hash` | string | SHA-256 hash of the parent block (64 hex chars) |
| `proposer` | string | Ed25519 public key of the block proposer (64 hex chars) |
| `tx_count` | integer | Number of transactions in the block |
| `block_hash` | string | SHA-256 hash of this block (64 hex chars) |

**Errors:**

| Code | Message | When |
|------|---------|------|
| -32001 | `block at height N not yet committed` | Requested height exceeds the current chain height |

---

### `trv1_getLatestBlock`

Returns the most recently committed block.

**Parameters:** None

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getLatestBlock","params":[]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "height": 142,
    "timestamp": 1700000284,
    "parent_hash": "aabbccdd...64 hex chars",
    "proposer": "11223344...64 hex chars",
    "tx_count": 1,
    "block_hash": "eeff0011...64 hex chars"
  }
}
```

Response fields are identical to [`trv1_getBlock`](#trv1_getblock).

---

### `trv1_getValidators`

Returns the current active validator set with stake, commission, and performance info.

**Parameters:** None

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getValidators","params":[]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "pubkey": "3b6a27bc...64 hex chars",
      "stake": 10000000,
      "commission_rate": 500,
      "status": "Active",
      "performance_score": 10000
    },
    {
      "pubkey": "9d61b19d...64 hex chars",
      "stake": 10000000,
      "commission_rate": 500,
      "status": "Active",
      "performance_score": 10000
    }
  ]
}
```

**Response Fields (per validator):**

| Field | Type | Description |
|-------|------|-------------|
| `pubkey` | string | Ed25519 public key (64 hex chars) |
| `stake` | integer | Total staked amount in smallest token unit |
| `commission_rate` | integer | Commission rate in basis points (500 = 5%) |
| `status` | string | `"Active"`, `"Standby"`, or `"Jailed"` |
| `performance_score` | integer | Performance score (0-10000 bps; 10000 = perfect) |

---

### `trv1_getStakingInfo`

Returns staking information for a specific public key.

**Parameters:**

| Position | Type | Description |
|----------|------|-------------|
| 0 | string | Public key as hex string (64 characters) |

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getStakingInfo","params":["3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29"]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "pubkey": "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
    "total_staked": 10000000,
    "voting_power": 10000000
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `pubkey` | string | The queried public key |
| `total_staked` | integer | Total staked amount |
| `voting_power` | integer | Effective voting power (adjusted by tier vote weight) |

---

### `trv1_getFeeInfo`

Returns the current fee market parameters.

**Parameters:** None

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getFeeInfo","params":[]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "base_fee": 1,
    "target_gas_per_block": 15000000,
    "max_gas_per_block": 30000000
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `base_fee` | integer | Current EIP-1559 base fee (adjusts per block) |
| `target_gas_per_block` | integer | Target gas usage per block (equilibrium point) |
| `max_gas_per_block` | integer | Maximum gas allowed per block |

---

### `trv1_submitTransaction`

Submit a signed transaction to the mempool.

**Parameters:**

| Position | Type | Description |
|----------|------|-------------|
| 0 | object | Transaction object (see below) |

**Transaction Object Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `from` | string | Yes | Sender Ed25519 public key (64 hex chars = 32 bytes) |
| `to` | string | Yes | Recipient Ed25519 public key (64 hex chars = 32 bytes) |
| `amount` | integer | Yes | Transfer amount in smallest token unit |
| `nonce` | integer | Yes | Sender nonce (starts at 0, increments per transaction) |
| `signature` | string | Yes | Ed25519 signature (128 hex chars = 64 bytes) |
| `data` | string | Yes | Arbitrary data as hex string (use `""` for empty) |

### Transaction Signing Protocol

The signature is computed as follows:

1. Build the signing message by computing `SHA-256(from ++ to ++ amount_le ++ nonce_le ++ data)`:
   - `from` -- 32 bytes (raw public key)
   - `to` -- 32 bytes (raw public key)
   - `amount_le` -- 8 bytes (amount as little-endian u64)
   - `nonce_le` -- 8 bytes (nonce as little-endian u64)
   - `data` -- variable length raw bytes

2. Sign the 32-byte SHA-256 digest with the sender's Ed25519 private key

3. The resulting 64-byte Ed25519 signature is hex-encoded for the `signature` field

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "trv1_submitTransaction",
    "params": [{
      "from": "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
      "to": "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
      "amount": 1000,
      "nonce": 0,
      "signature": "aabb...128 hex chars",
      "data": ""
    }]
  }' | jq
```

**Response (accepted):**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tx_hash": "eeff0011...64 hex chars",
    "accepted": true
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `tx_hash` | string | SHA-256 hash of the transaction (64 hex chars) |
| `accepted` | boolean | Whether the transaction was accepted into the mempool |

**Errors:**

| Code | Message | When |
|------|---------|------|
| -32602 | `invalid 'from' hex: ...` | The `from` field is not valid hex |
| -32602 | `'from' must be 32 bytes` | The `from` field is not exactly 32 bytes |
| -32602 | `invalid 'to' hex: ...` | The `to` field is not valid hex |
| -32602 | `'to' must be 32 bytes` | The `to` field is not exactly 32 bytes |
| -32602 | `invalid 'signature' hex: ...` | The `signature` field is not valid hex |
| -32602 | `invalid 'data' hex: ...` | The `data` field is not valid hex |
| -32000 | `transaction rejected: ...` | Mempool rejected the transaction (e.g., duplicate, invalid signature) |

---

### `trv1_getAccount`

Returns the balance and nonce for an account.

**Parameters:**

| Position | Type | Description |
|----------|------|-------------|
| 0 | string | Public key as hex string (64 characters) |

**Request:**

```bash
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"trv1_getAccount","params":["3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29"]}' | jq
```

**Response:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "pubkey": "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
    "balance": 100000000,
    "nonce": 0
  }
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `pubkey` | string | The queried public key |
| `balance` | integer | Account balance in smallest token unit |
| `nonce` | integer | Current nonce (number of confirmed transactions from this account) |

If the account does not exist on chain, the response returns `balance: 0` and `nonce: 0`.

**Errors:**

| Code | Message | When |
|------|---------|------|
| -32602 | `invalid pubkey hex: ...` | The pubkey is not valid hex |
| -32602 | `pubkey must be 32 bytes` | The pubkey is not exactly 32 bytes |

---

## Error Codes Summary

| Code | Meaning |
|------|---------|
| -32700 | Parse error (malformed JSON) |
| -32600 | Invalid request (missing required JSON-RPC fields) |
| -32601 | Method not found |
| -32602 | Invalid params (wrong type, invalid hex, wrong byte length) |
| -32000 | Transaction rejected by mempool |
| -32001 | Block not yet committed at requested height |
