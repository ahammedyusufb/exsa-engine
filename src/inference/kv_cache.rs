use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};
use uuid::Uuid;

use super::context_config::SlotState;

/// Entry in the KV cache pool representing a cached context
pub struct KVCacheEntry {
    /// Unique identifier for this cache entry
    pub(crate) cache_id: Uuid,
    /// Hash of the sequence for lookup
    pub(crate) _sequence_hash: u64,
    /// Number of tokens in this cache entry  
    pub(crate) token_count: usize,
    /// Last time this entry was used
    pub(crate) last_used: std::time::Instant,
    /// Number of times this entry has been accessed
    pub(crate) reference_count: usize,
    /// Estimated memory size in bytes
    pub(crate) size_bytes: usize,
    /// Session ID that owns this cache slot (None = shared/anonymous)
    pub(crate) session_id: Option<Uuid>,
    /// Logical slot ID for llama.cpp sequence operations
    pub(crate) slot_id: usize,
    /// Number of tokens to preserve (system prompt)
    pub(crate) n_keep: usize,
    /// Current state of this slot
    pub(crate) state: SlotState,
}

impl KVCacheEntry {
    fn new(sequence_hash: u64, token_count: usize, size_bytes: usize) -> Self {
        Self::with_slot(sequence_hash, token_count, size_bytes, 0, None)
    }

    fn with_slot(
        sequence_hash: u64,
        token_count: usize,
        size_bytes: usize,
        slot_id: usize,
        session_id: Option<Uuid>,
    ) -> Self {
        Self {
            cache_id: Uuid::new_v4(),
            _sequence_hash: sequence_hash,
            token_count,
            last_used: std::time::Instant::now(),
            reference_count: 1,
            size_bytes,
            session_id,
            slot_id,
            n_keep: 0,
            state: SlotState::Active,
        }
    }

    fn touch(&mut self) {
        self.last_used = std::time::Instant::now();
        self.reference_count += 1;
    }

    /// Mark slot as warm (idle but holding valid cache)
    pub fn mark_warm(&mut self) {
        self.state = SlotState::Warm;
    }

    /// Mark slot as evictable
    pub fn mark_evictable(&mut self) {
        self.state = SlotState::Evictable;
    }

    /// Check if this slot can be evicted
    pub fn can_evict(&self) -> bool {
        self.state == SlotState::Evictable
    }

    /// Update n_keep value
    pub fn set_n_keep(&mut self, n_keep: usize) {
        self.n_keep = n_keep;
    }
}

pub struct KVCachePool {
    entries: HashMap<u64, KVCacheEntry>,
    access_order: VecDeque<u64>,
    max_entries: usize,
    max_memory_bytes: usize,
    current_memory_bytes: usize,
    hit_count: u64,
    miss_count: u64,
}

impl KVCachePool {
    pub fn new(max_entries: usize, max_memory_mb: usize) -> Self {
        Self {
            entries: HashMap::new(),
            access_order: VecDeque::new(),
            max_entries,
            max_memory_bytes: max_memory_mb * 1024 * 1024,
            current_memory_bytes: 0,
            hit_count: 0,
            miss_count: 0,
        }
    }

    pub fn get(&mut self, sequence_hash: u64) -> Option<Uuid> {
        let entry = match self.entries.get_mut(&sequence_hash) {
            Some(entry) => entry,
            None => {
                self.miss_count += 1;
                return None;
            }
        };

        entry.touch();
        let cache_id = entry.cache_id;

        self.promote_in_lru(sequence_hash);
        self.hit_count += 1;
        Some(cache_id)
    }

    pub fn put(&mut self, sequence_hash: u64, token_count: usize, size_bytes: usize) -> Uuid {
        if let Some(entry) = self.entries.get_mut(&sequence_hash) {
            entry.touch();
            let cache_id = entry.cache_id;
            self.promote_in_lru(sequence_hash);
            return cache_id;
        }

        self.evict_if_needed(size_bytes);

        let entry = KVCacheEntry::new(sequence_hash, token_count, size_bytes);
        let cache_id = entry.cache_id;

        self.entries.insert(sequence_hash, entry);
        self.access_order.push_back(sequence_hash);
        self.current_memory_bytes += size_bytes;

        info!(
            "KV cache entry created: tokens={}, size={}KB, total={}MB",
            token_count,
            size_bytes / 1024,
            self.current_memory_bytes / 1024 / 1024
        );

        cache_id
    }

