//! Thread-per-Core in-memory database storage for VelocityDB with Enterprise Memory Eviction.
//!
//! # Architecture & Concurrency Model:
//! - **Shared-Nothing (Thread-per-Core)**: Data is isolated strictly to the local CPU core's
//!   `LocalExecutor`. Zero cross-thread locks (`Mutex`, `RwLock`), zero atomic reference counting (`Arc`),
//!   and zero concurrent hash tables (`DashMap`).
//! - **Single-Threaded Interior Mutability**: State is wrapped in `Rc<RefCell<LocalCacheInner>>`,
//!   providing zero-cost cloning across tasks within the core while enforcing `!Send` and `!Sync`
//!   at compile time.
//! - **Enterprise Memory Eviction (LRU / LFU)**:
//!   - High-performance **$O(1)$ intrusive doubly-linked list** embedded directly into the node arena.
//!   - **Zero memory allocation on every `GET`**: Access time tracking and LRU repositioning
//!     perform in-place pointer updates without heap allocation.
//!   - **Synchronous inline eviction**: When `maxmemory` or `max_entries` is reached, oldest/least
//!     frequently used keys are evicted immediately before allowing new allocations—no background threads needed.
//! - **Advanced Data Structures**:
//!   - `String`: Raw binary payload (`Vec<u8>`).
//!   - `List`: Double-ended sequence (`VecDeque<Vec<u8>>`) with O(1) push/pop at head and tail.
//!   - `Hash`: Field-value dictionary (`HashMap<Vec<u8>, Vec<u8>>`) with O(1) lookups.
//!   - `ZSet`: Sorted set providing O(1) score lookup via `HashMap` and O(log N) ordering via `BTreeSet`.
//! - **Byte-Slice API**: All operational methods accept `&[u8]` keys and values.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Wrapper around `f64` providing a total ordering for BTree indexing via `f64::total_cmp`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderedFloat(pub f64);

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Internal storage for a Redis Sorted Set (ZSET).
///
/// Combines a hash map for O(1) score lookups with a BTreeSet for O(log N) ordering
/// and range queries.
#[derive(Debug, Clone, PartialEq)]
pub struct ZSetData {
    /// O(1) member -> score dictionary
    pub dict: HashMap<Vec<u8>, f64>,
    /// O(log N) (score, member) ordered set
    pub tree: BTreeSet<(OrderedFloat, Vec<u8>)>,
}

impl Default for ZSetData {
    fn default() -> Self {
        Self::new()
    }
}

impl ZSetData {
    /// Creates a new, empty `ZSetData`.
    #[inline]
    pub fn new() -> Self {
        Self {
            dict: HashMap::new(),
            tree: BTreeSet::new(),
        }
    }

    /// Number of elements in the sorted set.
    #[inline]
    pub fn len(&self) -> usize {
        self.dict.len()
    }

    /// Returns `true` if the sorted set contains zero elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dict.is_empty()
    }

    /// Adds or updates a member with the given score in O(log N) time.
    /// Returns `true` if the member was newly added, or `false` if its score was updated.
    pub fn add(&mut self, score: f64, member: &[u8]) -> bool {
        if let Some(&old_score) = self.dict.get(member) {
            if old_score == score {
                return false;
            }
            // Remove existing entry from tree
            self.tree
                .remove(&(OrderedFloat(old_score), member.to_vec()));
            self.tree.insert((OrderedFloat(score), member.to_vec()));
            self.dict.insert(member.to_vec(), score);
            false
        } else {
            self.dict.insert(member.to_vec(), score);
            self.tree.insert((OrderedFloat(score), member.to_vec()));
            true
        }
    }

    /// Removes a member from the sorted set in O(log N) time.
    /// Returns `true` if the member existed.
    pub fn remove(&mut self, member: &[u8]) -> bool {
        if let Some(old_score) = self.dict.remove(member) {
            self.tree
                .remove(&(OrderedFloat(old_score), member.to_vec()));
            true
        } else {
            false
        }
    }

    /// Retrieves the score of a member in O(1) time.
    #[inline]
    pub fn score(&self, member: &[u8]) -> Option<f64> {
        self.dict.get(member).copied()
    }
}

/// Calculates Redis-compliant CRC16 (XMODEM variant) for hash slot assignment.
#[inline]
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Extracts the hash tag from a key if present (e.g. `{user1}data` -> `user1`).
///
/// Follows standard Redis Cluster specification:
/// If the key contains `{` and then `}`, and there is at least one byte between them,
/// the hash tag is the slice between `{` and `}`.
/// Otherwise, the entire key is considered the hash tag.
#[inline]
pub fn extract_hash_tag(key: &[u8]) -> &[u8] {
    if let Some(start) = key.iter().position(|&b| b == b'{') {
        if let Some(offset) = key[start + 1..].iter().position(|&b| b == b'}') {
            let end = start + 1 + offset;
            if end > start + 1 {
                return &key[start + 1..end];
            }
        }
    }
    key
}

/// Computes the 16384-slot hash for a key using its hash tag.
#[inline]
pub fn key_slot(key: &[u8]) -> u16 {
    let tag = extract_hash_tag(key);
    crc16(tag) % 16384
}

/// Counts set bits in a byte slice using 64-bit hardware popcount instructions.
#[inline]
pub fn bitcount_slice(bytes: &[u8]) -> usize {
    let mut total = 0usize;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let val = u64::from_ne_bytes(chunk.try_into().unwrap());
        total += val.count_ones() as usize;
    }
    for &b in chunks.remainder() {
        total += b.count_ones() as usize;
    }
    total
}

/// An individual entry ID within a Redis stream: `<ms>-<seq>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId {
    pub ms: u64,
    pub seq: u64,
}

impl StreamId {
    #[inline]
    pub const fn new(ms: u64, seq: u64) -> Self {
        Self { ms, seq }
    }

    /// Parses a byte slice into a StreamId.
    pub fn parse(s: &[u8]) -> Option<Self> {
        let s_str = std::str::from_utf8(s).ok()?;
        if s_str == "*" {
            return None;
        }
        let parts: Vec<&str> = s_str.split('-').collect();
        if parts.len() == 1 {
            let ms = parts[0].parse::<u64>().ok()?;
            Some(StreamId { ms, seq: 0 })
        } else if parts.len() == 2 {
            let ms = parts[0].parse::<u64>().ok()?;
            if parts[1] == "*" {
                Some(StreamId { ms, seq: u64::MAX })
            } else {
                let seq = parts[1].parse::<u64>().ok()?;
                Some(StreamId { ms, seq })
            }
        } else {
            None
        }
    }

    #[inline]
    pub fn format(&self) -> String {
        format!("{}-{}", self.ms, self.seq)
    }
}

/// An entry in a Redis Stream containing its unique ID and field-value pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamEntry {
    pub id: StreamId,
    pub fields: Vec<(Vec<u8>, Vec<u8>)>,
}

/// In-memory storage for a Redis Stream.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamData {
    pub entries: Vec<StreamEntry>,
    pub last_id: Option<StreamId>,
}

impl StreamData {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn xread(&self, start_id: StreamId, count: Option<usize>) -> Vec<&StreamEntry> {
        let mut results = Vec::new();
        let limit = count.unwrap_or(usize::MAX);
        for entry in &self.entries {
            if entry.id > start_id {
                results.push(entry);
                if results.len() >= limit {
                    break;
                }
            }
        }
        results
    }

    pub fn xrange(&self, start: StreamId, end: StreamId, count: Option<usize>) -> Vec<&StreamEntry> {
        let mut results = Vec::new();
        let limit = count.unwrap_or(usize::MAX);
        for entry in &self.entries {
            if entry.id >= start && entry.id <= end {
                results.push(entry);
                if results.len() >= limit {
                    break;
                }
            }
        }
        results
    }
}

