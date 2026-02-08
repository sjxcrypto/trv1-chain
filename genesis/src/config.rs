use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::types::*;

/// The full genesis configuration for a TRv1 chain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisConfig {
    /// Human-readable chain identifier.
    pub chain_id: String,
    /// Timestamp when the chain starts.
    pub genesis_time: DateTime<Utc>,
    /// Chain-wide parameters.
    pub chain_params: ChainParams,
    /// Initial validator set.
    pub validators: Vec<GenesisValidator>,
    /// Initial account balances.
    pub accounts: Vec<GenesisAccount>,
    /// Hash of the canonical genesis (computed, not stored from file).
    #[serde(with = "hex_bytes")]
    pub genesis_hash: [u8; 32],
}

impl GenesisConfig {
    /// Load a genesis config from a JSON file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, GenesisError> {
        let contents = std::fs::read_to_string(path)?;
        let mut config: GenesisConfig = serde_json::from_str(&contents)?;
        config.genesis_hash = config.compute_genesis_hash();
        Ok(config)
    }

    /// Save the genesis config to a JSON file.
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), GenesisError> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Validate all invariants of the genesis configuration.
    pub fn validate(&self) -> Result<(), GenesisError> {
        // Must have at least one validator.
        if self.validators.is_empty() {
            return Err(GenesisError::NoValidators);
        }

        // Check for duplicate validator pubkeys.
        let mut seen = HashSet::new();
        for (i, v) in self.validators.iter().enumerate() {
            if !seen.insert(v.pubkey) {
                return Err(GenesisError::DuplicateValidator(i));
            }
            if v.initial_stake == 0 {
                return Err(GenesisError::ZeroStake { index: i });
            }
            if v.commission_rate > 10_000 {
                return Err(GenesisError::InvalidCommission { index: i });
            }
        }

        // Validate chain params.
        if self.chain_params.epoch_length == 0 {
            return Err(GenesisError::ZeroEpochLength);
        }
        if self.chain_params.block_time_ms == 0 {
            return Err(GenesisError::ZeroBlockTime);
        }
        if self.chain_params.max_validators == 0 {
            return Err(GenesisError::ZeroMaxValidators);
        }

        let launch_total = self.chain_params.fee_launch_burn_bps
            + self.chain_params.fee_launch_validator_bps
            + self.chain_params.fee_launch_treasury_bps
            + self.chain_params.fee_launch_developer_bps;
        if launch_total != 10_000 {
            return Err(GenesisError::InvalidFeeSplit(launch_total));
        }
        let maturity_total = self.chain_params.fee_maturity_burn_bps
            + self.chain_params.fee_maturity_validator_bps
            + self.chain_params.fee_maturity_treasury_bps
            + self.chain_params.fee_maturity_developer_bps;
        if maturity_total != 10_000 {
            return Err(GenesisError::InvalidFeeSplit(maturity_total));
        }

        Ok(())
    }

    /// Compute a SHA-256 hash of the canonical JSON representation.
    /// This provides a unique fingerprint for the genesis state.
    pub fn compute_genesis_hash(&self) -> [u8; 32] {
        // Create a canonical representation without the hash field itself.
        let canonical = CanonicalGenesis {
            chain_id: &self.chain_id,
            genesis_time: &self.genesis_time,
            chain_params: &self.chain_params,
            validators: &self.validators,
            accounts: &self.accounts,
        };
        let json = serde_json::to_string(&canonical).expect("genesis serialization should not fail");
        let digest = Sha256::digest(json.as_bytes());
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        hash
    }

    /// Create a default testnet configuration with 4 validators.
    pub fn default_testnet() -> Self {
        let validators: Vec<GenesisValidator> = (1..=4)
            .map(|i| {
                let mut pubkey = [0u8; 32];
                pubkey[0] = i;
                GenesisValidator {
                    pubkey,
                    initial_stake: 10_000_000,
                    commission_rate: 500,
                }
            })
            .collect();

        let accounts: Vec<GenesisAccount> = (1..=4)
            .map(|i| {
                let mut pubkey = [0u8; 32];
                pubkey[0] = i;
                GenesisAccount {
                    pubkey,
                    balance: 100_000_000,
                }
            })
            .collect();

        let mut config = GenesisConfig {
            chain_id: "trv1-testnet-1".to_string(),
            genesis_time: Utc::now(),
            chain_params: ChainParams {
                chain_id: "trv1-testnet-1".to_string(),
                ..ChainParams::default()
            },
            validators,
            accounts,
            genesis_hash: [0u8; 32],
        };
        config.genesis_hash = config.compute_genesis_hash();
        config
    }
}

