#!/bin/bash
#
# launch-testnet.sh -- Launch a local N-validator TRv1 testnet.
#
# Usage:
#   ./scripts/launch-testnet.sh [num_validators] [base_port]
#
# Arguments:
#   num_validators  Number of validators to launch (default: 4)
#   base_port       Base P2P listen port (default: 30333). Each validator
#                   gets base_port+i.  RPC ports start at 9944+i.
#
# The script creates everything under /tmp/trv1-testnet/:
#   validator-{i}.key   -- ed25519 secret key (hex)
#   genesis.json        -- shared genesis config
#   data-{i}/           -- per-validator data directory
#   stop.sh             -- kills all running validators
#   pids.txt            -- list of background PIDs
#
set -euo pipefail

NUM_VALIDATORS="${1:-4}"
BASE_PORT="${2:-30333}"
BASE_RPC_PORT=9944
TESTNET_DIR="/tmp/trv1-testnet"
CHAIN_ID="trv1-local-testnet"

echo "=== TRv1 Local Testnet Launcher ==="
echo "  Validators : ${NUM_VALIDATORS}"
echo "  Base P2P   : ${BASE_PORT}"
echo "  Base RPC   : ${BASE_RPC_PORT}"
echo "  Workdir    : ${TESTNET_DIR}"
echo ""

# -- Cleanup previous run --
if [ -f "${TESTNET_DIR}/stop.sh" ]; then
    echo "Stopping previous testnet..."
    bash "${TESTNET_DIR}/stop.sh" 2>/dev/null || true
fi
rm -rf "${TESTNET_DIR}"
mkdir -p "${TESTNET_DIR}"

# -- Build the workspace --
echo "Building workspace (release)..."
cargo build --release 2>&1 | tail -3
echo ""

TRV1_CLI="cargo run --release --bin trv1 --"
TRV1_VALIDATOR="cargo run --release --bin trv1-validator --"

# -- Generate keypairs --
declare -a PUBKEYS
for i in $(seq 0 $(( NUM_VALIDATORS - 1 ))); do
    KEY_FILE="${TESTNET_DIR}/validator-${i}.key"
    echo "Generating keypair for validator ${i}..."
    KEYGEN_OUTPUT=$($TRV1_CLI keygen --output "${KEY_FILE}" 2>&1)
    # Extract public key from output ("  Public key: <hex>")
    PUBKEY=$(echo "${KEYGEN_OUTPUT}" | grep "Public key:" | awk '{print $NF}')
    PUBKEYS+=("${PUBKEY}")
    echo "  Validator ${i}: pubkey=${PUBKEY}"
done
echo ""

# -- Create genesis --
echo "Initializing genesis..."
$TRV1_CLI genesis init --chain-id "${CHAIN_ID}" --output "${TESTNET_DIR}/genesis.json" 2>&1
echo ""

# -- Add our generated validators to the genesis --
for i in $(seq 0 $(( NUM_VALIDATORS - 1 ))); do
    echo "Adding validator ${i} to genesis (pubkey=${PUBKEYS[$i]})..."
    $TRV1_CLI genesis add-validator \
        --genesis "${TESTNET_DIR}/genesis.json" \
        --pubkey "${PUBKEYS[$i]}" \
        --stake 10000000 2>&1
done
echo ""

# -- Build peer address list --
declare -a PEER_ADDRS
for i in $(seq 0 $(( NUM_VALIDATORS - 1 ))); do
    PORT=$(( BASE_PORT + i ))
    PEER_ADDRS+=("/ip4/127.0.0.1/tcp/${PORT}")
done

# -- Launch validators --
declare -a PIDS
for i in $(seq 0 $(( NUM_VALIDATORS - 1 ))); do
    PORT=$(( BASE_PORT + i ))
    RPC_PORT=$(( BASE_RPC_PORT + i ))
    DATA_DIR="${TESTNET_DIR}/data-${i}"
    KEY_FILE="${TESTNET_DIR}/validator-${i}.key"
    LOG_FILE="${TESTNET_DIR}/validator-${i}.log"
    mkdir -p "${DATA_DIR}"

    # Build peers list: all peers except self
    PEERS=""
    for j in $(seq 0 $(( NUM_VALIDATORS - 1 ))); do
        if [ "$j" != "$i" ]; then
            if [ -n "${PEERS}" ]; then
                PEERS="${PEERS},"
            fi
            PEERS="${PEERS}${PEER_ADDRS[$j]}"
        fi
    done

    echo "Starting validator ${i} (P2P=:${PORT}, RPC=:${RPC_PORT})..."
    RUST_LOG=info $TRV1_VALIDATOR \
        --genesis "${TESTNET_DIR}/genesis.json" \
        --data-dir "${DATA_DIR}" \
        --listen "/ip4/127.0.0.1/tcp/${PORT}" \
        --rpc-port "${RPC_PORT}" \
        --validator-key "${KEY_FILE}" \
        --peers "${PEERS}" \
        > "${LOG_FILE}" 2>&1 &

    PID=$!
    PIDS+=("${PID}")
    echo "  PID: ${PID}, log: ${LOG_FILE}"
done
echo ""

# -- Write PID file --
printf "%s\n" "${PIDS[@]}" > "${TESTNET_DIR}/pids.txt"

# -- Create stop script --
cat > "${TESTNET_DIR}/stop.sh" << 'STOP_EOF'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -f "${SCRIPT_DIR}/pids.txt" ]; then
    while IFS= read -r pid; do
        if kill -0 "$pid" 2>/dev/null; then
            echo "Stopping PID ${pid}..."
            kill "$pid" 2>/dev/null || true
        fi
    done < "${SCRIPT_DIR}/pids.txt"
    rm -f "${SCRIPT_DIR}/pids.txt"
    echo "Testnet stopped."
else
    echo "No pids.txt found -- testnet may not be running."
fi
STOP_EOF
chmod +x "${TESTNET_DIR}/stop.sh"

echo "=== Testnet Running ==="
echo ""
echo "  Validators: ${NUM_VALIDATORS}"
echo "  PIDs: ${PIDS[*]}"
echo ""
echo "  RPC endpoints:"
for i in $(seq 0 $(( NUM_VALIDATORS - 1 ))); do
    RPC_PORT=$(( BASE_RPC_PORT + i ))
    echo "    Validator ${i}: http://127.0.0.1:${RPC_PORT}"
done
echo ""
echo "  Logs: ${TESTNET_DIR}/validator-{0..$(( NUM_VALIDATORS - 1 ))}.log"
echo "  Stop: bash ${TESTNET_DIR}/stop.sh"
echo ""
echo "Send a test transaction:"
echo "  ./scripts/send-tx.sh ${BASE_RPC_PORT} <from_hex> <to_hex> <amount> <nonce>"