    fn evict_if_needed(&mut self, incoming_size: usize) {
        while (self.entries.len() >= self.max_entries
            || self.current_memory_bytes + incoming_size > self.max_memory_bytes)
            && !self.access_order.is_empty()
        {
            if let Some(oldest_hash) = self.access_order.pop_front() {
                if let Some(entry) = self.entries.remove(&oldest_hash) {
                    self.current_memory_bytes =
                        self.current_memory_bytes.saturating_sub(entry.size_bytes);
                    debug!(
                        "Evicted KV cache entry: hash={}, refs={}, age={}s",
                        oldest_hash,
                        entry.reference_count,
                        entry.last_used.elapsed().as_secs()
                    );
                }
            }
        }
    }

    fn promote_in_lru(&mut self, sequence_hash: u64) {
        if let Some(pos) = self.access_order.iter().position(|&h| h == sequence_hash) {
            self.access_order.remove(pos);
            self.access_order.push_back(sequence_hash);
        }
    }

    pub fn remove(&mut self, sequence_hash: u64) -> bool {
        if let Some(entry) = self.entries.remove(&sequence_hash) {
            self.current_memory_bytes = self.current_memory_bytes.saturating_sub(entry.size_bytes);

            if let Some(pos) = self.access_order.iter().position(|&h| h == sequence_hash) {
                self.access_order.remove(pos);
            }

            true
        } else {
            false
        }
    }

