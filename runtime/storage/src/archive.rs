use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::types::{StorageKey, StorageValue};

/// Cold archive storage — the "cold" tier for historical data.
///
/// Structurally identical to `WarmStore` but lives in a separate directory
/// intended for cheaper / higher-latency media (HDD, object storage, etc.).
pub struct ColdArchive {
    base_dir: PathBuf,
}

impl ColdArchive {
    /// Open (or create) a cold archive at the given directory.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let base_dir = path.as_ref().to_path_buf();
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Archive a key-value pair to cold storage.
    pub fn archive(&self, key: &StorageKey, value: &StorageValue) -> io::Result<()> {
        let path = self.key_path(key);
        fs::write(&path, &value.0)
    }

    /// Retrieve a value from cold storage.
    pub fn retrieve(&self, key: &StorageKey) -> io::Result<Option<StorageValue>> {
        let path = self.key_path(key);
        match fs::read(&path) {
            Ok(data) => Ok(Some(StorageValue(data))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Remove a key from cold storage.
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

    /// Check if a key exists in cold storage.
    pub fn contains(&self, key: &StorageKey) -> bool {
        self.key_path(key).exists()
    }

    fn key_path(&self, key: &StorageKey) -> PathBuf {
        let hash = Self::hash_key(key);
        let (shard, rest) = hash.split_at(2);
        let shard_dir = self.base_dir.join(shard);
        let _ = fs::create_dir_all(&shard_dir);
        shard_dir.join(rest)
    }

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
        env::temp_dir().join(format!("trv1_cold_test_{name}_{}", std::process::id()))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn archive_and_retrieve() {
        let dir = temp_dir("archive_rt");
        cleanup(&dir);

        let archive = ColdArchive::new(&dir).unwrap();
        let key = StorageKey::from("old_block");
        let val = StorageValue::from("ancient_data");

        archive.archive(&key, &val).unwrap();
        let got = archive.retrieve(&key).unwrap();
        assert_eq!(got, Some(val));

        cleanup(&dir);
    }

    #[test]
    fn retrieve_missing_returns_none() {
        let dir = temp_dir("cold_missing");
        cleanup(&dir);

        let archive = ColdArchive::new(&dir).unwrap();
        assert_eq!(archive.retrieve(&StorageKey::from("nope")).unwrap(), None);

        cleanup(&dir);
    }

    #[test]
    fn remove_from_archive() {
        let dir = temp_dir("cold_remove");
        cleanup(&dir);

        let archive = ColdArchive::new(&dir).unwrap();
        let key = StorageKey::from("del");
        let val = StorageValue::from("gone");

        archive.archive(&key, &val).unwrap();
        let removed = archive.remove(&key).unwrap();
        assert_eq!(removed, Some(val));
        assert_eq!(archive.retrieve(&key).unwrap(), None);

        cleanup(&dir);
    }

    #[test]
    fn contains_check() {
        let dir = temp_dir("cold_contains");
        cleanup(&dir);

        let archive = ColdArchive::new(&dir).unwrap();
        let key = StorageKey::from("exists");
        assert!(!archive.contains(&key));

        archive.archive(&key, &StorageValue::from("yes")).unwrap();
        assert!(archive.contains(&key));

        cleanup(&dir);
    }
}
