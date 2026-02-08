use chrono::{DateTime, Utc};

use crate::config::GenesisConfig;
use crate::types::*;

/// Builder for constructing a `GenesisConfig` step by step.
pub struct GenesisBuilder {
    chain_id: String,
    genesis_time: DateTime<Utc>,
    chain_params: ChainParams,
    validators: Vec<GenesisValidator>,
    accounts: Vec<GenesisAccount>,
}

impl GenesisBuilder {
    /// Start building a genesis config with the given chain ID.
    pub fn new(chain_id: impl Into<String>) -> Self {
        let chain_id = chain_id.into();
        Self {
            chain_params: ChainParams {
                chain_id: chain_id.clone(),
                ..ChainParams::default()
            },
            chain_id,
            genesis_time: Utc::now(),
            validators: Vec::new(),
            accounts: Vec::new(),
        }
    }

    /// Set the genesis timestamp.
    pub fn with_genesis_time(mut self, time: DateTime<Utc>) -> Self {
        self.genesis_time = time;
        self
    }

    /// Add a validator to the genesis set.
    pub fn with_validator(
        mut self,
        pubkey: [u8; 32],
        stake: u64,
        commission_rate: u16,
    ) -> Self {
        self.validators.push(GenesisValidator {
            pubkey,
            initial_stake: stake,
            commission_rate,
        });
        self
    }

    /// Add an account with an initial balance.
    pub fn with_account(mut self, pubkey: [u8; 32], balance: u64) -> Self {
        self.accounts.push(GenesisAccount { pubkey, balance });
        self
    }

    /// Set the chain parameters.
    pub fn with_params(mut self, params: ChainParams) -> Self {
        self.chain_params = params;
        self
    }

    /// Build the final genesis configuration.
    /// Validates all invariants before returning.
    pub fn build(self) -> Result<GenesisConfig, GenesisError> {
        let mut config = GenesisConfig {
            chain_id: self.chain_id,
            genesis_time: self.genesis_time,
            chain_params: self.chain_params,
            validators: self.validators,
            accounts: self.accounts,
            genesis_hash: [0u8; 32],
        };

        config.validate()?;
        config.genesis_hash = config.compute_genesis_hash();
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey(n: u8) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[0] = n;
        k
    }

    #[test]
    fn builder_basic() {
        let config = GenesisBuilder::new("test-chain")
            .with_validator(test_pubkey(1), 10_000_000, 500)
            .with_account(test_pubkey(1), 100_000_000)
            .build()
            .unwrap();

        assert_eq!(config.chain_id, "test-chain");
        assert_eq!(config.validators.len(), 1);
        assert_eq!(config.accounts.len(), 1);
        assert_ne!(config.genesis_hash, [0u8; 32]);
    }

    #[test]
    fn builder_multiple_validators() {
        let config = GenesisBuilder::new("multi-val")
            .with_validator(test_pubkey(1), 5_000_000, 300)
            .with_validator(test_pubkey(2), 10_000_000, 500)
            .with_validator(test_pubkey(3), 15_000_000, 700)
            .build()
            .unwrap();

        assert_eq!(config.validators.len(), 3);
        config.validate().unwrap();
    }

    #[test]
    fn builder_no_validators_fails() {
        let result = GenesisBuilder::new("empty").build();
        assert!(matches!(result, Err(GenesisError::NoValidators)));
    }

    #[test]
    fn builder_with_custom_params() {
        let params = ChainParams {
            chain_id: "custom".to_string(),
            epoch_length: 50,
            block_time_ms: 1000,
            max_validators: 100,
            ..ChainParams::default()
        };

        let config = GenesisBuilder::new("custom")
            .with_params(params)
            .with_validator(test_pubkey(1), 10_000_000, 500)
            .build()
            .unwrap();

        assert_eq!(config.chain_params.epoch_length, 50);
        assert_eq!(config.chain_params.block_time_ms, 1000);
        assert_eq!(config.chain_params.max_validators, 100);
    }

    #[test]
    fn builder_with_genesis_time() {
        let fixed_time = DateTime::parse_from_rfc3339("2025-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let config = GenesisBuilder::new("timed")
            .with_genesis_time(fixed_time)
            .with_validator(test_pubkey(1), 10_000_000, 500)
            .build()
            .unwrap();

        assert_eq!(config.genesis_time, fixed_time);
    }

    #[test]
    fn builder_validates_on_build() {
        // Zero stake should fail validation.
        let result = GenesisBuilder::new("bad")
            .with_validator(test_pubkey(1), 0, 500)
            .build();
        assert!(matches!(result, Err(GenesisError::ZeroStake { .. })));
    }

    #[test]
    fn builder_hash_computed_on_build() {
        let config = GenesisBuilder::new("hashed")
            .with_validator(test_pubkey(1), 10_000_000, 500)
            .build()
            .unwrap();

        // The genesis hash should match a fresh computation.
        assert_eq!(config.genesis_hash, config.compute_genesis_hash());
    }
}