    pub fn stats(&self) -> CachePoolStats {
        CachePoolStats {
            entries: self.entries.len(),
            memory_mb: self.current_memory_bytes / 1024 / 1024,
            hit_count: self.hit_count,
            miss_count: self.miss_count,
            hit_rate: if self.hit_count + self.miss_count > 0 {
                self.hit_count as f64 / (self.hit_count + self.miss_count) as f64
            } else {
                0.0
            },
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
        self.current_memory_bytes = 0;
        info!("KV cache pool cleared");
    }

    pub fn warmup_context(&mut self, messages: &[String]) -> Vec<u64> {
        let mut hashes = Vec::new();
        let mut cumulative_text = String::new();

        for msg in messages {
            cumulative_text.push_str(msg);
            let hash = Self::hash_sequence(&cumulative_text);
            hashes.push(hash);
        }

        hashes
    }

    /// Hash a sequence using a more robust hash function (FNV-1a inspired)
    /// This provides better distribution than DefaultHasher for string keys
    fn hash_sequence(text: &str) -> u64 {
        // FNV-1a constants for 64-bit hash
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET;
        for byte in text.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    // ==================== DEFRAGMENTATION & COMPACTION ====================

    /// Defragment the cache by removing evictable entries and consolidating memory
    ///
    /// Returns the number of entries removed and bytes freed
    pub fn defragment(&mut self) -> (usize, usize) {
        let mut removed_count = 0;
        let mut freed_bytes = 0;

        // Collect evictable entry hashes
        let evictable: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.can_evict())
            .map(|(hash, _)| *hash)
            .collect();

        // Remove evictable entries
        for hash in evictable {
            if let Some(entry) = self.entries.remove(&hash) {
                freed_bytes += entry.size_bytes;
                removed_count += 1;

                // Remove from access order
                if let Some(pos) = self.access_order.iter().position(|&h| h == hash) {
                    self.access_order.remove(pos);
                }
            }
        }

        self.current_memory_bytes = self.current_memory_bytes.saturating_sub(freed_bytes);

        if removed_count > 0 {
            info!(
                "🧹 Defragmented cache: removed {} entries, freed {}KB",
                removed_count,
                freed_bytes / 1024
            );
        }

        (removed_count, freed_bytes)
    }

    /// Compact the cache by evicting oldest entries until memory usage is below target
    ///
    /// # Arguments
    /// * `target_memory_bytes` - Target memory usage to achieve
    /// * `min_keep` - Minimum number of entries to keep (won't evict below this)
    ///
    /// Returns the number of entries evicted and bytes freed
    pub fn compact(&mut self, target_memory_bytes: usize, min_keep: usize) -> (usize, usize) {
        let mut evicted_count = 0;
        let mut freed_bytes = 0;

        // First try defragmentation
        let (defrag_count, defrag_bytes) = self.defragment();
        evicted_count += defrag_count;
        freed_bytes += defrag_bytes;

        // If still over target, evict oldest non-active entries
        while self.current_memory_bytes > target_memory_bytes
            && self.entries.len() > min_keep
            && !self.access_order.is_empty()
        {
            if let Some(oldest_hash) = self.access_order.pop_front() {
                // Skip active entries
                if let Some(entry) = self.entries.get(&oldest_hash) {
                    if entry.state == SlotState::Active {
                        // Put back at end of queue and try next
                        self.access_order.push_back(oldest_hash);
                        continue;
                    }
                }

                if let Some(entry) = self.entries.remove(&oldest_hash) {
                    freed_bytes += entry.size_bytes;
                    evicted_count += 1;
                    self.current_memory_bytes =
                        self.current_memory_bytes.saturating_sub(entry.size_bytes);

                    debug!(
                        "Compacted entry: hash={}, age={}s",
                        oldest_hash,
                        entry.last_used.elapsed().as_secs()
                    );
                }
            } else {
                break;
            }
        }

        if evicted_count > 0 {
            info!(
                "📦 Compacted cache: evicted {} entries, freed {}KB, now at {}MB",
                evicted_count,
                freed_bytes / 1024,
                self.current_memory_bytes / 1024 / 1024
            );
        }

        (evicted_count, freed_bytes)
    }

    /// Get detailed memory statistics
    pub fn memory_stats(&self) -> MemoryStats {
        let active_count = self
            .entries
            .values()
            .filter(|e| e.state == SlotState::Active)
            .count();
        let warm_count = self
            .entries
            .values()
            .filter(|e| e.state == SlotState::Warm)
            .count();
        let evictable_count = self
            .entries
            .values()
            .filter(|e| e.state == SlotState::Evictable)
            .count();

        let active_bytes: usize = self
            .entries
            .values()
            .filter(|e| e.state == SlotState::Active)
            .map(|e| e.size_bytes)
            .sum();
        let warm_bytes: usize = self
            .entries
            .values()
            .filter(|e| e.state == SlotState::Warm)
            .map(|e| e.size_bytes)
            .sum();
        let evictable_bytes: usize = self
            .entries
            .values()
            .filter(|e| e.state == SlotState::Evictable)
            .map(|e| e.size_bytes)
            .sum();

        MemoryStats {
            total_entries: self.entries.len(),
            total_bytes: self.current_memory_bytes,
            max_bytes: self.max_memory_bytes,
            active_entries: active_count,
            active_bytes,
            warm_entries: warm_count,
            warm_bytes,
            evictable_entries: evictable_count,
            evictable_bytes,
            fragmentation_ratio: if self.current_memory_bytes > 0 {
                evictable_bytes as f64 / self.current_memory_bytes as f64
            } else {
                0.0
            },
        }
    }

    /// Check if defragmentation would be beneficial
    /// Returns true if more than 20% of memory is from evictable entries
    pub fn should_defragment(&self) -> bool {
        let stats = self.memory_stats();
        stats.fragmentation_ratio > 0.2
    }

    // ==================== SESSION MANAGEMENT ====================

    /// Allocate a slot for a session, returning the slot ID
    ///
    /// If the session already has a slot, returns its existing slot.
    /// Otherwise, allocates a new slot (evicting if necessary).
    pub fn allocate_session_slot(
        &mut self,
        session_id: Uuid,
        token_count: usize,
        size_bytes: usize,
    ) -> (Uuid, usize) {
        // Check if session already has a slot - collect result first to avoid borrow issues
        let existing = self
            .entries
            .iter_mut()
            .find(|(_, entry)| entry.session_id == Some(session_id))
            .map(|(hash, entry)| {
                entry.touch();
                entry.state = SlotState::Active;
                (*hash, entry.cache_id, entry.slot_id)
            });

        if let Some((hash, cache_id, slot_id)) = existing {
            self.promote_in_lru(hash);
            debug!(
                "Reusing existing slot {} for session {}",
                slot_id, session_id
            );
            return (cache_id, slot_id);
        }

        // Allocate new slot
        self.evict_if_needed(size_bytes);

        // Find next available slot ID
        let slot_id = self.next_slot_id();
        let sequence_hash = Self::session_to_hash(session_id);

        let entry = KVCacheEntry::with_slot(
            sequence_hash,
            token_count,
            size_bytes,
            slot_id,
            Some(session_id),
        );
        let cache_id = entry.cache_id;

        self.entries.insert(sequence_hash, entry);
        self.access_order.push_back(sequence_hash);
        self.current_memory_bytes += size_bytes;

        info!(
            "Allocated slot {} for session {}: tokens={}, size={}KB",
            slot_id,
            session_id,
            token_count,
            size_bytes / 1024
        );

        (cache_id, slot_id)
    }

    /// Get the slot for a session (if it exists)
    pub fn get_session_slot(&mut self, session_id: Uuid) -> Option<(Uuid, usize)> {
        let sequence_hash = Self::session_to_hash(session_id);

        // Extract values first to avoid borrow issues
        let result = if let Some(entry) = self.entries.get_mut(&sequence_hash) {
            if entry.session_id == Some(session_id) {
                entry.touch();
                entry.state = SlotState::Active;
                Some((entry.cache_id, entry.slot_id))
            } else {
                None
            }
        } else {
            None
        };

        if result.is_some() {
            self.hit_count += 1;
            self.promote_in_lru(sequence_hash);
        } else {
            self.miss_count += 1;
        }

        result
    }

    /// Release a session's slot (marks it as evictable)
    pub fn release_session_slot(&mut self, session_id: Uuid) -> bool {
        let sequence_hash = Self::session_to_hash(session_id);

        if let Some(entry) = self.entries.get_mut(&sequence_hash) {
            if entry.session_id == Some(session_id) {
                entry.mark_evictable();
                debug!("Released slot {} for session {}", entry.slot_id, session_id);
                return true;
            }
        }

        false
    }

    /// Mark a session's slot as warm (idle but holding valid cache)
    pub fn warm_session_slot(&mut self, session_id: Uuid) -> bool {
        let sequence_hash = Self::session_to_hash(session_id);

        if let Some(entry) = self.entries.get_mut(&sequence_hash) {
            if entry.session_id == Some(session_id) {
                entry.mark_warm();
                return true;
            }
        }

        false
    }

    /// Update n_keep for a session's slot
    pub fn set_session_n_keep(&mut self, session_id: Uuid, n_keep: usize) -> bool {
        let sequence_hash = Self::session_to_hash(session_id);

        if let Some(entry) = self.entries.get_mut(&sequence_hash) {
            if entry.session_id == Some(session_id) {
                entry.set_n_keep(n_keep);
                return true;
            }
        }

        false
    }

    /// Get n_keep value for a session's slot
    pub fn get_session_n_keep(&self, session_id: Uuid) -> Option<usize> {
        let sequence_hash = Self::session_to_hash(session_id);

        self.entries
            .get(&sequence_hash)
            .filter(|e| e.session_id == Some(session_id))
            .map(|e| e.n_keep)
    }

    /// Update token count for a session's slot
    pub fn update_session_tokens(&mut self, session_id: Uuid, token_count: usize) {
        let sequence_hash = Self::session_to_hash(session_id);

        if let Some(entry) = self.entries.get_mut(&sequence_hash) {
            if entry.session_id == Some(session_id) {
                entry.token_count = token_count;
            }
        }
    }

    /// Get active session count
    pub fn active_session_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.session_id.is_some() && e.state == SlotState::Active)
            .count()
    }

    /// Get warm session count  
    pub fn warm_session_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.session_id.is_some() && e.state == SlotState::Warm)
            .count()
    }

    /// Find next available slot ID
    fn next_slot_id(&self) -> usize {
        // Simple approach: find max slot ID and add 1
        // For production, could recycle IDs from evicted slots
        self.entries
            .values()
            .map(|e| e.slot_id)
            .max()
            .map(|max| max + 1)
            .unwrap_or(0)
    }

    /// Convert session UUID to sequence hash
    fn session_to_hash(session_id: Uuid) -> u64 {
        // Use the lower 64 bits of the UUID
        let bytes = session_id.as_bytes();
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }
}

