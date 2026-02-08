use std::net::SocketAddr;
use std::sync::Arc;

use jsonrpsee::core::RpcResult;
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::types::ErrorObjectOwned;
use parking_lot::RwLock;
use trv1_mempool::TransactionPool;
use trv1_state::StateDB;

use crate::handlers::Trv1ApiServer;
use crate::types::*;

/// Shared state accessible by all RPC handlers.
pub struct RpcState {
    /// Current blockchain height.
    pub current_height: Arc<RwLock<u64>>,
    /// Number of active validators (placeholder -- real node wires in the ValidatorSetManager).
    pub validator_count: Arc<RwLock<usize>>,
    /// Current base fee from the fee market.
    pub base_fee: Arc<RwLock<u64>>,
    /// Transaction mempool.
    pub mempool: Arc<RwLock<TransactionPool>>,
    /// Account state database.
    pub state_db: Arc<RwLock<StateDB>>,
}

impl RpcState {
    /// Create a new RPC state with the given mempool and state database.
    pub fn new(
        mempool: Arc<RwLock<TransactionPool>>,
        state_db: Arc<RwLock<StateDB>>,
    ) -> Self {
        Self {
            current_height: Arc::new(RwLock::new(0)),
            validator_count: Arc::new(RwLock::new(0)),
            base_fee: Arc::new(RwLock::new(1)),
            mempool,
            state_db,
        }
    }

    /// Create a mock RPC state for testing (empty mempool and state).
    pub fn new_mock() -> Self {
        Self::new(
            Arc::new(RwLock::new(TransactionPool::new(
                trv1_mempool::MempoolConfig::default(),
            ))),
            Arc::new(RwLock::new(StateDB::new())),
        )
    }
}

impl Default for RpcState {
    fn default() -> Self {
        Self::new_mock()
    }
}

/// The TRv1 RPC server.
pub struct RpcServer {
    port: u16,
    state: Arc<RpcState>,
}

impl RpcServer {
    /// Create a new RPC server on the given port with shared state.
    pub fn new(port: u16, state: Arc<RpcState>) -> Self {
        Self { port, state }
    }

    /// Start the JSON-RPC HTTP server. Blocks until the server is shut down.
    pub async fn start(self) -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
        let addr: SocketAddr = format!("0.0.0.0:{}", self.port).parse()?;
        let server = ServerBuilder::default().build(addr).await?;

        let rpc_impl = RpcImpl {
            state: self.state.clone(),
        };

        let addr = server.local_addr()?;
        tracing::info!(%addr, "TRv1 RPC server starting");

        let handle = server.start(rpc_impl.into_rpc());
        handle.stopped().await;

        Ok(addr)
    }
}

/// Internal implementation of the RPC trait backed by shared state.
struct RpcImpl {
    state: Arc<RpcState>,
}

impl Trv1ApiServer for RpcImpl {
    fn get_block(&self, height: u64) -> RpcResult<BlockResponse> {
        let current = *self.state.current_height.read();
        Ok(BlockResponse {
            height,
            timestamp: 0,
            parent_hash: "0".repeat(64),
            proposer: "0".repeat(64),
            tx_count: 0,
            block_hash: format!(
                "placeholder_block_hash_at_height_{height}_current_{current}"
            ),
        })
    }

    fn get_latest_block(&self) -> RpcResult<BlockResponse> {
        let height = *self.state.current_height.read();
        self.get_block(height)
    }

    fn get_validators(&self) -> RpcResult<Vec<ValidatorResponse>> {
        let count = *self.state.validator_count.read();
        let validators: Vec<ValidatorResponse> = (0..count)
            .map(|i| ValidatorResponse {
                pubkey: format!("validator_{i}_pubkey"),
                stake: 10_000_000,
                commission_rate: 500,
                status: "Active".to_string(),
                performance_score: 10_000,
            })
            .collect();
        Ok(validators)
    }

    fn get_staking_info(&self, pubkey: String) -> RpcResult<StakingInfoResponse> {
        Ok(StakingInfoResponse {
            pubkey,
            total_staked: 0,
            voting_power: 0,
        })
    }

    fn get_fee_info(&self) -> RpcResult<FeeInfoResponse> {
        let base_fee = *self.state.base_fee.read();
        Ok(FeeInfoResponse {
            base_fee,
            target_gas_per_block: 15_000_000,
            max_gas_per_block: 30_000_000,
        })
    }

