use parking_lot::RwLock;
use std::sync::Arc;

use crate::archive::ColdArchive;
use crate::lru_cache::HotCache;
use crate::nvme::WarmStore;
use crate::types::*;

/// Tiered storage — orchestrates Hot (LRU), Warm (NVMe), and Cold (archive) tiers.
///
/// Read path: hot -> warm -> cold, with promotion back to hot on access.
/// Write path: writes to hot immediately; evicted hot entries flush to warm.
pub struct TieredStorage {
    hot: Arc<HotCache>,
    warm: Arc<WarmStore>,
    cold: Arc<ColdArchive>,
    stats: Arc<RwLock<StorageStats>>,
}

impl TieredStorage {
    /// Create a new tiered storage system from the given config.
    pub fn new(config: &StorageConfig) -> StorageResult<Self> {
        let hot = Arc::new(HotCache::new(config.lru_capacity));
        let warm = Arc::new(WarmStore::new(&config.nvme_path)?);
        let cold = Arc::new(ColdArchive::new(&config.archive_path)?);
        let stats = Arc::new(RwLock::new(StorageStats::default()));

        // Wire up the eviction callback: when the hot cache evicts an entry,
        // flush it to the warm tier.
        let warm_for_cb = warm.clone();
        let stats_for_cb = stats.clone();
        hot.set_eviction_callback(move |key, value| {
            tracing::debug!(%key, "evicting from hot -> warm");
            if let Err(e) = warm_for_cb.put(&key, &value) {
                tracing::error!(%key, error = %e, "failed to flush evicted entry to warm store");
            }
            stats_for_cb.write().evictions += 1;
        });

        Ok(Self {
            hot,
            warm,
            cold,
            stats,
        })
    }

    /// Get a value, checking hot -> warm -> cold. Promotes to hot on access.
    pub fn get(&self, key: &StorageKey) -> StorageResult<Option<StorageValue>> {
        // 1) Hot tier.
        if let Some(val) = self.hot.get(key) {
            self.stats.write().hot_hits += 1;
            return Ok(Some(val));
        }
        self.stats.write().hot_misses += 1;

        // 2) Warm tier.
        match self.warm.get(key)? {
            Some(val) => {
                self.stats.write().warm_hits += 1;
                // Promote to hot.
                self.hot.put(key.clone(), val.clone());
                self.stats.write().promotions += 1;
                return Ok(Some(val));
            }
            None => {
                self.stats.write().warm_misses += 1;
            }
        }

        // 3) Cold tier.
        match self.cold.retrieve(key)? {
            Some(val) => {
                self.stats.write().cold_hits += 1;
                // Promote to hot.
                self.hot.put(key.clone(), val.clone());
                self.stats.write().promotions += 1;
                Ok(Some(val))
            }
            None => {
                self.stats.write().cold_misses += 1;
                Ok(None)
            }
        }
    }

    /// Put a value. Writes directly to the hot cache.
    /// Also writes to the warm tier for durability.
    pub fn put(&self, key: StorageKey, value: StorageValue) -> StorageResult<()> {
        // Write to warm tier for durability.
        self.warm.put(&key, &value)?;
        // Place in hot cache (may evict LRU -> warm via callback).
        self.hot.put(key, value);
        Ok(())
    }

    /// Remove a value from all tiers.
    pub fn remove(&self, key: &StorageKey) -> StorageResult<Option<StorageValue>> {
        let hot_val = self.hot.remove(key);
        let warm_val = self.warm.remove(key)?;
        let cold_val = self.cold.remove(key)?;
        // Return the first non-None value found.
        Ok(hot_val.or(warm_val).or(cold_val))
    }

