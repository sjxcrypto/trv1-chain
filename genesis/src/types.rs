use serde::{Deserialize, Serialize};

/// A validator entry in the genesis configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    /// The validator's Ed25519 public key (32 bytes, hex-encoded in JSON).
    #[serde(with = "hex_serde")]
    pub pubkey: [u8; 32],
    /// Initial stake in the smallest token unit.
    pub initial_stake: u64,
    /// Commission rate in basis points (e.g., 500 = 5%).
    pub commission_rate: u16,
}

/// An account entry in the genesis configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    /// The account's Ed25519 public key.
    #[serde(with = "hex_serde")]
    pub pubkey: [u8; 32],
    /// Initial balance in the smallest token unit.
    pub balance: u64,
}

/// Chain-wide parameters set at genesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainParams {
    /// Unique identifier for this chain (e.g., "trv1-testnet-1").
    pub chain_id: String,
    /// Number of blocks per epoch.
    pub epoch_length: u64,
    /// Target block time in milliseconds.
    pub block_time_ms: u64,
    /// Maximum number of active validators.
    pub max_validators: usize,
    /// Minimum base fee (EIP-1559 floor).
    pub base_fee_floor: u64,
    /// Launch fee split: burn percentage in basis points.
    pub fee_launch_burn_bps: u64,
    /// Launch fee split: validator percentage in basis points.
    pub fee_launch_validator_bps: u64,
    /// Launch fee split: treasury percentage in basis points.
    pub fee_launch_treasury_bps: u64,
    /// Launch fee split: developer percentage in basis points.
    pub fee_launch_developer_bps: u64,
    /// Maturity fee split: burn percentage in basis points.
    pub fee_maturity_burn_bps: u64,
    /// Maturity fee split: validator percentage in basis points.
    pub fee_maturity_validator_bps: u64,
    /// Maturity fee split: treasury percentage in basis points.
    pub fee_maturity_treasury_bps: u64,
    /// Maturity fee split: developer percentage in basis points.
    pub fee_maturity_developer_bps: u64,
    /// Number of epochs to transition from launch to maturity fee split.
    pub fee_transition_epochs: u64,
    /// Slash percentage for double-signing in basis points.
    pub slash_double_sign_bps: u64,
    /// Slash percentage for downtime in basis points.
    pub slash_downtime_bps: u64,
    /// Base staking APY in basis points (500 = 5.00%).
    pub staking_base_apy: u64,
}

impl Default for ChainParams {
    fn default() -> Self {
        Self {
            chain_id: "trv1-devnet".to_string(),
            epoch_length: 100,
            block_time_ms: 2000,
            max_validators: 200,
            base_fee_floor: 1,
            fee_launch_burn_bps: 1000,
            fee_launch_validator_bps: 0,
            fee_launch_treasury_bps: 4500,
            fee_launch_developer_bps: 4500,
            fee_maturity_burn_bps: 2500,
            fee_maturity_validator_bps: 2500,
            fee_maturity_treasury_bps: 2500,
            fee_maturity_developer_bps: 2500,
            fee_transition_epochs: 1825,
            slash_double_sign_bps: 5000,
            slash_downtime_bps: 100,
            staking_base_apy: 500,
        }
    }
}

impl ChainParams {
    /// Validate that both launch and maturity fee split ratios sum to 10,000 bps.
    pub fn validate_fee_split(&self) -> bool {
        let launch_sum = self.fee_launch_burn_bps
            + self.fee_launch_validator_bps
            + self.fee_launch_treasury_bps
            + self.fee_launch_developer_bps;
        let maturity_sum = self.fee_maturity_burn_bps
            + self.fee_maturity_validator_bps
            + self.fee_maturity_treasury_bps
            + self.fee_maturity_developer_bps;
        launch_sum == 10_000 && maturity_sum == 10_000
    }
}

/// Genesis configuration error.
#[derive(Debug, thiserror::Error)]
pub enum GenesisError {
    #[error("no validators in genesis configuration")]
    NoValidators,

    #[error("validator at index {index} has zero stake")]
    ZeroStake { index: usize },

    #[error("validator at index {index} has commission > 10000 bps")]
    InvalidCommission { index: usize },

    #[error("fee split ratios must sum to 10000 bps, got {0}")]
    InvalidFeeSplit(u64),

    #[error("epoch length must be > 0")]
    ZeroEpochLength,

    #[error("block time must be > 0")]
    ZeroBlockTime,

    #[error("max validators must be > 0")]
    ZeroMaxValidators,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("duplicate validator pubkey at index {0}")]
    DuplicateValidator(usize),
}

/// Helper module for serializing [u8; 32] as hex strings in JSON.
mod hex_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "expected 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chain_params() {
        let params = ChainParams::default();
        assert_eq!(params.max_validators, 200);
        assert_eq!(params.staking_base_apy, 500);
        assert!(params.validate_fee_split());
    }

    #[test]
    fn fee_split_validation() {
        let mut params = ChainParams::default();
        assert!(params.validate_fee_split());

        // Breaking launch sum should fail.
        params.fee_launch_burn_bps = 5000;
        assert!(!params.validate_fee_split());

        // Fix launch, break maturity.
        params.fee_launch_burn_bps = 1000;
        params.fee_maturity_burn_bps = 5000;
        assert!(!params.validate_fee_split());
    }

    #[test]
    fn genesis_validator_serde_roundtrip() {
        let val = GenesisValidator {
            pubkey: [42u8; 32],
            initial_stake: 1_000_000,
            commission_rate: 500,
        };
        let json = serde_json::to_string(&val).unwrap();
        let val2: GenesisValidator = serde_json::from_str(&json).unwrap();
        assert_eq!(val.pubkey, val2.pubkey);
        assert_eq!(val.initial_stake, val2.initial_stake);
        assert_eq!(val.commission_rate, val2.commission_rate);
    }

    #[test]
    fn genesis_account_serde_roundtrip() {
        let acct = GenesisAccount {
            pubkey: [7u8; 32],
            balance: 500_000,
        };
        let json = serde_json::to_string(&acct).unwrap();
        let acct2: GenesisAccount = serde_json::from_str(&json).unwrap();
        assert_eq!(acct.pubkey, acct2.pubkey);
        assert_eq!(acct.balance, acct2.balance);
    }

    #[test]
    fn chain_params_serde_roundtrip() {
        let params = ChainParams::default();
        let json = serde_json::to_string(&params).unwrap();
        let params2: ChainParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params.chain_id, params2.chain_id);
        assert_eq!(params.max_validators, params2.max_validators);
        assert_eq!(params.staking_base_apy, params2.staking_base_apy);
    }
}