/// Internal type for canonical hashing (excludes genesis_hash field).
#[derive(serde::Serialize)]
struct CanonicalGenesis<'a> {
    chain_id: &'a str,
    genesis_time: &'a DateTime<Utc>,
    chain_params: &'a ChainParams,
    validators: &'a [GenesisValidator],
    accounts: &'a [GenesisAccount],
}

/// Helper module for serializing [u8; 32] as hex in the top-level genesis hash.
mod hex_bytes {
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
    use std::env;

    #[test]
    fn default_testnet_is_valid() {
        let config = GenesisConfig::default_testnet();
        config.validate().unwrap();
        assert_eq!(config.validators.len(), 4);
        assert_eq!(config.accounts.len(), 4);
        assert_eq!(config.chain_id, "trv1-testnet-1");
    }

    #[test]
    fn genesis_hash_is_deterministic() {
        let mut c1 = GenesisConfig::default_testnet();
        let mut c2 = c1.clone();
        // Reset time to same value for deterministic test.
        let fixed_time = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        c1.genesis_time = fixed_time;
        c2.genesis_time = fixed_time;

        let h1 = c1.compute_genesis_hash();
        let h2 = c2.compute_genesis_hash();
        assert_eq!(h1, h2);
        assert_ne!(h1, [0u8; 32]);
    }

    #[test]
    fn genesis_hash_changes_with_data() {
        let mut c1 = GenesisConfig::default_testnet();
        let fixed_time = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        c1.genesis_time = fixed_time;

        let h1 = c1.compute_genesis_hash();

        let mut c2 = c1.clone();
        c2.chain_id = "different-chain".to_string();
        let h2 = c2.compute_genesis_hash();
        assert_ne!(h1, h2);
    }

    #[test]
    fn validate_no_validators_fails() {
        let mut config = GenesisConfig::default_testnet();
        config.validators.clear();
        assert!(matches!(config.validate(), Err(GenesisError::NoValidators)));
    }

    #[test]
    fn validate_zero_stake_fails() {
        let mut config = GenesisConfig::default_testnet();
        config.validators[0].initial_stake = 0;
        assert!(matches!(
            config.validate(),
            Err(GenesisError::ZeroStake { index: 0 })
        ));
    }

    #[test]
    fn validate_invalid_commission_fails() {
        let mut config = GenesisConfig::default_testnet();
        config.validators[1].commission_rate = 10_001;
        assert!(matches!(
            config.validate(),
            Err(GenesisError::InvalidCommission { index: 1 })
        ));
    }

    #[test]
    fn validate_bad_fee_split_fails() {
        let mut config = GenesisConfig::default_testnet();
        config.chain_params.fee_launch_burn_bps = 5000;
        assert!(matches!(
            config.validate(),
            Err(GenesisError::InvalidFeeSplit(_))
        ));
    }

    #[test]
    fn validate_zero_epoch_length_fails() {
        let mut config = GenesisConfig::default_testnet();
        config.chain_params.epoch_length = 0;
        assert!(matches!(
            config.validate(),
            Err(GenesisError::ZeroEpochLength)
        ));
    }

    #[test]
    fn validate_duplicate_validators_fails() {
        let mut config = GenesisConfig::default_testnet();
        config.validators[1].pubkey = config.validators[0].pubkey;
        assert!(matches!(
            config.validate(),
            Err(GenesisError::DuplicateValidator(1))
        ));
    }

    #[test]
    fn serde_roundtrip() {
        let config = GenesisConfig::default_testnet();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let config2: GenesisConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.chain_id, config2.chain_id);
        assert_eq!(config.validators.len(), config2.validators.len());
        assert_eq!(config.genesis_hash, config2.genesis_hash);
    }

    #[test]
    fn file_roundtrip() {
        let config = GenesisConfig::default_testnet();
        let dir = env::temp_dir().join(format!(
            "trv1_genesis_test_{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("genesis.json");

        config.to_file(&path).unwrap();
        let loaded = GenesisConfig::from_file(&path).unwrap();

        assert_eq!(config.chain_id, loaded.chain_id);
        assert_eq!(config.validators.len(), loaded.validators.len());
        assert_eq!(
            config.compute_genesis_hash(),
            loaded.compute_genesis_hash()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