    /// Move a key-value pair from warm storage into cold archive.
    /// This is used for data below a certain block height that is
    /// considered historical and no longer needs fast access.
    pub fn archive_key(&self, key: &StorageKey) -> StorageResult<bool> {
        if let Some(val) = self.warm.remove(key)? {
            self.cold.archive(key, &val)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Move all keys matching a prefix from warm to cold storage.
    /// In a real implementation, this would scan by block height;
    /// for now, callers pass specific keys to archive.
    pub fn archive_keys(&self, keys: &[StorageKey]) -> StorageResult<u64> {
        let mut count = 0u64;
        for key in keys {
            // Also evict from hot if present.
            self.hot.remove(key);
            if self.archive_key(key)? {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Locate which tier a key resides in (if any).
    pub fn locate(&self, key: &StorageKey) -> Option<StorageTier> {
        if self.hot.contains(key) {
            return Some(StorageTier::Hot);
        }
        if self.warm.contains(key) {
            return Some(StorageTier::Warm);
        }
        if self.cold.contains(key) {
            return Some(StorageTier::Cold);
        }
        None
    }

    /// Return a snapshot of runtime statistics.
    pub fn stats(&self) -> StorageStats {
        let mut s = self.stats.read().clone();
        s.hot_entries = self.hot.len();
        s
    }

    /// Access to the underlying hot cache (for advanced use).
    pub fn hot_cache(&self) -> &HotCache {
        &self.hot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    fn test_config(name: &str) -> (StorageConfig, PathBuf, PathBuf) {
        let pid = std::process::id();
        let warm = env::temp_dir().join(format!("trv1_tiered_warm_{name}_{pid}"));
        let cold = env::temp_dir().join(format!("trv1_tiered_cold_{name}_{pid}"));
        let _ = fs::remove_dir_all(&warm);
        let _ = fs::remove_dir_all(&cold);

        let config = StorageConfig {
            lru_capacity: 3,
            nvme_path: warm.to_string_lossy().into_owned(),
            archive_path: cold.to_string_lossy().into_owned(),
            max_ram_bytes: 1024,
        };
        (config, warm, cold)
    }

    fn cleanup(warm: &PathBuf, cold: &PathBuf) {
        let _ = fs::remove_dir_all(warm);
        let _ = fs::remove_dir_all(cold);
    }

    #[test]
    fn basic_put_get() {
        let (cfg, warm, cold) = test_config("basic");
        let ts = TieredStorage::new(&cfg).unwrap();

        ts.put(StorageKey::from("k1"), StorageValue::from("v1")).unwrap();
        let got = ts.get(&StorageKey::from("k1")).unwrap();
        assert_eq!(got, Some(StorageValue::from("v1")));

        cleanup(&warm, &cold);
    }

    #[test]
    fn get_missing_returns_none() {
        let (cfg, warm, cold) = test_config("miss");
        let ts = TieredStorage::new(&cfg).unwrap();

        assert_eq!(ts.get(&StorageKey::from("nope")).unwrap(), None);

        cleanup(&warm, &cold);
    }

    #[test]
    fn hot_eviction_flushes_to_warm() {
        let (cfg, warm, cold) = test_config("evict");
        let ts = TieredStorage::new(&cfg).unwrap();

        // Capacity is 3 — insert 4 entries to trigger eviction.
        ts.put(StorageKey::from("a"), StorageValue::from("1")).unwrap();
        ts.put(StorageKey::from("b"), StorageValue::from("2")).unwrap();
        ts.put(StorageKey::from("c"), StorageValue::from("3")).unwrap();
        ts.put(StorageKey::from("d"), StorageValue::from("4")).unwrap();

        // "a" should have been evicted from hot to warm.
        // But it was already written to warm on put, so it's still retrievable.
        let val = ts.get(&StorageKey::from("a")).unwrap();
        assert_eq!(val, Some(StorageValue::from("1")));

        let stats = ts.stats();
        assert!(stats.evictions >= 1, "expected at least 1 eviction");

        cleanup(&warm, &cold);
    }

    #[test]
    fn warm_promotion_to_hot() {
        let (cfg, warm, cold) = test_config("promote");
        let ts = TieredStorage::new(&cfg).unwrap();

        // Write some entries and fill the cache to push "a" out of hot.
        ts.put(StorageKey::from("a"), StorageValue::from("1")).unwrap();
        ts.put(StorageKey::from("b"), StorageValue::from("2")).unwrap();
        ts.put(StorageKey::from("c"), StorageValue::from("3")).unwrap();
        ts.put(StorageKey::from("d"), StorageValue::from("4")).unwrap();

        // "a" was evicted from hot but exists in warm. Getting it promotes back.
        let _ = ts.get(&StorageKey::from("a")).unwrap();

        // After promotion, "a" should be in hot tier.
        assert_eq!(ts.locate(&StorageKey::from("a")), Some(StorageTier::Hot));

        let stats = ts.stats();
        assert!(stats.promotions >= 1);

        cleanup(&warm, &cold);
    }

    #[test]
    fn archive_moves_to_cold() {
        let (cfg, warm, cold) = test_config("archive");
        let ts = TieredStorage::new(&cfg).unwrap();

        let key = StorageKey::from("old_block");
        ts.put(key.clone(), StorageValue::from("historical")).unwrap();

        // Remove from hot so we're only testing warm -> cold.
        ts.hot_cache().remove(&key);

        // Archive the key (warm -> cold).
        let moved = ts.archive_key(&key).unwrap();
        assert!(moved);

        // It should now be in cold.
        assert_eq!(ts.locate(&key), Some(StorageTier::Cold));

        // And still retrievable (from cold, with promotion).
        let val = ts.get(&key).unwrap();
        assert_eq!(val, Some(StorageValue::from("historical")));

        cleanup(&warm, &cold);
    }

    #[test]
    fn remove_from_all_tiers() {
        let (cfg, warm, cold) = test_config("remove_all");
        let ts = TieredStorage::new(&cfg).unwrap();

        let key = StorageKey::from("temp");
        ts.put(key.clone(), StorageValue::from("data")).unwrap();

        let removed = ts.remove(&key).unwrap();
        assert!(removed.is_some());

        // Confirm gone from everywhere.
        assert_eq!(ts.get(&key).unwrap(), None);
        assert_eq!(ts.locate(&key), None);

        cleanup(&warm, &cold);
    }

    #[test]
    fn stats_tracking() {
        let (cfg, warm, cold) = test_config("stats");
        let ts = TieredStorage::new(&cfg).unwrap();

        ts.put(StorageKey::from("x"), StorageValue::from("y")).unwrap();
        ts.get(&StorageKey::from("x")).unwrap(); // hot hit
        ts.get(&StorageKey::from("missing")).unwrap(); // miss all tiers

        let stats = ts.stats();
        assert_eq!(stats.hot_hits, 1);
        assert!(stats.hot_misses >= 1);
        assert_eq!(stats.hot_entries, 1);

        cleanup(&warm, &cold);
    }

    #[test]
    fn archive_multiple_keys() {
        let (cfg, warm, cold) = test_config("multi_archive");
        let ts = TieredStorage::new(&cfg).unwrap();

        let keys: Vec<StorageKey> = (0..3)
            .map(|i| StorageKey::from(format!("block_{i}").as_str()))
            .collect();

        for (i, key) in keys.iter().enumerate() {
            ts.put(key.clone(), StorageValue::new(format!("data_{i}"))).unwrap();
        }

        let archived = ts.archive_keys(&keys).unwrap();
        assert_eq!(archived, 3);

        // All should be retrievable from cold.
        for key in &keys {
            assert!(ts.get(key).unwrap().is_some());
        }

        cleanup(&warm, &cold);
    }
}
