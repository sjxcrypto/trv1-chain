use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque storage key — wraps a byte vector.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey(pub Vec<u8>);

impl StorageKey {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for StorageKey {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl From<&[u8]> for StorageKey {
    fn from(v: &[u8]) -> Self {
        Self(v.to_vec())
    }
}

impl From<&str> for StorageKey {
    fn from(s: &str) -> Self {
        Self(s.as_bytes().to_vec())
    }
}

impl fmt::Display for StorageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}

/// Opaque storage value — wraps a byte vector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageValue(pub Vec<u8>);

impl StorageValue {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u8>> for StorageValue {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl From<&[u8]> for StorageValue {
    fn from(v: &[u8]) -> Self {
        Self(v.to_vec())
    }
}

impl From<&str> for StorageValue {
    fn from(s: &str) -> Self {
        Self(s.as_bytes().to_vec())
    }
}

/// Configuration for the tiered storage system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Maximum number of entries in the hot LRU cache.
    pub lru_capacity: usize,
    /// Base directory path for the NVMe (warm) tier.
    pub nvme_path: String,
    /// Base directory path for the cold archive tier.
    pub archive_path: String,
    /// Maximum RAM budget in bytes (advisory — used for stats).
    pub max_ram_bytes: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            lru_capacity: 10_000,
            nvme_path: "/tmp/trv1-warm".into(),
            archive_path: "/tmp/trv1-cold".into(),
            max_ram_bytes: 512 * 1024 * 1024, // 512 MiB
        }
    }
}

/// Which tier a piece of data resides in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageTier {
    /// RAM-based LRU cache — fastest.
    Hot,
    /// NVMe / SSD-based file storage — fast reads.
    Warm,
    /// Archival cold storage — historical data.
    Cold,
}

impl fmt::Display for StorageTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageTier::Hot => write!(f, "Hot"),
            StorageTier::Warm => write!(f, "Warm"),
            StorageTier::Cold => write!(f, "Cold"),
        }
    }
}

/// Runtime statistics for the tiered storage system.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StorageStats {
    pub hot_entries: usize,
    pub hot_hits: u64,
    pub hot_misses: u64,
    pub warm_hits: u64,
    pub warm_misses: u64,
    pub cold_hits: u64,
    pub cold_misses: u64,
    pub promotions: u64,
    pub evictions: u64,
}

/// Errors produced by the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Key not found")]
    NotFound,

    #[error("Storage configuration error: {0}")]
    Config(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_key_from_str() {
        let key = StorageKey::from("hello");
        assert_eq!(key.as_bytes(), b"hello");
    }

    #[test]
    fn storage_value_len() {
        let val = StorageValue::new(vec![1, 2, 3]);
        assert_eq!(val.len(), 3);
        assert!(!val.is_empty());
    }

    #[test]
    fn default_config_is_sane() {
        let cfg = StorageConfig::default();
        assert_eq!(cfg.lru_capacity, 10_000);
        assert!(cfg.max_ram_bytes > 0);
    }

    #[test]
    fn tier_display() {
        assert_eq!(StorageTier::Hot.to_string(), "Hot");
        assert_eq!(StorageTier::Warm.to_string(), "Warm");
        assert_eq!(StorageTier::Cold.to_string(), "Cold");
    }

    #[test]
    fn storage_key_display_hex() {
        let key = StorageKey::new(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(key.to_string(), "0xdeadbeef");
    }
}