    fn health(&self) -> RpcResult<HealthResponse> {
        let height = *self.state.current_height.read();
        let validators = *self.state.validator_count.read();
        Ok(HealthResponse {
            status: "ok".to_string(),
            current_height: height,
            validator_count: validators,
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    fn submit_transaction(&self, req: SubmitTransactionRequest) -> RpcResult<SubmitTransactionResponse> {
        let from: [u8; 32] = hex::decode(&req.from)
            .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid 'from' hex: {e}"), None::<()>))?
            .try_into()
            .map_err(|_| ErrorObjectOwned::owned(-32602, "'from' must be 32 bytes", None::<()>))?;

        let to: [u8; 32] = hex::decode(&req.to)
            .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid 'to' hex: {e}"), None::<()>))?
            .try_into()
            .map_err(|_| ErrorObjectOwned::owned(-32602, "'to' must be 32 bytes", None::<()>))?;

        let signature = hex::decode(&req.signature)
            .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid 'signature' hex: {e}"), None::<()>))?;

        let data = hex::decode(&req.data)
            .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid 'data' hex: {e}"), None::<()>))?;

        let tx = trv1_bft::block::Transaction {
            from,
            to,
            amount: req.amount,
            nonce: req.nonce,
            signature,
            data,
        };

        let tx_hash = hex::encode(trv1_mempool::pool::compute_tx_hash(&tx));

        let mut mempool = self.state.mempool.write();
        match mempool.add_transaction(tx) {
            Ok(()) => Ok(SubmitTransactionResponse {
                tx_hash,
                accepted: true,
            }),
            Err(e) => Err(ErrorObjectOwned::owned(
                -32000,
                format!("transaction rejected: {e}"),
                None::<()>,
            )),
        }
    }

    fn get_account(&self, pubkey: String) -> RpcResult<AccountResponse> {
        let key: [u8; 32] = hex::decode(&pubkey)
            .map_err(|e| ErrorObjectOwned::owned(-32602, format!("invalid pubkey hex: {e}"), None::<()>))?
            .try_into()
            .map_err(|_| ErrorObjectOwned::owned(-32602, "pubkey must be 32 bytes", None::<()>))?;

        let state_db = self.state.state_db.read();
        match state_db.get_account(&key) {
            Some(acct) => Ok(AccountResponse {
                pubkey,
                balance: acct.balance,
                nonce: acct.nonce,
            }),
            None => Ok(AccountResponse {
                pubkey,
                balance: 0,
                nonce: 0,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use trv1_state::AccountState;

    fn mock_rpc() -> RpcImpl {
        RpcImpl {
            state: Arc::new(RpcState::new_mock()),
        }
    }

    #[test]
    fn rpc_state_defaults() {
        let state = RpcState::new_mock();
        assert_eq!(*state.current_height.read(), 0);
        assert_eq!(*state.validator_count.read(), 0);
        assert_eq!(*state.base_fee.read(), 1);
    }

    #[test]
    fn rpc_impl_health() {
        let state = Arc::new(RpcState::new_mock());
        *state.current_height.write() = 42;
        *state.validator_count.write() = 4;

        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.health().unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.current_height, 42);
        assert_eq!(resp.validator_count, 4);
    }

    #[test]
    fn rpc_impl_get_block() {
        let rpc = mock_rpc();
        let resp = rpc.get_block(10).unwrap();
        assert_eq!(resp.height, 10);
    }

    #[test]
    fn rpc_impl_get_latest_block() {
        let state = Arc::new(RpcState::new_mock());
        *state.current_height.write() = 99;
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_latest_block().unwrap();
        assert_eq!(resp.height, 99);
    }

    #[test]
    fn rpc_impl_get_validators() {
        let state = Arc::new(RpcState::new_mock());
        *state.validator_count.write() = 3;
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_validators().unwrap();
        assert_eq!(resp.len(), 3);
    }

    #[test]
    fn rpc_impl_staking_info() {
        let rpc = mock_rpc();
        let resp = rpc.get_staking_info("test_key".to_string()).unwrap();
        assert_eq!(resp.pubkey, "test_key");
    }

    #[test]
    fn rpc_impl_fee_info() {
        let state = Arc::new(RpcState::new_mock());
        *state.base_fee.write() = 100;
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_fee_info().unwrap();
        assert_eq!(resp.base_fee, 100);
        assert_eq!(resp.target_gas_per_block, 15_000_000);
    }

    // --- New endpoint tests ---

    fn make_signed_submit_request(
        signing_key: &SigningKey,
        to: [u8; 32],
        amount: u64,
        nonce: u64,
    ) -> SubmitTransactionRequest {
        let from = signing_key.verifying_key().to_bytes();

        // Build a Transaction to compute signing_message(), then sign it
        let mut tx = trv1_bft::block::Transaction {
            from,
            to,
            amount,
            nonce,
            signature: vec![],
            data: vec![],
        };
        tx.sign(signing_key);

        SubmitTransactionRequest {
            from: hex::encode(from),
            to: hex::encode(to),
            amount,
            nonce,
            signature: hex::encode(&tx.signature),
            data: String::new(),
        }
    }

    #[test]
    fn rpc_submit_transaction_accepted() {
        let rpc = mock_rpc();
        let sk = SigningKey::generate(&mut OsRng);
        let req = make_signed_submit_request(&sk, [2u8; 32], 100, 0);

        let resp = rpc.submit_transaction(req).unwrap();
        assert!(resp.accepted);
        assert!(!resp.tx_hash.is_empty());
    }

    #[test]
    fn rpc_submit_transaction_duplicate_rejected() {
        let rpc = mock_rpc();
        let sk = SigningKey::generate(&mut OsRng);
        let req = make_signed_submit_request(&sk, [2u8; 32], 100, 0);

        let resp1 = rpc.submit_transaction(req.clone()).unwrap();
        assert!(resp1.accepted);

        // Same tx again should be rejected
        let resp2 = rpc.submit_transaction(req);
        assert!(resp2.is_err());
    }

    #[test]
    fn rpc_submit_transaction_bad_hex() {
        let rpc = mock_rpc();
        let req = SubmitTransactionRequest {
            from: "not_valid_hex!".to_string(),
            to: hex::encode([2u8; 32]),
            amount: 100,
            nonce: 0,
            signature: hex::encode([0u8; 64]),
            data: String::new(),
        };

        let resp = rpc.submit_transaction(req);
        assert!(resp.is_err());
    }

    #[test]
    fn rpc_submit_transaction_wrong_length() {
        let rpc = mock_rpc();
        let req = SubmitTransactionRequest {
            from: hex::encode([1u8; 16]), // 16 bytes, not 32
            to: hex::encode([2u8; 32]),
            amount: 100,
            nonce: 0,
            signature: hex::encode([0u8; 64]),
            data: String::new(),
        };

        let resp = rpc.submit_transaction(req);
        assert!(resp.is_err());
    }

    #[test]
    fn rpc_get_account_existing() {
        let state = Arc::new(RpcState::new_mock());
        let pubkey = [0xaau8; 32];

        // Seed the state DB
        {
            let mut db = state.state_db.write();
            db.set_account(pubkey, AccountState::new(5000));
            let acct = db.get_account_mut(&pubkey).unwrap();
            acct.increment_nonce();
            acct.increment_nonce();
        }

        let rpc = RpcImpl {
            state: state.clone(),
        };

        let resp = rpc.get_account(hex::encode(pubkey)).unwrap();
        assert_eq!(resp.balance, 5000);
        assert_eq!(resp.nonce, 2);
        assert_eq!(resp.pubkey, hex::encode(pubkey));
    }

    #[test]
    fn rpc_get_account_nonexistent() {
        let rpc = mock_rpc();
        let pubkey = [0xbb; 32];
        let resp = rpc.get_account(hex::encode(pubkey)).unwrap();
        assert_eq!(resp.balance, 0);
        assert_eq!(resp.nonce, 0);
    }

    #[test]
    fn rpc_get_account_bad_hex() {
        let rpc = mock_rpc();
        let resp = rpc.get_account("not_hex!!".to_string());
        assert!(resp.is_err());
    }

    #[test]
    fn rpc_get_account_wrong_length() {
        let rpc = mock_rpc();
        let resp = rpc.get_account(hex::encode([1u8; 16]));
        assert!(resp.is_err());
    }
}