#[derive(Debug, Clone)]
pub struct CachePoolStats {
    pub entries: usize,
    pub memory_mb: usize,
    pub hit_count: u64,
    pub miss_count: u64,
    pub hit_rate: f64,
}

/// Detailed memory statistics for KV cache pool
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Total number of cache entries
    pub total_entries: usize,
    /// Total memory usage in bytes
    pub total_bytes: usize,
    /// Maximum allowed memory in bytes
    pub max_bytes: usize,
    /// Number of active (in-use) entries
    pub active_entries: usize,
    /// Memory used by active entries
    pub active_bytes: usize,
    /// Number of warm (idle but cached) entries
    pub warm_entries: usize,
    /// Memory used by warm entries
    pub warm_bytes: usize,
    /// Number of evictable entries
    pub evictable_entries: usize,
    /// Memory used by evictable entries
    pub evictable_bytes: usize,
    /// Ratio of evictable memory to total (fragmentation indicator)
    pub fragmentation_ratio: f64,
}

impl MemoryStats {
    /// Memory utilization as a percentage
    pub fn utilization(&self) -> f64 {
        if self.max_bytes > 0 {
            (self.total_bytes as f64 / self.max_bytes as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Check if memory pressure is high (>80% utilization)
    pub fn is_high_pressure(&self) -> bool {
        self.utilization() > 80.0
    }
}

pub struct SharedKVCachePool {
    inner: Arc<Mutex<KVCachePool>>,
}

impl SharedKVCachePool {
    pub fn new(max_entries: usize, max_memory_mb: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(KVCachePool::new(max_entries, max_memory_mb))),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, KVCachePool> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn get(&self, sequence_hash: u64) -> Option<Uuid> {
        self.lock_inner().get(sequence_hash)
    }

    pub fn put(&self, sequence_hash: u64, token_count: usize, size_bytes: usize) -> Uuid {
        self.lock_inner()
            .put(sequence_hash, token_count, size_bytes)
    }

    pub fn remove(&self, sequence_hash: u64) -> bool {
        self.lock_inner().remove(sequence_hash)
    }

    pub fn stats(&self) -> CachePoolStats {
        self.lock_inner().stats()
    }

    pub fn clear(&self) {
        self.lock_inner().clear()
    }
}

impl Clone for SharedKVCachePool {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let mut pool = KVCachePool::new(100, 1024);
        let hash = 12345u64;

        pool.put(hash, 100, 1024);
        assert!(pool.get(hash).is_some());
    }

    #[test]
    fn test_cache_eviction() {
        let mut pool = KVCachePool::new(2, 1024);

        pool.put(1, 100, 512);
        pool.put(2, 100, 512);
        pool.put(3, 100, 512);

        assert_eq!(pool.entries.len(), 2);
    }

    #[test]
    fn test_hit_rate() {
        let mut pool = KVCachePool::new(10, 1024);
        pool.put(1, 100, 256);

        pool.get(1);
        pool.get(1);
        pool.get(2);

        let stats = pool.stats();
        assert!(stats.hit_rate > 0.0);
    }
}
