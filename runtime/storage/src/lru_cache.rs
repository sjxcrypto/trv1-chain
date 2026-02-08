use lru::LruCache;
use parking_lot::RwLock;
use std::num::NonZeroUsize;

use crate::types::{StorageKey, StorageValue};

/// RAM-based LRU cache — the "hot" tier.
///
/// Thread-safe via `parking_lot::RwLock`. Entries that exceed the capacity
/// are evicted in least-recently-used order. An optional eviction callback
/// can be registered to flush evicted entries to the warm tier.
pub struct HotCache {
    inner: RwLock<LruCache<Vec<u8>, StorageValue>>,
    capacity: usize,
    /// Called when an entry is evicted to make room for new data.
    eviction_callback: RwLock<Option<Box<dyn Fn(StorageKey, StorageValue) + Send + Sync>>>,
}

impl HotCache {
    /// Create a new HotCache with the given entry capacity.
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).expect("LRU capacity must be > 0");
        Self {
            inner: RwLock::new(LruCache::new(cap)),
            capacity,
            eviction_callback: RwLock::new(None),
        }
    }

    /// Register a callback invoked when entries are evicted from the cache.
    pub fn set_eviction_callback<F>(&self, cb: F)
    where
        F: Fn(StorageKey, StorageValue) + Send + Sync + 'static,
    {
        *self.eviction_callback.write() = Some(Box::new(cb));
    }

    /// Retrieve a value, promoting it to most-recently-used.
    pub fn get(&self, key: &StorageKey) -> Option<StorageValue> {
        self.inner.write().get(key.as_bytes()).cloned()
    }

    /// Insert or update a key-value pair.
    ///
    /// If the cache is at capacity, the least-recently-used entry is evicted
    /// and the eviction callback is invoked (if set).
    pub fn put(&self, key: StorageKey, value: StorageValue) {
        let mut cache = self.inner.write();

        // If this key already exists, just update in-place (no eviction needed).
        if cache.contains(key.as_bytes()) {
            cache.put(key.0, value);
            return;
        }

        // If we're at capacity, manually pop the LRU entry so we can run the callback.
        if cache.len() >= self.capacity {
            if let Some((evicted_key, evicted_val)) = cache.pop_lru() {
                let cb = self.eviction_callback.read();
                if let Some(ref callback) = *cb {
                    callback(StorageKey(evicted_key), evicted_val);
                }
            }
        }

        cache.put(key.0, value);
    }

    /// Remove a key from the cache, returning its value if present.
    pub fn remove(&self, key: &StorageKey) -> Option<StorageValue> {
        self.inner.write().pop(key.as_bytes())
    }

    /// Check if a key exists without promoting it.
    pub fn contains(&self, key: &StorageKey) -> bool {
        self.inner.read().contains(key.as_bytes())
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// The configured capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    #[test]
    fn basic_put_get() {
        let cache = HotCache::new(10);
        let key = StorageKey::from("key1");
        let val = StorageValue::from("value1");
        cache.put(key.clone(), val.clone());
        assert_eq!(cache.get(&key), Some(val));
    }

    #[test]
    fn returns_none_for_missing_key() {
        let cache = HotCache::new(10);
        assert_eq!(cache.get(&StorageKey::from("absent")), None);
    }

    #[test]
    fn remove_returns_value() {
        let cache = HotCache::new(10);
        let key = StorageKey::from("k");
        cache.put(key.clone(), StorageValue::from("v"));
        let removed = cache.remove(&key);
        assert!(removed.is_some());
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn lru_eviction_at_capacity() {
        let cache = HotCache::new(2);
        cache.put(StorageKey::from("a"), StorageValue::from("1"));
        cache.put(StorageKey::from("b"), StorageValue::from("2"));
        // "a" is LRU. Inserting "c" should evict "a".
        cache.put(StorageKey::from("c"), StorageValue::from("3"));

        assert_eq!(cache.get(&StorageKey::from("a")), None); // evicted
        assert!(cache.get(&StorageKey::from("b")).is_some());
        assert!(cache.get(&StorageKey::from("c")).is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn eviction_callback_is_called() {
        let evict_count = Arc::new(AtomicUsize::new(0));
        let count_clone = evict_count.clone();

        let cache = HotCache::new(2);
        cache.set_eviction_callback(move |_k, _v| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        cache.put(StorageKey::from("a"), StorageValue::from("1"));
        cache.put(StorageKey::from("b"), StorageValue::from("2"));
        cache.put(StorageKey::from("c"), StorageValue::from("3")); // evicts "a"

        assert_eq!(evict_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn access_promotes_entry() {
        let cache = HotCache::new(2);
        cache.put(StorageKey::from("a"), StorageValue::from("1"));
        cache.put(StorageKey::from("b"), StorageValue::from("2"));

        // Access "a" — now "b" is LRU.
        cache.get(&StorageKey::from("a"));

        // Inserting "c" should evict "b" (the LRU), not "a".
        cache.put(StorageKey::from("c"), StorageValue::from("3"));
        assert!(cache.get(&StorageKey::from("a")).is_some());
        assert_eq!(cache.get(&StorageKey::from("b")), None);
    }

    #[test]
    fn overwrite_does_not_evict() {
        let evict_count = Arc::new(AtomicUsize::new(0));
        let count_clone = evict_count.clone();

        let cache = HotCache::new(2);
        cache.set_eviction_callback(move |_k, _v| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        cache.put(StorageKey::from("a"), StorageValue::from("1"));
        cache.put(StorageKey::from("b"), StorageValue::from("2"));
        // Overwrite existing key "a" — should NOT trigger eviction.
        cache.put(StorageKey::from("a"), StorageValue::from("updated"));

        assert_eq!(evict_count.load(Ordering::SeqCst), 0);
        assert_eq!(cache.get(&StorageKey::from("a")), Some(StorageValue::from("updated")));
    }

    #[test]
    fn contains_and_len() {
        let cache = HotCache::new(10);
        assert!(cache.is_empty());

        cache.put(StorageKey::from("x"), StorageValue::from("y"));
        assert!(cache.contains(&StorageKey::from("x")));
        assert!(!cache.contains(&StorageKey::from("z")));
        assert_eq!(cache.len(), 1);
    }
}