/// Dynamically typed Redis value payload.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheValue {
    /// Raw binary string payload.
    String(Vec<u8>),
    /// Double-ended list with O(1) head/tail operations.
    List(VecDeque<Vec<u8>>),
    /// Field-value hash map with O(1) field lookups.
    Hash(HashMap<Vec<u8>, Vec<u8>>),
    /// Sorted set with O(1) score lookups and O(log N) range queries.
    ZSet(ZSetData),
    /// Append-only message stream.
    Stream(StreamData),
}

/// A single cache entry storing an owned payload and an optional expiration deadline.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    /// Stored value payload.
    pub value: CacheValue,
    /// Absolute monotonic deadline at which this entry expires.
    pub expires_at: Option<Instant>,
}

impl CacheEntry {
    /// Creates a new `CacheEntry` with a String value without expiration.
    #[inline]
    pub fn new_string(value: Vec<u8>) -> Self {
        Self {
            value: CacheValue::String(value),
            expires_at: None,
        }
    }

    /// Creates a new `CacheEntry` with custom value and optional expiration deadline.
    #[inline]
    pub fn with_deadline(value: CacheValue, expires_at: Option<Instant>) -> Self {
        Self { value, expires_at }
    }

    /// Returns `true` if the entry has expired relative to `now`.
    #[inline]
    pub fn is_expired(&self, now: Instant) -> bool {
        match self.expires_at {
            Some(deadline) => now >= deadline,
            None => false,
        }
    }
}

/// Memory eviction policies for VelocityDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Do not evict keys. Returns failure/ignores insertions when maxmemory is reached.
    NoEviction,
    /// Evict the least recently used (LRU) keys among all keys.
    AllKeysLru,
    /// Evict the least frequently used (LFU) keys among all keys.
    AllKeysLfu,
    /// Evict the least recently used keys among those with an expiration deadline.
    VolatileLru,
    /// Evict the least frequently used keys among those with an expiration deadline.
    VolatileLfu,
}

/// An intrusive node inside the shard's local memory arena.
#[derive(Debug)]
struct CacheNode {
    /// Stored key bytes.
    key: Vec<u8>,
    /// Cache payload and expiration metadata.
    entry: CacheEntry,
    /// Intrusive doubly-linked list previous index (towards MRU head).
    lru_prev: Option<usize>,
    /// Intrusive doubly-linked list next index (towards LRU tail).
    lru_next: Option<usize>,
    /// Total access counter for LFU policy.
    access_count: u64,
    /// Monotonic tick when this node was last accessed.
    last_access_tick: u64,
    /// Estimated memory footprint in bytes.
    size_bytes: usize,
}

impl CacheNode {
    fn new(key: Vec<u8>, entry: CacheEntry, size_bytes: usize, tick: u64) -> Self {
        Self {
            key,
            entry,
            lru_prev: None,
            lru_next: None,
            access_count: 1,
            last_access_tick: tick,
            size_bytes,
        }
    }
}

/// Estimates memory overhead in bytes for a given key and value payload.
fn estimate_entry_bytes(key: &[u8], value: &CacheValue) -> usize {
    // 64 bytes base node + hash table bucket overhead
    let base = 64 + key.len();
    let val_bytes = match value {
        CacheValue::String(s) => s.len(),
        CacheValue::List(l) => l.iter().map(|item| 32 + item.len()).sum(),
        CacheValue::Hash(h) => h.iter().map(|(k, v)| 48 + k.len() + v.len()).sum(),
        CacheValue::ZSet(z) => z.dict.iter().map(|(m, _)| 64 + m.len()).sum(),
        CacheValue::Stream(s) => s.entries.iter().map(|e| 32 + e.fields.iter().map(|(f, v)| 32 + f.len() + v.len()).sum::<usize>()).sum(),
    };
    base + val_bytes
}

/// Internal store representation holding the hash map, intrusive doubly-linked list, and arena.
#[derive(Debug)]
struct LocalCacheInner {
    /// Mapping: key bytes -> index in `nodes` arena.
    map: HashMap<Vec<u8>, usize>,
    /// Dense, reusable slot arena storing cache nodes.
    nodes: Vec<Option<CacheNode>>,
    /// Free slot indices for O(1) allocation reuse without memory fragmentation.
    free_slots: Vec<usize>,

    /// Head of intrusive LRU list (Most Recently Used).
    lru_head: Option<usize>,
    /// Tail of intrusive LRU list (Least Recently Used).
    lru_tail: Option<usize>,

    /// Maximum allowable memory in bytes (0 = unlimited).
    max_memory: usize,
    /// Maximum allowable number of key entries (0 = unlimited).
    max_entries: usize,
    /// Current estimated memory consumption in bytes.
    current_memory: usize,
    /// Configured eviction policy.
    policy: EvictionPolicy,

    /// Total number of keys evicted due to memory limits.
    evictions: usize,
    /// Monotonic logical clock for access recency tie-breaking.
    logical_clock: u64,
}

