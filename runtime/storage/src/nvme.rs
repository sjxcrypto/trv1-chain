use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::types::{StorageKey, StorageValue};

/// NVMe / SSD-backed key-value store — the "warm" tier.
///
/// Each key is SHA-256 hashed and hex-encoded to produce a safe filename.
/// Values are stored as raw bytes on disk.
pub struct WarmStore {
    base_dir: PathBuf,
}

impl WarmStore {
    /// Open (or create) a warm store at the given directory.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let base_dir = path.as_ref().to_path_buf();
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Retrieve a value by key from disk.
    pub fn get(&self, key: &StorageKey) -> io::Result<Option<StorageValue>> {
        let path = self.key_path(key);
        match fs::read(&path) {
            Ok(data) => Ok(Some(StorageValue(data))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write a key-value pair to disk.
    pub fn put(&self, key: &StorageKey, value: &StorageValue) -> io::Result<()> {
        let path = self.key_path(key);
        fs::write(&path, &value.0)
    }

    /// Remove a key from disk, returning the old value if it existed.
    pub fn remove(&self, key: &StorageKey) -> io::Result<Option<StorageValue>> {
        let path = self.key_path(key);
        match fs::read(&path) {
            Ok(data) => {
                fs::remove_file(&path)?;
                Ok(Some(StorageValue(data)))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Check if a key exists on disk without reading the value.
    pub fn contains(&self, key: &StorageKey) -> bool {
        self.key_path(key).exists()
    }

    /// Compute the filesystem path for a given key.
    fn key_path(&self, key: &StorageKey) -> PathBuf {
        let hash = Self::hash_key(key);
        // Use first 2 hex chars as subdirectory for sharding.
        let (shard, rest) = hash.split_at(2);
        let shard_dir = self.base_dir.join(shard);
        // Lazily create the shard directory — ignore errors (checked on put).
        let _ = fs::create_dir_all(&shard_dir);
        shard_dir.join(rest)
    }

    /// SHA-256 hash a key and return its hex encoding.
    fn hash_key(key: &StorageKey) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_dir(name: &str) -> PathBuf {
        env::temp_dir().join(format!("trv1_warm_test_{name}_{}", std::process::id()))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn roundtrip_put_get() {
        let dir = temp_dir("roundtrip");
        cleanup(&dir);

        let store = WarmStore::new(&dir).unwrap();
        let key = StorageKey::from("block_42");
        let val = StorageValue::from("data_here");

        store.put(&key, &val).unwrap();
        let got = store.get(&key).unwrap();
        assert_eq!(got, Some(val));

        cleanup(&dir);
    }

    #[test]
    fn get_missing_returns_none() {
        let dir = temp_dir("missing");
        cleanup(&dir);

        let store = WarmStore::new(&dir).unwrap();
        let result = store.get(&StorageKey::from("nope")).unwrap();
        assert_eq!(result, None);

        cleanup(&dir);
    }

    #[test]
    fn remove_returns_old_value() {
        let dir = temp_dir("remove");
        cleanup(&dir);

        let store = WarmStore::new(&dir).unwrap();
        let key = StorageKey::from("to_remove");
        let val = StorageValue::from("bye");

        store.put(&key, &val).unwrap();
        let removed = store.remove(&key).unwrap();
        assert_eq!(removed, Some(val));

        // Confirm it's gone.
        assert_eq!(store.get(&key).unwrap(), None);

        cleanup(&dir);
    }

    #[test]
    fn overwrite_value() {
        let dir = temp_dir("overwrite");
        cleanup(&dir);

        let store = WarmStore::new(&dir).unwrap();
        let key = StorageKey::from("k1");
        store.put(&key, &StorageValue::from("v1")).unwrap();
        store.put(&key, &StorageValue::from("v2")).unwrap();

        let got = store.get(&key).unwrap().unwrap();
        assert_eq!(got, StorageValue::from("v2"));

        cleanup(&dir);
    }

    #[test]
    fn contains_check() {
        let dir = temp_dir("contains");
        cleanup(&dir);

        let store = WarmStore::new(&dir).unwrap();
        let key = StorageKey::from("exist");
        assert!(!store.contains(&key));

        store.put(&key, &StorageValue::from("yes")).unwrap();
        assert!(store.contains(&key));

        cleanup(&dir);
    }
}
