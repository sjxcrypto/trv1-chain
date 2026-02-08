use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;

use crate::types::*;

/// The TRv1 JSON-RPC API trait.
///
/// Using jsonrpsee's `#[rpc]` proc macro to generate the server implementation.
/// Each method is prefixed with `trv1_` in the JSON-RPC namespace.
#[rpc(server)]
pub trait Trv1Api {
    /// Get a block at a specific height.
    #[method(name = "trv1_getBlock")]
    fn get_block(&self, height: u64) -> RpcResult<BlockResponse>;

    /// Get the latest (highest) block.
    #[method(name = "trv1_getLatestBlock")]
    fn get_latest_block(&self) -> RpcResult<BlockResponse>;

    /// Get the current active validator set.
    #[method(name = "trv1_getValidators")]
    fn get_validators(&self) -> RpcResult<Vec<ValidatorResponse>>;

    /// Get staking information for a given public key.
    #[method(name = "trv1_getStakingInfo")]
    fn get_staking_info(&self, pubkey: String) -> RpcResult<StakingInfoResponse>;

    /// Get current fee market information.
    #[method(name = "trv1_getFeeInfo")]
    fn get_fee_info(&self) -> RpcResult<FeeInfoResponse>;

    /// Health check endpoint.
    #[method(name = "trv1_health")]
    fn health(&self) -> RpcResult<HealthResponse>;
}