impl LocalCacheInner {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            nodes: Vec::with_capacity(capacity),
            free_slots: Vec::new(),
            lru_head: None,
            lru_tail: None,
            max_memory: 0,
            max_entries: 0,
            current_memory: 0,
            policy: EvictionPolicy::AllKeysLru,
            evictions: 0,
            logical_clock: 0,
        }
    }

    // -------------------------------------------------------------------------
    // INTRUSIVE DOUBLY-LINKED LIST OPERATIONS (O(1), ZERO-ALLOCATION)
    // -------------------------------------------------------------------------

    /// Detaches a node from the intrusive LRU doubly-linked list in O(1) time.
    #[inline]
    fn detach_lru(&mut self, idx: usize) {
        let (prev, next) = match self.nodes[idx] {
            Some(ref mut node) => {
                let p = node.lru_prev;
                let n = node.lru_next;
                node.lru_prev = None;
                node.lru_next = None;
                (p, n)
            }
            None => return,
        };

        if let Some(p) = prev {
            if let Some(ref mut p_node) = self.nodes[p] {
                p_node.lru_next = next;
            }
        } else {
            self.lru_head = next;
        }

        if let Some(n) = next {
            if let Some(ref mut n_node) = self.nodes[n] {
                n_node.lru_prev = prev;
            }
        } else {
            self.lru_tail = prev;
        }
    }

    /// Attaches a node to the head (MRU) of the intrusive LRU list in O(1) time.
    #[inline]
    fn attach_lru_head(&mut self, idx: usize) {
        let old_head = self.lru_head;
        if let Some(ref mut node) = self.nodes[idx] {
            node.lru_prev = None;
            node.lru_next = old_head;
        }

        if let Some(h) = old_head {
            if let Some(ref mut h_node) = self.nodes[h] {
                h_node.lru_prev = Some(idx);
            }
        } else {
            self.lru_tail = Some(idx);
        }

        self.lru_head = Some(idx);
    }

    /// Marks a node as recently accessed in O(1) time with **zero heap allocations**.
    #[inline]
    fn touch_node(&mut self, idx: usize) {
        self.detach_lru(idx);
        self.attach_lru_head(idx);
        self.logical_clock = self.logical_clock.wrapping_add(1);
        if let Some(ref mut node) = self.nodes[idx] {
            node.access_count = node.access_count.saturating_add(1);
            node.last_access_tick = self.logical_clock;
        }
    }

    /// Allocates or reuses a slot in the arena in O(1) time.
    fn allocate_slot(&mut self, key: Vec<u8>, entry: CacheEntry, size_bytes: usize) -> usize {
        self.logical_clock = self.logical_clock.wrapping_add(1);
        let node = CacheNode::new(key, entry, size_bytes, self.logical_clock);

        let idx = if let Some(free_idx) = self.free_slots.pop() {
            self.nodes[free_idx] = Some(node);
            free_idx
        } else {
            let new_idx = self.nodes.len();
            self.nodes.push(Some(node));
            new_idx
        };

        self.attach_lru_head(idx);
        self.current_memory += size_bytes;
        idx
    }

    /// Frees a slot in the arena and unlinks it in O(1) time.
    fn free_slot(&mut self, idx: usize) -> Option<CacheNode> {
        self.detach_lru(idx);
        let node = self.nodes[idx].take();
        if let Some(ref n) = node {
            self.current_memory = self.current_memory.saturating_sub(n.size_bytes);
            self.free_slots.push(idx);
        }
        node
    }

    // -------------------------------------------------------------------------
    // SYNCHRONOUS INLINE MEMORY EVICTION (LRU / LFU)
    // -------------------------------------------------------------------------

    /// Synchronously evicts the optimal candidate key based on the configured policy.
    ///
    /// Returns `true` if an entry was evicted, or `false` if no candidate was available.
    fn evict_one(&mut self) -> bool {
        let candidate_idx = match self.policy {
            EvictionPolicy::NoEviction => return false,
            EvictionPolicy::AllKeysLru => self.lru_tail,
            EvictionPolicy::AllKeysLfu => self.find_lfu_candidate(false),
            EvictionPolicy::VolatileLru => self.find_volatile_lru_candidate(),
            EvictionPolicy::VolatileLfu => self.find_lfu_candidate(true),
        };

        if let Some(idx) = candidate_idx {
            if let Some(node) = self.free_slot(idx) {
                self.map.remove(&node.key);
                self.evictions += 1;
                return true;
            }
        }

        false
    }

    /// Finds the least frequently used (LFU) candidate.
    fn find_lfu_candidate(&self, volatile_only: bool) -> Option<usize> {
        let mut min_count = u64::MAX;
        let mut oldest_tick = u64::MAX;
        let mut best_idx = None;

        // Sample up to 32 nodes starting from LRU tail where cold keys naturally collect
        let mut curr = self.lru_tail;
        let mut samples = 0;

        while let Some(idx) = curr {
            if let Some(ref node) = self.nodes[idx] {
                if !volatile_only || node.entry.expires_at.is_some() {
                    if node.access_count < min_count
                        || (node.access_count == min_count && node.last_access_tick < oldest_tick)
                    {
                        min_count = node.access_count;
                        oldest_tick = node.last_access_tick;
                        best_idx = Some(idx);
                    }
                }
                curr = node.lru_prev;
                samples += 1;
                if samples >= 32 && best_idx.is_some() {
                    break;
                }
            } else {
                break;
            }
        }

        best_idx.or(self.lru_tail)
    }

    /// Finds the least recently used candidate among volatile keys (keys with TTL).
    fn find_volatile_lru_candidate(&self) -> Option<usize> {
        let mut curr = self.lru_tail;
        while let Some(idx) = curr {
            if let Some(ref node) = self.nodes[idx] {
                if node.entry.expires_at.is_some() {
                    return Some(idx);
                }
                curr = node.lru_prev;
            } else {
                break;
            }
        }
        // Fallback to tail if no volatile keys found
        self.lru_tail
    }

    /// Ensures sufficient capacity exists before accepting a new allocation.
    /// Synchronously evicts entries inline until requirements are met.
    fn ensure_capacity(&mut self, required_bytes: usize) -> bool {
        if self.policy == EvictionPolicy::NoEviction {
            if (self.max_memory > 0 && self.current_memory + required_bytes > self.max_memory)
                || (self.max_entries > 0 && self.map.len() >= self.max_entries)
            {
                return false;
            }
            return true;
        }

        // Inline synchronous eviction loop
        while (!self.map.is_empty())
            && ((self.max_memory > 0 && self.current_memory + required_bytes > self.max_memory)
                || (self.max_entries > 0 && self.map.len() >= self.max_entries))
        {
            if !self.evict_one() {
                break;
            }
        }

        true
    }
}

/// Thread-local in-memory key-value cache for a single CPU core with Enterprise Eviction.
///
/// Because `LocalCache` contains `Rc<RefCell<...>>`, it is strictly `!Send` and `!Sync`.
/// Rust's type system prevents this struct from ever crossing thread boundaries.
#[derive(Debug, Clone)]
pub struct LocalCache {
    inner: Rc<RefCell<LocalCacheInner>>,
}

