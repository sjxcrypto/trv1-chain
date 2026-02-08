use serde::{Deserialize, Serialize};

/// Response for a block query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockResponse {
    pub height: u64,
    pub timestamp: u64,
    pub parent_hash: String,
    pub proposer: String,
    pub tx_count: usize,
    pub block_hash: String,
}

/// Response for a validator query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorResponse {
    pub pubkey: String,
    pub stake: u64,
    pub commission_rate: u16,
    pub status: String,
    pub performance_score: u16,
}

/// Response for a staking info query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingInfoResponse {
    pub pubkey: String,
    pub total_staked: u64,
    pub voting_power: u64,
}

/// Response for fee market info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeInfoResponse {
    pub base_fee: u64,
    pub target_gas_per_block: u64,
    pub max_gas_per_block: u64,
}

/// Response for health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub current_height: u64,
    pub validator_count: usize,
    pub version: String,
}

/// Request to submit a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitTransactionRequest {
    /// Sender public key as hex string (64 hex chars = 32 bytes).
    pub from: String,
    /// Recipient public key as hex string.
    pub to: String,
    /// Transfer amount.
    pub amount: u64,
    /// Sender nonce.
    pub nonce: u64,
    /// ed25519 signature as hex string (128 hex chars = 64 bytes).
    pub signature: String,
    /// Arbitrary data as hex string.
    pub data: String,
}

/// Response after submitting a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitTransactionResponse {
    /// Transaction hash as hex string.
    pub tx_hash: String,
    /// Whether the transaction was accepted into the mempool.
    pub accepted: bool,
}

/// Response for an account query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountResponse {
    /// Public key as hex string.
    pub pubkey: String,
    /// Account balance.
    pub balance: u64,
    /// Account nonce.
    pub nonce: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_response_serde_roundtrip() {
        let resp = BlockResponse {
            height: 42,
            timestamp: 1700000000,
            parent_hash: "aa".repeat(32),
            proposer: "bb".repeat(32),
            tx_count: 5,
            block_hash: "cc".repeat(32),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let resp2: BlockResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.height, resp2.height);
        assert_eq!(resp.tx_count, resp2.tx_count);
    }

    #[test]
    fn health_response_serde() {
        let resp = HealthResponse {
            status: "ok".to_string(),
            current_height: 100,
            validator_count: 4,
            version: "0.1.0".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let resp2: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.status, resp2.status);
        assert_eq!(resp.current_height, resp2.current_height);
    }

    #[test]
    fn validator_response_serde() {
        let resp = ValidatorResponse {
            pubkey: "aa".repeat(32),
            stake: 1_000_000,
            commission_rate: 500,
            status: "Active".to_string(),
            performance_score: 10_000,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let resp2: ValidatorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.stake, resp2.stake);
        assert_eq!(resp.status, resp2.status);
    }

    #[test]
    fn submit_tx_request_serde() {
        let req = SubmitTransactionRequest {
            from: "aa".repeat(32),
            to: "bb".repeat(32),
            amount: 100,
            nonce: 0,
            signature: "cc".repeat(64),
            data: String::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let req2: SubmitTransactionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.from, req2.from);
        assert_eq!(req.amount, req2.amount);
        assert_eq!(req.signature, req2.signature);
    }

    #[test]
    fn submit_tx_response_serde() {
        let resp = SubmitTransactionResponse {
            tx_hash: "dd".repeat(32),
            accepted: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let resp2: SubmitTransactionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.tx_hash, resp2.tx_hash);
        assert!(resp2.accepted);
    }

    #[test]
    fn account_response_serde() {
        let resp = AccountResponse {
            pubkey: "ee".repeat(32),
            balance: 1_000_000,
            nonce: 5,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let resp2: AccountResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.pubkey, resp2.pubkey);
        assert_eq!(resp.balance, resp2.balance);
        assert_eq!(resp.nonce, resp2.nonce);
    }
}
