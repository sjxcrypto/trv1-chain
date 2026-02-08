#!/bin/bash
#
# send-tx.sh -- Send a test transaction to a TRv1 node via JSON-RPC.
#
# Usage:
#   ./scripts/send-tx.sh [rpc_port] [from_hex] [to_hex] [amount] [nonce]
#
# Arguments:
#   rpc_port  RPC port of the target node (default: 9944)
#   from_hex  Sender public key, 64 hex characters (default: 32 zero bytes)
#   to_hex    Recipient public key, 64 hex characters (default: 32 0x02 bytes)
#   amount    Transfer amount (default: 100)
#   nonce     Sender nonce (default: 0)
#
# The signature and data fields are set to dummy values.
# For real usage, transactions should be properly signed with ed25519.
#
set -euo pipefail

RPC_PORT="${1:-9944}"
FROM="${2:-$(printf '%064d' 1)}"
TO="${3:-$(printf '02%.0s' {1..32})}"
AMOUNT="${4:-100}"
NONCE="${5:-0}"

# Dummy 64-byte signature (128 hex chars)
SIGNATURE=$(printf '00%.0s' {1..128})
DATA=""

RPC_URL="http://127.0.0.1:${RPC_PORT}"

echo "Sending transaction to ${RPC_URL}..."
echo "  From   : ${FROM}"
echo "  To     : ${TO}"
echo "  Amount : ${AMOUNT}"
echo "  Nonce  : ${NONCE}"
echo ""

RESPONSE=$(curl -s -X POST "${RPC_URL}" \
    -H "Content-Type: application/json" \
    -d "{
        \"jsonrpc\": \"2.0\",
        \"id\": 1,
        \"method\": \"trv1_submitTransaction\",
        \"params\": [{
            \"from\": \"${FROM}\",
            \"to\": \"${TO}\",
            \"amount\": ${AMOUNT},
            \"nonce\": ${NONCE},
            \"signature\": \"${SIGNATURE}\",
            \"data\": \"${DATA}\"
        }]
    }")

echo "Response:"
echo "${RESPONSE}" | python3 -m json.tool 2>/dev/null || echo "${RESPONSE}"
