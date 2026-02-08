use std::net::SocketAddr;
use std::sync::Arc;

use jsonrpsee::core::RpcResult;
use jsonrpsee::server::ServerBuilder;
use parking_lot::RwLock;

use crate::handlers::Trv1ApiServer;
use crate::types::*;

/// Shared state accessible by all RPC handlers.
pub struct RpcState {
    /// Current blockchain height.
    pub current_height: Arc<RwLock<u64>>,
    /// Number of active validators (placeholder — real node wires in the ValidatorSetManager).
    pub validator_count: Arc<RwLock<usize>>,
    /// Current base fee from the fee market.
    pub base_fee: Arc<RwLock<u64>>,
}

impl RpcState {
    /// Create a new RPC state with default values.
    pub fn new() -> Self {
        Self {
            current_height: Arc::new(RwLock::new(0)),
            validator_count: Arc::new(RwLock::new(0)),
            base_fee: Arc::new(RwLock::new(1)),
        }
    }
}

impl Default for RpcState {
    fn default() -> Self {
        Self::new()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_state_defaults() {
        let state = RpcState::new();
        assert_eq!(*state.current_height.read(), 0);
        assert_eq!(*state.validator_count.read(), 0);
        assert_eq!(*state.base_fee.read(), 1);
    }

    #[test]
    fn rpc_impl_health() {
        let state = Arc::new(RpcState::new());
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
        let state = Arc::new(RpcState::new());
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_block(10).unwrap();
        assert_eq!(resp.height, 10);
    }

    #[test]
    fn rpc_impl_get_latest_block() {
        let state = Arc::new(RpcState::new());
        *state.current_height.write() = 99;
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_latest_block().unwrap();
        assert_eq!(resp.height, 99);
    }

    #[test]
    fn rpc_impl_get_validators() {
        let state = Arc::new(RpcState::new());
        *state.validator_count.write() = 3;
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_validators().unwrap();
        assert_eq!(resp.len(), 3);
    }

    #[test]
    fn rpc_impl_staking_info() {
        let state = Arc::new(RpcState::new());
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_staking_info("test_key".to_string()).unwrap();
        assert_eq!(resp.pubkey, "test_key");
    }

    #[test]
    fn rpc_impl_fee_info() {
        let state = Arc::new(RpcState::new());
        *state.base_fee.write() = 100;
        let rpc = RpcImpl {
            state: state.clone(),
        };
        let resp = rpc.get_fee_info().unwrap();
        assert_eq!(resp.base_fee, 100);
        assert_eq!(resp.target_gas_per_block, 15_000_000);
    }
}