impl Default for LocalCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalCache {
    /// Creates a new, empty `LocalCache`.
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(LocalCacheInner::with_capacity(1024))),
        }
    }

    /// Creates a new `LocalCache` with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new(LocalCacheInner::with_capacity(capacity))),
        }
    }

    // -------------------------------------------------------------------------
    // ENTERPRISE EVICTION & MEMORY MANAGEMENT API
    // -------------------------------------------------------------------------

    /// Sets the maximum allowable memory in bytes (0 = unlimited).
    pub fn set_max_memory(&self, bytes: usize) {
        let mut inner = self.inner.borrow_mut();
        inner.max_memory = bytes;
        if bytes > 0 {
            inner.ensure_capacity(0);
        }
    }

    /// Returns the configured maximum memory in bytes.
    pub fn max_memory(&self) -> usize {
        self.inner.borrow().max_memory
    }

    /// Sets the maximum allowable number of key entries (0 = unlimited).
    pub fn set_max_entries(&self, count: usize) {
        let mut inner = self.inner.borrow_mut();
        inner.max_entries = count;
        if count > 0 {
            inner.ensure_capacity(0);
        }
    }

    /// Returns the configured maximum number of key entries.
    pub fn max_entries(&self) -> usize {
        self.inner.borrow().max_entries
    }

    /// Sets the memory eviction policy (e.g. `AllKeysLru`, `AllKeysLfu`, `NoEviction`).
    pub fn set_eviction_policy(&self, policy: EvictionPolicy) {
        self.inner.borrow_mut().policy = policy;
    }

    /// Returns the currently active memory eviction policy.
    pub fn eviction_policy(&self) -> EvictionPolicy {
        self.inner.borrow().policy
    }

    /// Returns the current estimated memory consumption in bytes.
    pub fn memory_usage(&self) -> usize {
        self.inner.borrow().current_memory
    }

    /// Returns the total number of keys synchronously evicted due to memory pressure.
    pub fn evictions_count(&self) -> usize {
        self.inner.borrow().evictions
    }

    // -------------------------------------------------------------------------
    // STRING OPERATIONS (GET, SET)
    // -------------------------------------------------------------------------

    /// Stores a raw string value without expiration.
    pub fn set(&self, key: &[u8], value: &[u8]) {
        let mut inner = self.inner.borrow_mut();
        let val_payload = CacheValue::String(value.to_vec());
        let new_size = estimate_entry_bytes(key, &val_payload);

        // Update existing key
        if let Some(&idx) = inner.map.get(key) {
            let old_size = inner.nodes[idx].as_ref().map(|n| n.size_bytes).unwrap_or(0);
            inner.current_memory = inner.current_memory.saturating_sub(old_size);
            inner.ensure_capacity(new_size);

            if let Some(ref mut node) = inner.nodes[idx] {
                node.entry.value = val_payload;
                node.entry.expires_at = None;
                node.size_bytes = new_size;
            }
            inner.current_memory += new_size;
            inner.touch_node(idx);
            return;
        }

        // Insert new key with synchronous inline eviction
        if !inner.ensure_capacity(new_size) {
            return; // NoEviction policy rejection
        }

        let entry = CacheEntry::new_string(value.to_vec());
        let idx = inner.allocate_slot(key.to_vec(), entry, new_size);
        inner.map.insert(key.to_vec(), idx);
    }

    /// Stores a raw string value with a relative Time-to-Live (TTL).
    pub fn set_with_ttl(&self, key: &[u8], value: &[u8], ttl: Duration) {
        let expires_at = Instant::now().checked_add(ttl);
        let mut inner = self.inner.borrow_mut();
        let val_payload = CacheValue::String(value.to_vec());
        let new_size = estimate_entry_bytes(key, &val_payload);

        // Update existing key
        if let Some(&idx) = inner.map.get(key) {
            let old_size = inner.nodes[idx].as_ref().map(|n| n.size_bytes).unwrap_or(0);
            inner.current_memory = inner.current_memory.saturating_sub(old_size);
            inner.ensure_capacity(new_size);

            if let Some(ref mut node) = inner.nodes[idx] {
                node.entry.value = val_payload;
                node.entry.expires_at = expires_at;
                node.size_bytes = new_size;
            }
            inner.current_memory += new_size;
            inner.touch_node(idx);
            return;
        }

        // Insert new key with synchronous inline eviction
        if !inner.ensure_capacity(new_size) {
            return;
        }

        let entry = CacheEntry::with_deadline(val_payload, expires_at);
        let idx = inner.allocate_slot(key.to_vec(), entry, new_size);
        inner.map.insert(key.to_vec(), idx);
    }

    /// Retrieves a cloned copy of the raw string value associated with `key`.
    ///
    /// Updates the access time in $O(1)$ time with **zero heap allocation**.
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        // O(1) in-place intrusive LRU update (zero allocation)
        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::String(ref s) = node.entry.value {
                return Some(s.clone());
            }
        }

        None
    }

    /// Zero-copy read access to a string value via closure, avoiding allocation.
    pub fn with_value<R, F: FnOnce(&[u8]) -> R>(&self, key: &[u8], f: F) -> Option<R> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::String(ref s) = node.entry.value {
                return Some(f(s));
            }
        }

        None
    }

    // -------------------------------------------------------------------------
    // HASH OPERATIONS (HSET, HGET, HDEL, HEXISTS, HLEN, HGETALL)
    // -------------------------------------------------------------------------

    /// Sets field in the hash stored at key to value.
    pub fn hset(&self, key: &[u8], field: &[u8], value: &[u8]) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx]
                .as_ref()
                .map(|n| n.entry.is_expired(now))
                .unwrap_or(false);

            if is_expired {
                inner.map.remove(key);
                inner.free_slot(idx);
            } else {
                let update_result = if let Some(ref mut node) = inner.nodes[idx] {
                    if let CacheValue::Hash(ref mut hash) = node.entry.value {
                        let is_new = hash.insert(field.to_vec(), value.to_vec()).is_none();
                        let updated_size = estimate_entry_bytes(key, &node.entry.value);
                        let old_size = node.size_bytes;
                        node.size_bytes = updated_size;
                        Some((is_new, old_size, updated_size))
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some((is_new, old_size, updated_size)) = update_result {
                    inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
                    inner.touch_node(idx);
                    return is_new;
                } else if inner.nodes[idx].is_some() {
                    return false;
                }
            }
        }

        let mut hash = HashMap::new();
        hash.insert(field.to_vec(), value.to_vec());
        let val_payload = CacheValue::Hash(hash);
        let size = estimate_entry_bytes(key, &val_payload);

        if !inner.ensure_capacity(size) {
            return false;
        }

        let entry = CacheEntry::with_deadline(val_payload, None);
        let idx = inner.allocate_slot(key.to_vec(), entry, size);
        inner.map.insert(key.to_vec(), idx);
        true
    }

    /// Returns the value associated with field in the hash stored at key.
    pub fn hget(&self, key: &[u8], field: &[u8]) -> Option<Vec<u8>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::Hash(ref hash) = node.entry.value {
                return hash.get(field).cloned();
            }
        }

        None
    }

    /// Removes the specified field from the hash stored at key.
    pub fn hdel(&self, key: &[u8], field: &[u8]) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return false,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return false;
        }

        let mut should_remove_key = false;
        let mut removed = false;
        let mut mem_delta = None;

        if let Some(ref mut node) = inner.nodes[idx] {
            if let CacheValue::Hash(ref mut hash) = node.entry.value {
                removed = hash.remove(field).is_some();
                if hash.is_empty() {
                    should_remove_key = true;
                } else {
                    let updated_size = estimate_entry_bytes(key, &node.entry.value);
                    let old_size = node.size_bytes;
                    node.size_bytes = updated_size;
                    mem_delta = Some((old_size, updated_size));
                }
            }
        }

        if let Some((old_size, updated_size)) = mem_delta {
            inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
        }

        if should_remove_key {
            inner.map.remove(key);
            inner.free_slot(idx);
        } else if removed {
            inner.touch_node(idx);
        }

        removed
    }

    /// Returns `true` if field exists in the hash stored at key.
    pub fn hexists(&self, key: &[u8], field: &[u8]) -> bool {
        self.hget(key, field).is_some()
    }

    /// Returns the number of fields contained in the hash stored at key.
    pub fn hlen(&self, key: &[u8]) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return 0,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return 0;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::Hash(ref hash) = node.entry.value {
                return hash.len();
            }
        }

        0
    }

    /// Returns all fields and values of the hash stored at key.
    pub fn hgetall(&self, key: &[u8]) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::Hash(ref hash) = node.entry.value {
                return Some(hash.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
        }

        None
    }

    // -------------------------------------------------------------------------
    // LIST OPERATIONS (LPUSH, RPUSH, LPOP, RPOP, LLEN, LRANGE, LINDEX)
    // -------------------------------------------------------------------------

    /// Inserts element at the head of the list. Returns the new list length.
    pub fn lpush(&self, key: &[u8], value: &[u8]) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx]
                .as_ref()
                .map(|n| n.entry.is_expired(now))
                .unwrap_or(false);

            if is_expired {
                inner.map.remove(key);
                inner.free_slot(idx);
            } else {
                let update_result = if let Some(ref mut node) = inner.nodes[idx] {
                    if let CacheValue::List(ref mut list) = node.entry.value {
                        list.push_front(value.to_vec());
                        let len = list.len();
                        let updated_size = estimate_entry_bytes(key, &node.entry.value);
                        let old_size = node.size_bytes;
                        node.size_bytes = updated_size;
                        Some((len, old_size, updated_size))
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some((len, old_size, updated_size)) = update_result {
                    inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
                    inner.touch_node(idx);
                    return len;
                } else if inner.nodes[idx].is_some() {
                    return 0;
                }
            }
        }

        let mut list = VecDeque::new();
        list.push_front(value.to_vec());
        let val_payload = CacheValue::List(list);
        let size = estimate_entry_bytes(key, &val_payload);

        if !inner.ensure_capacity(size) {
            return 0;
        }

        let entry = CacheEntry::with_deadline(val_payload, None);
        let idx = inner.allocate_slot(key.to_vec(), entry, size);
        inner.map.insert(key.to_vec(), idx);
        1
    }

    /// Inserts element at the tail of the list. Returns the new list length.
    pub fn rpush(&self, key: &[u8], value: &[u8]) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx]
                .as_ref()
                .map(|n| n.entry.is_expired(now))
                .unwrap_or(false);

            if is_expired {
                inner.map.remove(key);
                inner.free_slot(idx);
            } else {
                let update_result = if let Some(ref mut node) = inner.nodes[idx] {
                    if let CacheValue::List(ref mut list) = node.entry.value {
                        list.push_back(value.to_vec());
                        let len = list.len();
                        let updated_size = estimate_entry_bytes(key, &node.entry.value);
                        let old_size = node.size_bytes;
                        node.size_bytes = updated_size;
                        Some((len, old_size, updated_size))
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some((len, old_size, updated_size)) = update_result {
                    inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
                    inner.touch_node(idx);
                    return len;
                } else if inner.nodes[idx].is_some() {
                    return 0;
                }
            }
        }

        let mut list = VecDeque::new();
        list.push_back(value.to_vec());
        let val_payload = CacheValue::List(list);
        let size = estimate_entry_bytes(key, &val_payload);

        if !inner.ensure_capacity(size) {
            return 0;
        }

        let entry = CacheEntry::with_deadline(val_payload, None);
        let idx = inner.allocate_slot(key.to_vec(), entry, size);
        inner.map.insert(key.to_vec(), idx);
        1
    }

    /// Removes and returns the first element of the list.
    pub fn lpop(&self, key: &[u8]) -> Option<Vec<u8>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        let mut should_remove_key = false;
        let mut val = None;
        let mut mem_delta = None;

        if let Some(ref mut node) = inner.nodes[idx] {
            if let CacheValue::List(ref mut list) = node.entry.value {
                val = list.pop_front();
                if list.is_empty() {
                    should_remove_key = true;
                } else {
                    let updated_size = estimate_entry_bytes(key, &node.entry.value);
                    let old_size = node.size_bytes;
                    node.size_bytes = updated_size;
                    mem_delta = Some((old_size, updated_size));
                }
            }
        }

        if let Some((old_size, updated_size)) = mem_delta {
            inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
        }

        if should_remove_key {
            inner.map.remove(key);
            inner.free_slot(idx);
        } else if val.is_some() {
            inner.touch_node(idx);
        }

        val
    }

    /// Removes and returns the last element of the list.
    pub fn rpop(&self, key: &[u8]) -> Option<Vec<u8>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        let mut should_remove_key = false;
        let mut val = None;
        let mut mem_delta = None;

        if let Some(ref mut node) = inner.nodes[idx] {
            if let CacheValue::List(ref mut list) = node.entry.value {
                val = list.pop_back();
                if list.is_empty() {
                    should_remove_key = true;
                } else {
                    let updated_size = estimate_entry_bytes(key, &node.entry.value);
                    let old_size = node.size_bytes;
                    node.size_bytes = updated_size;
                    mem_delta = Some((old_size, updated_size));
                }
            }
        }

        if let Some((old_size, updated_size)) = mem_delta {
            inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
        }

        if should_remove_key {
            inner.map.remove(key);
            inner.free_slot(idx);
        } else if val.is_some() {
            inner.touch_node(idx);
        }

        val
    }

    /// Returns the length of the list stored at key.
    pub fn llen(&self, key: &[u8]) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return 0,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return 0;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::List(ref list) = node.entry.value {
                return list.len();
            }
        }

        0
    }

    /// Returns elements from list between `start` and `stop` (inclusive).
    pub fn lrange(&self, key: &[u8], start: i64, stop: i64) -> Vec<Vec<u8>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return Vec::new(),
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return Vec::new();
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::List(ref list) = node.entry.value {
                let len = list.len();
                if len == 0 {
                    return Vec::new();
                }

                let s = if start < 0 {
                    (len as i64 + start).max(0) as usize
                } else {
                    start as usize
                };

                let e = if stop < 0 {
                    (len as i64 + stop).max(0) as usize
                } else {
                    stop as usize
                };

                if s >= len || s > e {
                    return Vec::new();
                }

                let e = e.min(len - 1);
                return list.range(s..=e).cloned().collect();
            }
        }

        Vec::new()
    }

    /// Returns the element at index in the list stored at key.
    pub fn lindex(&self, key: &[u8], index: i64) -> Option<Vec<u8>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::List(ref list) = node.entry.value {
                let len = list.len();
                if len == 0 {
                    return None;
                }
                let pos = if index < 0 {
                    let p = len as i64 + index;
                    if p < 0 {
                        return None;
                    }
                    p as usize
                } else {
                    index as usize
                };

                return list.get(pos).cloned();
            }
        }

        None
    }

    // -------------------------------------------------------------------------
    // SORTED SET (ZSET) OPERATIONS (ZADD, ZSCORE, ZREM, ZCARD, ZRANGE, ZREVRANGE, ZRANGEBYSCORE)
    // -------------------------------------------------------------------------

    /// Adds the specified member with the specified score to the sorted set stored at key.
    pub fn zadd(&self, key: &[u8], score: f64, member: &[u8]) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx]
                .as_ref()
                .map(|n| n.entry.is_expired(now))
                .unwrap_or(false);

            if is_expired {
                inner.map.remove(key);
                inner.free_slot(idx);
            } else {
                let update_result = if let Some(ref mut node) = inner.nodes[idx] {
                    if let CacheValue::ZSet(ref mut zset) = node.entry.value {
                        let is_new = zset.add(score, member);
                        let updated_size = estimate_entry_bytes(key, &node.entry.value);
                        let old_size = node.size_bytes;
                        node.size_bytes = updated_size;
                        Some((is_new, old_size, updated_size))
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some((is_new, old_size, updated_size)) = update_result {
                    inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
                    inner.touch_node(idx);
                    return is_new;
                } else if inner.nodes[idx].is_some() {
                    return false;
                }
            }
        }

        let mut zset = ZSetData::new();
        zset.add(score, member);
        let val_payload = CacheValue::ZSet(zset);
        let size = estimate_entry_bytes(key, &val_payload);

        if !inner.ensure_capacity(size) {
            return false;
        }

        let entry = CacheEntry::with_deadline(val_payload, None);
        let idx = inner.allocate_slot(key.to_vec(), entry, size);
        inner.map.insert(key.to_vec(), idx);
        true
    }

    /// Returns the score of member in the sorted set at key in O(1) time.
    pub fn zscore(&self, key: &[u8], member: &[u8]) -> Option<f64> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::ZSet(ref zset) = node.entry.value {
                return zset.score(member);
            }
        }

        None
    }

    /// Removes the specified member from the sorted set at key in O(log N) time.
    pub fn zrem(&self, key: &[u8], member: &[u8]) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return false,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return false;
        }

        let mut should_remove_key = false;
        let mut removed = false;
        let mut mem_delta = None;

        if let Some(ref mut node) = inner.nodes[idx] {
            if let CacheValue::ZSet(ref mut zset) = node.entry.value {
                removed = zset.remove(member);
                if zset.is_empty() {
                    should_remove_key = true;
                } else {
                    let updated_size = estimate_entry_bytes(key, &node.entry.value);
                    let old_size = node.size_bytes;
                    node.size_bytes = updated_size;
                    mem_delta = Some((old_size, updated_size));
                }
            }
        }

        if let Some((old_size, updated_size)) = mem_delta {
            inner.current_memory = inner.current_memory.saturating_sub(old_size) + updated_size;
        }

        if should_remove_key {
            inner.map.remove(key);
            inner.free_slot(idx);
        } else if removed {
            inner.touch_node(idx);
        }

        removed
    }

    /// Returns the sorted set cardinality in O(1) time.
    pub fn zcard(&self, key: &[u8]) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return 0,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return 0;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::ZSet(ref zset) = node.entry.value {
                return zset.len();
            }
        }

        0
    }

    /// Returns elements in the sorted set ordered by score ascending.
    pub fn zrange(&self, key: &[u8], start: i64, stop: i64) -> Vec<(Vec<u8>, f64)> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return Vec::new(),
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return Vec::new();
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::ZSet(ref zset) = node.entry.value {
                let len = zset.len();
                if len == 0 {
                    return Vec::new();
                }

                let s = if start < 0 {
                    (len as i64 + start).max(0) as usize
                } else {
                    start as usize
                };

                let e = if stop < 0 {
                    (len as i64 + stop).max(0) as usize
                } else {
                    stop as usize
                };

                if s >= len || s > e {
                    return Vec::new();
                }

                let count = (e.min(len - 1) - s) + 1;
                return zset
                    .tree
                    .iter()
                    .skip(s)
                    .take(count)
                    .map(|(score, member)| (member.clone(), score.0))
                    .collect();
            }
        }

        Vec::new()
    }

    /// Returns elements in the sorted set ordered by score descending.
    pub fn zrevrange(&self, key: &[u8], start: i64, stop: i64) -> Vec<(Vec<u8>, f64)> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return Vec::new(),
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return Vec::new();
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::ZSet(ref zset) = node.entry.value {
                let len = zset.len();
                if len == 0 {
                    return Vec::new();
                }

                let s = if start < 0 {
                    (len as i64 + start).max(0) as usize
                } else {
                    start as usize
                };

                let e = if stop < 0 {
                    (len as i64 + stop).max(0) as usize
                } else {
                    stop as usize
                };

                if s >= len || s > e {
                    return Vec::new();
                }

                let count = (e.min(len - 1) - s) + 1;
                return zset
                    .tree
                    .iter()
                    .rev()
                    .skip(s)
                    .take(count)
                    .map(|(score, member)| (member.clone(), score.0))
                    .collect();
            }
        }

        Vec::new()
    }

    /// Returns elements in the sorted set with score between `min` and `max`.
    pub fn zrangebyscore(&self, key: &[u8], min: f64, max: f64) -> Vec<(Vec<u8>, f64)> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return Vec::new(),
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return Vec::new();
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            if let CacheValue::ZSet(ref zset) = node.entry.value {
                let min_bound = (OrderedFloat(min), Vec::new());
                return zset
                    .tree
                    .range(min_bound..)
                    .take_while(|(score, _)| score.0 <= max)
                    .map(|(score, member)| (member.clone(), score.0))
                    .collect();
            }
        }

        Vec::new()
    }

    // -------------------------------------------------------------------------
    // BITMAP OPERATIONS (SETBIT, GETBIT, BITCOUNT)
    // -------------------------------------------------------------------------

    /// Sets or clears the bit at offset in the string value stored at key.
    /// Returns the original bit value stored at offset.
    pub fn setbit(&self, key: &[u8], offset: usize, value: u8) -> Result<u8, &'static str> {
        if value != 0 && value != 1 {
            return Err("ERR bit is not an integer or out of range");
        }
        if offset >= 536_870_912 * 8 {
            return Err("ERR bit offset is not an integer or out of range");
        }

        let byte_index = offset / 8;
        let bit_offset = 7 - (offset % 8);
        let mask = 1u8 << bit_offset;

        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
            if is_expired {
                inner.free_slot(idx);
                inner.map.remove(key);
            } else {
                let (old_bit, delta) = {
                    let node = inner.nodes[idx].as_mut().unwrap();
                    match &mut node.entry.value {
                        CacheValue::String(ref mut bytes) => {
                            let old_len = bytes.len();
                            if bytes.len() <= byte_index {
                                bytes.resize(byte_index + 1, 0);
                            }
                            let old_bit = (bytes[byte_index] >> bit_offset) & 1;
                            if value == 1 {
                                bytes[byte_index] |= mask;
                            } else {
                                bytes[byte_index] &= !mask;
                            }
                            let delta = bytes.len().saturating_sub(old_len);
                            node.size_bytes += delta;
                            (old_bit, delta)
                        }
                        _ => return Err("WRONGTYPE Operation against a key holding the wrong kind of value"),
                    }
                };

                inner.current_memory += delta;
                inner.touch_node(idx);
                return Ok(old_bit);
            }
        }

        let mut bytes = vec![0u8; byte_index + 1];
        if value == 1 {
            bytes[byte_index] |= mask;
        }
        let val_payload = CacheValue::String(bytes);
        let new_size = estimate_entry_bytes(key, &val_payload);
        if !inner.ensure_capacity(new_size) {
            return Err("OOM command not allowed when used memory > 'maxmemory'");
        }
        let entry = CacheEntry::with_deadline(val_payload, None);
        let slot = inner.allocate_slot(key.to_vec(), entry, new_size);
        inner.map.insert(key.to_vec(), slot);
        Ok(0)
    }

    /// Returns the bit value at offset in the string value stored at key.
    pub fn getbit(&self, key: &[u8], offset: usize) -> u8 {
        let byte_index = offset / 8;
        let bit_offset = 7 - (offset % 8);

        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
            if is_expired {
                inner.free_slot(idx);
                inner.map.remove(key);
                return 0;
            }
            inner.touch_node(idx);
            if let Some(ref node) = inner.nodes[idx] {
                if let CacheValue::String(ref bytes) = node.entry.value {
                    if byte_index < bytes.len() {
                        return (bytes[byte_index] >> bit_offset) & 1;
                    }
                }
            }
        }
        0
    }

    /// Counts set bits in a string using SIMD/hardware popcount.
    pub fn bitcount(&self, key: &[u8], start: Option<isize>, end: Option<isize>) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
            if is_expired {
                inner.free_slot(idx);
                inner.map.remove(key);
                return 0;
            }
            inner.touch_node(idx);
            if let Some(ref node) = inner.nodes[idx] {
                if let CacheValue::String(ref bytes) = node.entry.value {
                    if bytes.is_empty() {
                        return 0;
                    }
                    let len = bytes.len() as isize;
                    let (s, e) = match (start, end) {
                        (Some(st), Some(en)) => {
                            let s_idx = if st < 0 { (len + st).max(0) } else { st.min(len) } as usize;
                            let e_idx = if en < 0 { (len + en).max(0) } else { en.min(len - 1) } as usize;
                            (s_idx, e_idx)
                        }
                        (Some(st), None) => {
                            let s_idx = if st < 0 { (len + st).max(0) } else { st.min(len) } as usize;
                            (s_idx, (len - 1) as usize)
                        }
                        _ => (0, (len - 1) as usize),
                    };

                    if s <= e && s < bytes.len() {
                        let e_bounded = e.min(bytes.len() - 1);
                        return bitcount_slice(&bytes[s..=e_bounded]);
                    }
                }
            }
        }
        0
    }

    // -------------------------------------------------------------------------
    // STREAM OPERATIONS (XADD, XREAD, XRANGE, XLEN)
    // -------------------------------------------------------------------------

    /// Appends a new entry to the stream stored at key.
    pub fn xadd(
        &self,
        key: &[u8],
        id_req: Option<StreamId>,
        fields: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<StreamId, &'static str> {
        let now = Instant::now();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let resolve_id = |last: Option<StreamId>, req: Option<StreamId>| -> Result<StreamId, &'static str> {
            match req {
                None => {
                    match last {
                        None => Ok(StreamId::new(now_ms, 0)),
                        Some(last_id) => {
                            if now_ms > last_id.ms {
                                Ok(StreamId::new(now_ms, 0))
                            } else {
                                Ok(StreamId::new(last_id.ms, last_id.seq + 1))
                            }
                        }
                    }
                }
                Some(req_id) if req_id.seq == u64::MAX => {
                    match last {
                        None => Ok(StreamId::new(req_id.ms, if req_id.ms == 0 { 1 } else { 0 })),
                        Some(last_id) => {
                            if req_id.ms < last_id.ms {
                                Err("ERR The ID specified in XADD is equal or smaller than the target stream top item")
                            } else if req_id.ms == last_id.ms {
                                Ok(StreamId::new(req_id.ms, last_id.seq + 1))
                            } else {
                                Ok(StreamId::new(req_id.ms, 0))
                            }
                        }
                    }
                }
                Some(req_id) => {
                    if req_id.ms == 0 && req_id.seq == 0 {
                        return Err("ERR The ID specified in XADD must be greater than 0-0");
                    }
                    if let Some(last_id) = last {
                        if req_id <= last_id {
                            return Err("ERR The ID specified in XADD is equal or smaller than the target stream top item");
                        }
                    }
                    Ok(req_id)
                }
            }
        };

        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
            if is_expired {
                inner.free_slot(idx);
                inner.map.remove(key);
            } else {
                let (final_id, added_bytes) = {
                    let node = inner.nodes[idx].as_mut().unwrap();
                    match &mut node.entry.value {
                        CacheValue::Stream(ref mut stream_data) => {
                            let final_id = resolve_id(stream_data.last_id, id_req)?;
                            let added_bytes = 32 + fields.iter().map(|(f, v)| 32 + f.len() + v.len()).sum::<usize>();
                            stream_data.entries.push(StreamEntry { id: final_id, fields });
                            stream_data.last_id = Some(final_id);
                            node.size_bytes += added_bytes;
                            (final_id, added_bytes)
                        }
                        _ => return Err("WRONGTYPE Operation against a key holding the wrong kind of value"),
                    }
                };

                inner.current_memory += added_bytes;
                inner.touch_node(idx);
                return Ok(final_id);
            }
        }

        let final_id = resolve_id(None, id_req)?;
        let mut stream_data = StreamData::new();
        stream_data.entries.push(StreamEntry { id: final_id, fields });
        stream_data.last_id = Some(final_id);
        let val_payload = CacheValue::Stream(stream_data);
        let new_size = estimate_entry_bytes(key, &val_payload);
        if !inner.ensure_capacity(new_size) {
            return Err("OOM command not allowed when used memory > 'maxmemory'");
        }
        let entry = CacheEntry::with_deadline(val_payload, None);
        let slot = inner.allocate_slot(key.to_vec(), entry, new_size);
        inner.map.insert(key.to_vec(), slot);
        Ok(final_id)
    }

    /// Reads entries from one or multiple streams strictly newer than given start IDs.
    pub fn xread(&self, streams: &[(&[u8], StreamId)], count: Option<usize>) -> Vec<(Vec<u8>, Vec<StreamEntry>)> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();
        let mut results = Vec::new();

        for &(k, start_id) in streams {
            if let Some(&idx) = inner.map.get(k) {
                let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
                if is_expired {
                    inner.free_slot(idx);
                    inner.map.remove(k);
                    continue;
                }
                inner.touch_node(idx);
                if let Some(ref node) = inner.nodes[idx] {
                    if let CacheValue::Stream(ref s) = node.entry.value {
                        let entries = s.xread(start_id, count);
                        if !entries.is_empty() {
                            results.push((k.to_vec(), entries.into_iter().cloned().collect()));
                        }
                    }
                }
            }
        }
        results
    }

    /// Returns stream entries within the inclusive range [start, end].
    pub fn xrange(&self, key: &[u8], start: StreamId, end: StreamId, count: Option<usize>) -> Vec<StreamEntry> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
            if is_expired {
                inner.free_slot(idx);
                inner.map.remove(key);
                return Vec::new();
            }
            inner.touch_node(idx);
            if let Some(ref node) = inner.nodes[idx] {
                if let CacheValue::Stream(ref s) = node.entry.value {
                    return s.xrange(start, end, count).into_iter().cloned().collect();
                }
            }
        }
        Vec::new()
    }

    /// Returns the number of entries in the stream.
    pub fn xlen(&self, key: &[u8]) -> usize {
        let now = Instant::now();
        let inner = self.inner.borrow();
        if let Some(&idx) = inner.map.get(key) {
            let is_expired = inner.nodes[idx].as_ref().map(|n| n.entry.is_expired(now)).unwrap_or(true);
            if is_expired {
                return 0;
            }
            if let Some(ref node) = inner.nodes[idx] {
                if let CacheValue::Stream(ref s) = node.entry.value {
                    return s.len();
                }
            }
        }
        0
    }

    // -------------------------------------------------------------------------
    // GENERAL KEY MANAGEMENT & TTL
    // -------------------------------------------------------------------------

    /// Deletes a key from the cache regardless of its internal value type.
    pub fn del(&self, key: &[u8]) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();
        if let Some(idx) = inner.map.remove(key) {
            let was_expired = inner.nodes[idx]
                .as_ref()
                .map(|n| n.entry.is_expired(now))
                .unwrap_or(true);
            inner.free_slot(idx);
            !was_expired
        } else {
            false
        }
    }

    /// Updates the Time-to-Live (TTL) of an existing key.
    pub fn expire(&self, key: &[u8], ttl: Duration) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return false,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            false
        } else {
            if let Some(ref mut node) = inner.nodes[idx] {
                node.entry.expires_at = now.checked_add(ttl);
            }
            inner.touch_node(idx);
            true
        }
    }

    /// Returns the remaining TTL for `key`.
    pub fn ttl(&self, key: &[u8]) -> Option<Option<Duration>> {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return None,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            return None;
        }

        inner.touch_node(idx);

        if let Some(ref node) = inner.nodes[idx] {
            match node.entry.expires_at {
                Some(deadline) => Some(Some(deadline.saturating_duration_since(now))),
                None => Some(None),
            }
        } else {
            None
        }
    }

    /// Actively sweeps and purges all expired keys across the cache.
    pub fn purge_expired(&self) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();
        let mut expired_keys = Vec::new();

        for (key, &idx) in inner.map.iter() {
            if let Some(ref node) = inner.nodes[idx] {
                if node.entry.is_expired(now) {
                    expired_keys.push((key.clone(), idx));
                }
            }
        }

        let purged = expired_keys.len();
        for (key, idx) in expired_keys {
            inner.map.remove(&key);
            inner.free_slot(idx);
        }

        purged
    }

    /// Returns `true` if the cache contains the given `key` (and it is not expired).
    pub fn contains_key(&self, key: &[u8]) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.borrow_mut();

        let idx = match inner.map.get(key) {
            Some(&i) => i,
            None => return false,
        };

        let is_expired = inner.nodes[idx]
            .as_ref()
            .map(|n| n.entry.is_expired(now))
            .unwrap_or(false);

        if is_expired {
            inner.map.remove(key);
            inner.free_slot(idx);
            false
        } else {
            true
        }
    }

    /// Total number of keys currently residing in memory.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.borrow().map.len()
    }

    /// Counts non-expired keys in the cache that hash to the specified cluster slot.
    pub fn count_keys_in_slot(&self, slot: u16) -> usize {
        let now = Instant::now();
        let inner = self.inner.borrow();
        let mut count = 0;
        for (key, &idx) in &inner.map {
            if let Some(node) = &inner.nodes[idx] {
                if !node.entry.is_expired(now) && key_slot(key) == slot {
                    count += 1;
                }
            }
        }
        count
    }

    /// Returns `true` if the cache holds zero entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().map.is_empty()
    }

    /// Removes all keys and values from the cache.
    pub fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.map.clear();
        inner.nodes.clear();
        inner.free_slots.clear();
        inner.lru_head = None;
        inner.lru_tail = None;
        inner.current_memory = 0;
    }

    /// Iterates over all non-expired key-value entries in the cache without heap allocation.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&[u8], &CacheValue, Option<Duration>),
    {
        let now = Instant::now();
        let inner = self.inner.borrow();
        for &idx in inner.map.values() {
            if let Some(ref node) = inner.nodes[idx] {
                if !node.entry.is_expired(now) {
                    let ttl = node.entry.expires_at.map(|exp| exp.saturating_duration_since(now));
                    f(&node.key, &node.entry.value, ttl);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_set_and_get() {
        let cache = LocalCache::new();
        cache.set(b"foo", b"bar");

        assert_eq!(cache.get(b"foo"), Some(b"bar".to_vec()));
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn test_del_existing() {
        let cache = LocalCache::new();
        cache.set(b"key1", b"val1");

        assert!(cache.del(b"key1"));
        assert_eq!(cache.get(b"key1"), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_del_nonexistent() {
        let cache = LocalCache::new();
        assert!(!cache.del(b"missing"));
    }

    #[test]
    fn test_set_with_ttl_lazy_expiration() {
        let cache = LocalCache::new();
        cache.set_with_ttl(b"short", b"val", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));

        assert_eq!(cache.get(b"short"), None);
        assert!(!cache.contains_key(b"short"));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_del_expired_returns_false() {
        let cache = LocalCache::new();
        cache.set_with_ttl(b"expired", b"val", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));

        assert!(!cache.del(b"expired"));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_expire_method() {
        let cache = LocalCache::new();
        cache.set(b"persist", b"data");
        assert_eq!(cache.ttl(b"persist"), Some(None));

        assert!(cache.expire(b"persist", Duration::from_secs(120)));
        match cache.ttl(b"persist") {
            Some(Some(rem)) => assert!(rem > Duration::from_secs(60)),
            other => panic!("Expected Some(Some(duration)), got {other:?}"),
        }
    }

    #[test]
    fn test_purge_expired() {
        let cache = LocalCache::new();
        cache.set(b"permanent", b"p");
        cache.set_with_ttl(b"exp1", b"e1", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));

        let purged = cache.purge_expired();
        assert_eq!(purged, 1);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(b"permanent"), Some(b"p".to_vec()));
    }

    #[test]
    fn test_hash_operations() {
        let cache = LocalCache::new();
        assert!(cache.hset(b"user:100", b"name", b"alice"));
        assert!(!cache.hset(b"user:100", b"name", b"bob"));
        assert!(cache.hset(b"user:100", b"role", b"admin"));

        assert_eq!(cache.hget(b"user:100", b"name"), Some(b"bob".to_vec()));
        assert_eq!(cache.hget(b"user:100", b"role"), Some(b"admin".to_vec()));
        assert_eq!(cache.hlen(b"user:100"), 2);
        assert!(cache.hexists(b"user:100", b"name"));

        assert!(cache.hdel(b"user:100", b"role"));
        assert_eq!(cache.hlen(b"user:100"), 1);

        assert!(cache.hdel(b"user:100", b"name"));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_list_operations() {
        let cache = LocalCache::new();
        assert_eq!(cache.rpush(b"queue", b"task1"), 1);
        assert_eq!(cache.rpush(b"queue", b"task2"), 2);
        assert_eq!(cache.lpush(b"queue", b"task0"), 3);

        assert_eq!(cache.llen(b"queue"), 3);
        assert_eq!(cache.lindex(b"queue", 0), Some(b"task0".to_vec()));
        assert_eq!(cache.lindex(b"queue", -1), Some(b"task2".to_vec()));

        let range = cache.lrange(b"queue", 0, 1);
        assert_eq!(range, vec![b"task0".to_vec(), b"task1".to_vec()]);

        assert_eq!(cache.lpop(b"queue"), Some(b"task0".to_vec()));
        assert_eq!(cache.rpop(b"queue"), Some(b"task2".to_vec()));
        assert_eq!(cache.lpop(b"queue"), Some(b"task1".to_vec()));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_zset_operations() {
        let cache = LocalCache::new();
        assert!(cache.zadd(b"leaders", 100.0, b"alice"));
        assert!(cache.zadd(b"leaders", 200.0, b"bob"));
        assert!(cache.zadd(b"leaders", 150.0, b"charlie"));
        assert!(!cache.zadd(b"leaders", 250.0, b"alice"));

        assert_eq!(cache.zcard(b"leaders"), 3);
        assert_eq!(cache.zscore(b"leaders", b"alice"), Some(250.0));

        let ascending = cache.zrange(b"leaders", 0, -1);
        assert_eq!(
            ascending,
            vec![
                (b"charlie".to_vec(), 150.0),
                (b"bob".to_vec(), 200.0),
                (b"alice".to_vec(), 250.0)
            ]
        );

        let by_score = cache.zrangebyscore(b"leaders", 160.0, 300.0);
        assert_eq!(
            by_score,
            vec![(b"bob".to_vec(), 200.0), (b"alice".to_vec(), 250.0)]
        );

        assert!(cache.zrem(b"leaders", b"bob"));
        assert_eq!(cache.zcard(b"leaders"), 2);
    }

    #[test]
    fn test_zset_lexicographical_tie_break() {
        let cache = LocalCache::new();
        cache.zadd(b"ties", 10.0, b"zebra");
        cache.zadd(b"ties", 10.0, b"alpha");
        cache.zadd(b"ties", 10.0, b"mango");

        let range = cache.zrange(b"ties", 0, -1);
        assert_eq!(
            range,
            vec![
                (b"alpha".to_vec(), 10.0),
                (b"mango".to_vec(), 10.0),
                (b"zebra".to_vec(), 10.0)
            ]
        );
    }

    #[test]
    fn test_ttl_on_advanced_types() {
        let cache = LocalCache::new();
        cache.hset(b"temp_hash", b"k", b"v");
        assert!(cache.expire(b"temp_hash", Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(10));

        assert_eq!(cache.hget(b"temp_hash", b"k"), None);
        assert_eq!(cache.len(), 0);
    }

    // -------------------------------------------------------------------------
    // ENTERPRISE MEMORY EVICTION TESTS (LRU / LFU)
    // -------------------------------------------------------------------------

    #[test]
    fn test_lru_eviction_strict_order() {
        let cache = LocalCache::new();
        cache.set_max_entries(3);
        cache.set_eviction_policy(EvictionPolicy::AllKeysLru);

        cache.set(b"key1", b"val1");
        cache.set(b"key2", b"val2");
        cache.set(b"key3", b"val3");
        assert_eq!(cache.len(), 3);

        // Access key1 to move it to MRU head
        assert_eq!(cache.get(b"key1"), Some(b"val1".to_vec()));

        // Insert key4: key2 was the least recently used, so key2 must be evicted!
        cache.set(b"key4", b"val4");
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.evictions_count(), 1);

        assert_eq!(cache.get(b"key2"), None); // Evicted!
        assert_eq!(cache.get(b"key1"), Some(b"val1".to_vec())); // Kept!
        assert_eq!(cache.get(b"key3"), Some(b"val3".to_vec())); // Kept!
        assert_eq!(cache.get(b"key4"), Some(b"val4".to_vec())); // Kept!
    }

    #[test]
    fn test_lfu_eviction_frequency_order() {
        let cache = LocalCache::new();
        cache.set_max_entries(3);
        cache.set_eviction_policy(EvictionPolicy::AllKeysLfu);

        cache.set(b"k1", b"v1");
        cache.set(b"k2", b"v2");
        cache.set(b"k3", b"v3");

        // Bump access count of k1 and k3
        for _ in 0..5 {
            let _ = cache.get(b"k1");
        }
        for _ in 0..3 {
            let _ = cache.get(b"k3");
        }
        // k2 has only 1 access (initial set)

        // Insert k4: k2 must be evicted as least frequently used
        cache.set(b"k4", b"v4");
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.evictions_count(), 1);

        assert_eq!(cache.get(b"k2"), None); // Evicted!
        assert_eq!(cache.get(b"k1"), Some(b"v1".to_vec()));
        assert_eq!(cache.get(b"k3"), Some(b"v3".to_vec()));
        assert_eq!(cache.get(b"k4"), Some(b"v4".to_vec()));
    }

    #[test]
    fn test_max_memory_byte_threshold_eviction() {
        let cache = LocalCache::new();
        // Set maximum memory limit to 500 bytes
        cache.set_max_memory(500);
        cache.set_eviction_policy(EvictionPolicy::AllKeysLru);

        // Insert entries with 100-byte values
        let val100 = vec![b'x'; 100];
        cache.set(b"mem_key1", &val100);
        cache.set(b"mem_key2", &val100);

        let mem_usage = cache.memory_usage();
        assert!(mem_usage > 0 && mem_usage <= 500);

        // Insert more keys to force synchronous byte-level eviction
        cache.set(b"mem_key3", &val100);
        cache.set(b"mem_key4", &val100);
        cache.set(b"mem_key5", &val100);

        assert!(cache.memory_usage() <= 500);
        assert!(cache.evictions_count() > 0);
    }

    #[test]
    fn test_no_eviction_policy_blocks_on_full() {
        let cache = LocalCache::new();
        cache.set_max_entries(2);
        cache.set_eviction_policy(EvictionPolicy::NoEviction);

        cache.set(b"a", b"1");
        cache.set(b"b", b"2");
        assert_eq!(cache.len(), 2);

        // Insertion of 3rd key must be rejected under NoEviction
        cache.set(b"c", b"3");
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(b"c"), None);
        assert_eq!(cache.evictions_count(), 0);
    }

    #[test]
    fn test_zero_allocation_on_repeated_get() {
        let cache = LocalCache::new();
        cache.set_max_entries(10);
        cache.set(b"static_key", b"static_val");

        // Repeated GET operations update the intrusive doubly-linked list in place
        for _ in 0..10_000 {
            assert_eq!(cache.get(b"static_key"), Some(b"static_val".to_vec()));
        }

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.evictions_count(), 0);
    }
}
