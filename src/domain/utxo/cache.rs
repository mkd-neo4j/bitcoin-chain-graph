//! UTXO Cache Implementation
//!
//! LRU-based cache for transaction outputs with Neo4j fallback on cache misses.
//! This cache dramatically improves ingestion performance by keeping recent
//! outputs in memory and avoiding expensive database queries.

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use crate::writer::{GraphWriter, Result};

/// Simplified output data stored in cache
///
/// Contains only the essential fields needed for SPENDS relationship
/// creation and amount calculations. Optimized for minimal memory footprint.
#[derive(Clone, Debug)]
pub struct CachedOutput {
    /// Output identifier in format "txid:index"
    pub output_id: String,
    /// Output index within transaction
    pub output_index: u32,
    /// Amount in satoshis
    pub amount: u64,
    /// Script type (P2PKH, P2SH, etc.)
    pub script_type: String,
    /// Bitcoin address (if derivable from script)
    pub address: Option<String>,
}

/// Cache statistics for monitoring performance
#[derive(Debug, Clone, Default)]
pub struct UtxoCacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses (required Neo4j fallback)
    pub misses: u64,
    /// Number of inserts
    pub inserts: u64,
    /// Number of removals (spent outputs)
    pub removals: u64,
}

impl UtxoCacheStats {
    /// Calculate cache hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Calculate cache hit rate as percentage (0.0 to 100.0)
    pub fn hit_rate_percent(&self) -> f64 {
        self.hit_rate() * 100.0
    }
}

/// UTXO Cache with LRU eviction and Neo4j fallback
///
/// # Memory Usage
///
/// Each cached output is approximately 138 bytes:
/// - output_id: ~70 bytes (txid:index string)
/// - amount: 8 bytes (u64)
/// - script_type: ~10 bytes (string)
/// - address: ~35 bytes (Option<String>)
/// - LRU metadata: ~15 bytes
///
/// Total memory usage = capacity × 138 bytes:
/// - 100,000 entries ≈ 15 MB
/// - 1,000,000 entries ≈ 150 MB
/// - 10,000,000 entries ≈ 1.5 GB
///
/// # Example
///
/// ```no_run
/// use bitcoin_chain_graph::domain::utxo::{UtxoCache, CachedOutput};
/// use bitcoin_chain_graph::writer::MockWriter;
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() {
///     let writer = Arc::new(MockWriter::new());
///     let mut cache = UtxoCache::new(100_000, writer);
///
///     // Insert output into cache
///     let output = CachedOutput {
///         output_id: "tx1:0".to_string(),
///         output_index: 0,
///         amount: 5000000000,
///         script_type: "P2PKH".to_string(),
///         address: Some("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa".to_string()),
///     };
///     cache.insert(output);
///
///     // Lookup output (cache hit)
///     let retrieved = cache.get("tx1:0").await.unwrap();
///     assert_eq!(retrieved.amount, 5000000000);
///
///     // View statistics
///     let stats = cache.stats();
///     println!("Hit rate: {:.2}%", stats.hit_rate_percent());
/// }
/// ```
pub struct UtxoCache<W: GraphWriter> {
    /// LRU cache for output data
    cache: Arc<Mutex<LruCache<String, CachedOutput>>>,
    /// GraphWriter for Neo4j fallback on cache miss
    writer: Arc<W>,
    /// Cache statistics
    stats: Arc<Mutex<UtxoCacheStats>>,
    /// Pre-warming mode flag (prevents eviction during backward loading)
    prewarm_mode: Arc<Mutex<bool>>,
}

impl<W: GraphWriter> UtxoCache<W> {
    /// Create a new UTXO cache with specified capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of outputs to cache (LRU evicts oldest)
    /// * `writer` - GraphWriter implementation for Neo4j fallback (will be Arc-wrapped internally)
    ///
    /// # Panics
    ///
    /// Panics if capacity is 0
    pub fn new(capacity: usize, writer: Arc<W>) -> Self {
        assert!(capacity > 0, "UTXO cache capacity must be > 0");

        Self {
            cache: Arc::new(Mutex::new(
                LruCache::new(NonZeroUsize::new(capacity).unwrap())
            )),
            writer,
            stats: Arc::new(Mutex::new(UtxoCacheStats::default())),
            prewarm_mode: Arc::new(Mutex::new(false)),
        }
    }

    /// Insert output into cache
    ///
    /// If cache is full, evicts the least recently used entry.
    ///
    /// # Arguments
    ///
    /// * `output` - Cached output data to insert
    pub fn insert(&self, output: CachedOutput) {
        let output_id = output.output_id.clone();
        let mut cache = self.cache.lock().unwrap();
        cache.put(output_id, output);

        let mut stats = self.stats.lock().unwrap();
        stats.inserts += 1;
    }

    /// Lookup output by ID (cache-first, then Neo4j fallback)
    ///
    /// # Arguments
    ///
    /// * `output_id` - Output identifier in format "txid:index"
    ///
    /// # Returns
    ///
    /// CachedOutput if found in cache or Neo4j
    ///
    /// # Errors
    ///
    /// Returns WriterError::OutputNotFound if output doesn't exist in cache or database
    pub async fn get(&self, output_id: &str) -> Result<CachedOutput> {
        // Try cache first
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(output) = cache.get(output_id) {
                let mut stats = self.stats.lock().unwrap();
                stats.hits += 1;
                return Ok(output.clone());
            }
        }

        // Cache miss - query Neo4j
        {
            let mut stats = self.stats.lock().unwrap();
            stats.misses += 1;
        }

        let output = self.fetch_from_neo4j(output_id).await?;

        // Insert into cache for future lookups
        self.insert(output.clone());

        Ok(output)
    }

    /// Fetch output from Neo4j (fallback for cache misses)
    async fn fetch_from_neo4j(&self, output_id: &str) -> Result<CachedOutput> {
        let output_data = self.writer.lookup_output(output_id).await?;

        Ok(CachedOutput {
            output_id: output_data.output_id,
            output_index: output_data.output_index,
            amount: output_data.amount,
            script_type: output_data.script_type,
            address: output_data.address,
        })
    }

    /// Remove output from cache (mark as spent)
    ///
    /// Outputs are removed from cache when they're spent, as they're no
    /// longer in the UTXO set. This keeps the cache focused on unspent outputs.
    ///
    /// # Arguments
    ///
    /// * `output_id` - Output identifier to remove
    ///
    /// # Returns
    ///
    /// true if output was in cache and removed, false if not in cache
    pub fn remove(&self, output_id: &str) -> bool {
        let mut cache = self.cache.lock().unwrap();
        let removed = cache.pop(output_id).is_some();

        if removed {
            let mut stats = self.stats.lock().unwrap();
            stats.removals += 1;
        }

        removed
    }

    /// Get cache statistics
    ///
    /// Returns a snapshot of cache performance metrics including hits, misses,
    /// and hit rate.
    pub fn stats(&self) -> UtxoCacheStats {
        let stats = self.stats.lock().unwrap();
        stats.clone()
    }

    /// Clear all cache statistics (useful for testing)
    #[allow(dead_code)]
    pub fn reset_stats(&self) {
        let mut stats = self.stats.lock().unwrap();
        *stats = UtxoCacheStats::default();
    }

    /// Get current number of entries in cache
    pub fn len(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get cache capacity
    pub fn capacity(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.cap().get()
    }

    /// Enable pre-warming mode
    ///
    /// In pre-warming mode, inserts using `insert_prewarm()` will stop
    /// when the cache is full, preventing eviction of existing entries.
    /// This is used during backward block loading to populate the cache
    /// without evicting recently added entries.
    pub fn enable_prewarm_mode(&self) {
        let mut mode = self.prewarm_mode.lock().unwrap();
        *mode = true;
    }

    /// Disable pre-warming mode and return to normal LRU operation
    ///
    /// After pre-warming is complete, call this to resume normal LRU
    /// eviction behavior where new inserts can evict old entries.
    pub fn disable_prewarm_mode(&self) {
        let mut mode = self.prewarm_mode.lock().unwrap();
        *mode = false;
    }

    /// Insert output during pre-warming (stops when cache is full)
    ///
    /// Unlike regular `insert()`, this method will not evict existing
    /// entries if the cache is full. Instead, it returns `false` to
    /// indicate pre-warming should stop.
    ///
    /// # Arguments
    ///
    /// * `output` - Cached output data to insert
    ///
    /// # Returns
    ///
    /// * `true` - Output was inserted successfully
    /// * `false` - Cache is full, pre-warming should stop
    pub fn insert_prewarm(&self, output: CachedOutput) -> bool {
        let mut cache = self.cache.lock().unwrap();

        // Check if cache is full before inserting
        if cache.len() >= cache.cap().get() {
            return false; // Stop pre-warming
        }

        let output_id = output.output_id.clone();
        cache.put(output_id, output);

        let mut stats = self.stats.lock().unwrap();
        stats.inserts += 1;

        true // Continue pre-warming
    }

    /// Check if cache has capacity for more entries
    ///
    /// Returns `true` if cache is not full and can accept more entries
    /// without evicting existing ones.
    pub fn has_capacity(&self) -> bool {
        let cache = self.cache.lock().unwrap();
        cache.len() < cache.cap().get()
    }

    /// Get cache fill percentage (0.0 to 1.0)
    ///
    /// Returns the ratio of current entries to maximum capacity.
    /// - 0.0 = empty
    /// - 0.5 = half full
    /// - 1.0 = completely full
    pub fn fill_percentage(&self) -> f64 {
        let cache = self.cache.lock().unwrap();
        cache.len() as f64 / cache.cap().get() as f64
    }
}

impl<W: GraphWriter> Clone for UtxoCache<W> {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            writer: Arc::clone(&self.writer),
            stats: Arc::clone(&self.stats),
            prewarm_mode: Arc::clone(&self.prewarm_mode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{MockWriter, WriterError};

    #[tokio::test]
    async fn test_create_cache() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, writer);

        assert_eq!(cache.capacity(), 100);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, writer);

        let output = CachedOutput {
            output_id: "tx1:0".to_string(),
            output_index: 0,
            amount: 5000000000,
            script_type: "P2PKH".to_string(),
            address: Some("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa".to_string()),
        };

        cache.insert(output.clone());
        assert_eq!(cache.len(), 1);

        let retrieved = cache.get("tx1:0").await.unwrap();
        assert_eq!(retrieved.output_id, "tx1:0");
        assert_eq!(retrieved.amount, 5000000000);
    }

    #[tokio::test]
    async fn test_cache_hit_stats() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, writer);

        let output = CachedOutput {
            output_id: "tx1:0".to_string(),
            output_index: 0,
            amount: 5000000000,
            script_type: "P2PKH".to_string(),
            address: None,
        };

        cache.insert(output);

        // First get - cache hit
        cache.get("tx1:0").await.unwrap();

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.hit_rate(), 1.0);
        assert_eq!(stats.hit_rate_percent(), 100.0);
    }

    #[tokio::test]
    async fn test_remove() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, writer);

        let output = CachedOutput {
            output_id: "tx1:0".to_string(),
            output_index: 0,
            amount: 5000000000,
            script_type: "P2PKH".to_string(),
            address: None,
        };

        cache.insert(output);
        assert_eq!(cache.len(), 1);

        let removed = cache.remove("tx1:0");
        assert!(removed);
        assert_eq!(cache.len(), 0);

        let stats = cache.stats();
        assert_eq!(stats.removals, 1);
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(2, writer); // Small capacity for testing

        let output1 = CachedOutput {
            output_id: "tx1:0".to_string(),
            output_index: 0,
            amount: 100,
            script_type: "P2PKH".to_string(),
            address: None,
        };

        let output2 = CachedOutput {
            output_id: "tx2:0".to_string(),
            output_index: 0,
            amount: 200,
            script_type: "P2PKH".to_string(),
            address: None,
        };

        let output3 = CachedOutput {
            output_id: "tx3:0".to_string(),
            output_index: 0,
            amount: 300,
            script_type: "P2PKH".to_string(),
            address: None,
        };

        cache.insert(output1);
        cache.insert(output2);
        assert_eq!(cache.len(), 2);

        // Access output1 to make it more recently used
        cache.get("tx1:0").await.unwrap();

        // Insert output3 - should evict output2 (least recently used)
        cache.insert(output3);
        assert_eq!(cache.len(), 2);

        // output1 and output3 should be in cache, output2 should be evicted
        assert!(cache.get("tx1:0").await.is_ok());
        assert!(cache.get("tx3:0").await.is_ok());
    }

    #[tokio::test]
    async fn test_cache_miss_neo4j_fallback() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, Arc::clone(&writer));

        // Create an output and write it to MockWriter (simulating Neo4j)
        let output_data = crate::domain::OutputData {
            output_id: "fallback_tx:5".to_string(),
            output_index: 5,
            txid: "fallback_tx".to_string(),
            amount: 12345678,
            script_pubkey: "76a914...88ac".to_string(),
            script_type: "P2PKH".to_string(),
            address: Some("1FallbackAddress".to_string()),
        };
        writer.write_outputs(&[output_data.clone()]).await.unwrap();

        // Get from cache - should miss cache and query Neo4j
        let result = cache.get("fallback_tx:5").await.unwrap();

        // Verify correct data returned from Neo4j
        assert_eq!(result.output_id, "fallback_tx:5");
        assert_eq!(result.amount, 12345678);
        assert_eq!(result.script_type, "P2PKH");
        assert_eq!(result.address, Some("1FallbackAddress".to_string()));

        // Verify stats show 1 miss (Neo4j query) and 0 hits
        let stats = cache.stats();
        assert_eq!(stats.misses, 1, "Should have 1 cache miss");
        assert_eq!(stats.hits, 0, "Should have 0 cache hits on first access");

        // Second get - should now be cached (inserted after fallback)
        let result2 = cache.get("fallback_tx:5").await.unwrap();
        assert_eq!(result2.output_id, "fallback_tx:5");

        // Verify stats show cache hit on second access
        let stats2 = cache.stats();
        assert_eq!(stats2.misses, 1, "Should still have 1 miss");
        assert_eq!(stats2.hits, 1, "Should now have 1 cache hit after re-cache");
        assert_eq!(stats2.hit_rate(), 0.5, "Hit rate should be 50%");
    }

    #[tokio::test]
    async fn test_cache_statistics_comprehensive() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, Arc::clone(&writer));

        // Initial stats should be zero
        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.inserts, 0);
        assert_eq!(stats.removals, 0);
        assert_eq!(stats.hit_rate(), 0.0);

        // Insert 3 outputs
        cache.insert(CachedOutput {
            output_id: "tx1:0".to_string(),
            output_index: 0,
            amount: 100,
            script_type: "P2PKH".to_string(),
            address: None,
        });
        cache.insert(CachedOutput {
            output_id: "tx2:0".to_string(),
            output_index: 0,
            amount: 200,
            script_type: "P2PKH".to_string(),
            address: None,
        });
        cache.insert(CachedOutput {
            output_id: "tx3:0".to_string(),
            output_index: 0,
            amount: 300,
            script_type: "P2PKH".to_string(),
            address: None,
        });

        let stats = cache.stats();
        assert_eq!(stats.inserts, 3, "Should have 3 inserts");

        // 2 cache hits
        cache.get("tx1:0").await.unwrap();
        cache.get("tx2:0").await.unwrap();

        // 1 cache miss (output not in cache or Neo4j)
        let _ = cache.get("tx99:0").await; // Will fail but increment miss counter

        // Remove 1 output
        assert!(cache.remove("tx3:0"));

        // Final stats check
        let stats = cache.stats();
        assert_eq!(stats.hits, 2, "Should have 2 hits");
        assert_eq!(stats.misses, 1, "Should have 1 miss");
        assert_eq!(stats.inserts, 3, "Should have 3 inserts");
        assert_eq!(stats.removals, 1, "Should have 1 removal");

        // Use approximate equality for floating point
        let hit_rate = stats.hit_rate();
        assert!((hit_rate - 0.6666).abs() < 0.01, "Hit rate should be ~66.67%");

        let hit_rate_pct = stats.hit_rate_percent();
        assert!((hit_rate_pct - 66.666).abs() < 0.1, "Hit rate % should be ~66.67");
    }

    #[tokio::test]
    async fn test_cache_miss_output_not_found() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, writer);

        // Try to get output that doesn't exist in cache or MockWriter
        let result = cache.get("nonexistent:99").await;

        // Should return error
        assert!(result.is_err(), "Should return error for nonexistent output");

        // Verify it's the correct error type
        match result {
            Err(WriterError::OutputNotFound(msg)) => {
                assert!(msg.contains("nonexistent:99"), "Error should mention output ID");
            }
            _ => panic!("Expected OutputNotFound error"),
        }

        // Verify stats incremented miss counter
        let stats = cache.stats();
        assert_eq!(stats.misses, 1, "Should have 1 miss for failed lookup");
        assert_eq!(stats.hits, 0, "Should have 0 hits");

        // Verify cache doesn't contain the failed lookup
        assert_eq!(cache.len(), 0, "Cache should still be empty after failed lookup");
    }

    #[tokio::test]
    async fn test_large_transaction_many_inputs() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(1000, Arc::clone(&writer));

        // Create 150 outputs and insert into cache and MockWriter
        let mut output_data_batch = vec![];
        for i in 0..150 {
            let output = CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: 10000000,
                script_type: "P2PKH".to_string(),
                address: Some(format!("addr_{}", i)),
            };
            cache.insert(output.clone());

            // Also write to MockWriter for potential fallback
            let output_data = crate::domain::OutputData {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                txid: format!("tx{}", i),
                amount: 10000000,
                script_pubkey: "76a914...88ac".to_string(),
                script_type: "P2PKH".to_string(),
                address: Some(format!("addr_{}", i)),
            };
            output_data_batch.push(output_data);
        }
        writer.write_outputs(&output_data_batch).await.unwrap();

        // Simulate a large transaction spending all 150 outputs
        let start = std::time::Instant::now();
        let mut total_amount = 0u64;

        for i in 0..150 {
            let output = cache.get(&format!("tx{}:0", i)).await.unwrap();
            total_amount += output.amount;
        }

        let duration = start.elapsed();

        // Verify all lookups succeeded
        assert_eq!(total_amount, 150 * 10000000, "Should calculate correct total");

        // Verify all were cache hits (no Neo4j queries)
        let stats = cache.stats();
        assert_eq!(stats.hits, 150, "All lookups should be cache hits");
        assert_eq!(stats.misses, 0, "Should have zero misses");

        // Performance assertion: 150 cache lookups should be very fast
        println!("✅ 150 cache lookups completed in {:?}", duration);
        assert!(duration.as_millis() < 10, "Cache lookups should be <10ms");
    }

    #[tokio::test]
    async fn test_concurrent_cache_access() {
        let writer = Arc::new(MockWriter::new());
        let cache = Arc::new(UtxoCache::new(1000, writer));

        // Pre-populate cache with 100 outputs
        for i in 0..100 {
            cache.insert(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: (i as u64) * 1000000,
                script_type: "P2PKH".to_string(),
                address: Some(format!("addr_{}", i)),
            });
        }

        // Spawn 10 concurrent tasks all reading from cache
        let mut handles = vec![];

        for task_id in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = tokio::spawn(async move {
                let mut sum = 0u64;
                // Each task reads 50 outputs (with overlap for contention)
                for i in 0..50 {
                    let output_id = format!("tx{}:0", (task_id * 5 + i) % 100);
                    if let Ok(output) = cache_clone.get(&output_id).await {
                        sum += output.amount;
                    }
                }
                sum
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        let mut total_sum = 0u64;
        for handle in handles {
            let sum = handle.await.unwrap();
            total_sum += sum;
        }

        // Verify no panics occurred and results are correct
        assert!(total_sum > 0, "Concurrent reads should succeed");

        // Verify stats tracked all operations correctly
        let stats = cache.stats();
        println!("✅ Concurrent access: hits={}, misses={}, total_ops={}",
                 stats.hits, stats.misses, stats.hits + stats.misses);
        assert_eq!(stats.hits + stats.misses, 500, "Should have 500 total lookups (10 tasks * 50 each)");
        assert_eq!(stats.hits, 500, "All should be cache hits (no concurrency issues)");
    }

    #[tokio::test]
    async fn test_cache_eviction_under_pressure() {
        let writer = Arc::new(MockWriter::new());
        let cache_size = 1000;
        let cache = UtxoCache::new(cache_size, writer);

        // Insert 5000 outputs (5x cache capacity) to force evictions
        println!("Inserting 5000 outputs into cache with capacity {}", cache_size);

        for i in 0..5000 {
            cache.insert(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: 10000000,
                script_type: "P2PKH".to_string(),
                address: None,
            });

            // Verify cache never exceeds capacity
            assert!(cache.len() <= cache_size,
                    "Cache size {} should never exceed capacity {}",
                    cache.len(), cache_size);
        }

        // Verify cache is at max capacity
        assert_eq!(cache.len(), cache_size, "Cache should be at max capacity");

        // Verify eviction stats
        let stats = cache.stats();
        assert_eq!(stats.inserts, 5000, "Should have 5000 inserts");

        // Most recent outputs (4900-4999) should still be in cache (LRU)
        let mut hits = 0;
        for i in 4900..5000 {
            if cache.get(&format!("tx{}:0", i)).await.is_ok() {
                hits += 1;
            }
        }

        assert!(hits >= 95, "At least 95% of last 100 outputs should be in cache (LRU eviction)");

        println!("✅ Successfully handled 5x capacity: {} inserts into {} capacity cache",
                 stats.inserts, cache_size);
        println!("   Final cache size: {}", cache.len());
        println!("   Recent output retention: {}/100", hits);
    }

    #[tokio::test]
    async fn test_cache_with_minimal_capacity() {
        let writer = Arc::new(MockWriter::new());

        // Edge case: cache size = 1 (minimum useful capacity)
        let cache = UtxoCache::new(1, Arc::clone(&writer));

        assert_eq!(cache.capacity(), 1);
        assert_eq!(cache.len(), 0);

        // Write 10 outputs to MockWriter for fallback
        let mut outputs = vec![];
        for i in 0..10 {
            outputs.push(crate::domain::OutputData {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                txid: format!("tx{}", i),
                amount: (i as u64 + 1) * 1000000,
                script_pubkey: "76a914...88ac".to_string(),
                script_type: "P2PKH".to_string(),
                address: Some(format!("addr_{}", i)),
            });
        }
        writer.write_outputs(&outputs).await.unwrap();

        // Insert output 0
        cache.insert(CachedOutput {
            output_id: "tx0:0".to_string(),
            output_index: 0,
            amount: 1000000,
            script_type: "P2PKH".to_string(),
            address: Some("addr_0".to_string()),
        });

        assert_eq!(cache.len(), 1);

        // Insert output 1 - should evict output 0 immediately
        cache.insert(CachedOutput {
            output_id: "tx1:0".to_string(),
            output_index: 0,
            amount: 2000000,
            script_type: "P2PKH".to_string(),
            address: Some("addr_1".to_string()),
        });

        assert_eq!(cache.len(), 1, "Cache should still have exactly 1 entry");

        // Get output 1 - should be cache hit
        let result1 = cache.get("tx1:0").await.unwrap();
        assert_eq!(result1.amount, 2000000);

        // Get output 0 - should be cache miss, fallback to Neo4j/MockWriter
        let result0 = cache.get("tx0:0").await.unwrap();
        assert_eq!(result0.amount, 1000000);

        // Verify stats show correct hit/miss pattern
        let stats = cache.stats();
        assert_eq!(stats.hits, 1, "Should have 1 hit (tx1)");
        assert_eq!(stats.misses, 1, "Should have 1 miss (tx0, evicted)");
        assert_eq!(stats.inserts, 3, "2 manual inserts + 1 from fallback re-cache");

        println!("✅ Cache with capacity=1 functions correctly");
        println!("   Eviction working: ✓");
        println!("   Fallback to Neo4j: ✓");
        println!("   Hit rate: {:.1}%", stats.hit_rate_percent());
    }

    #[tokio::test]
    async fn test_prewarm_mode_basic() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(10, writer); // Small capacity for testing

        // Enable pre-warming mode
        cache.enable_prewarm_mode();

        // Insert 10 outputs (fill cache exactly)
        for i in 0..10 {
            let result = cache.insert_prewarm(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: (i as u64) * 1000000,
                script_type: "P2PKH".to_string(),
                address: Some(format!("addr_{}", i)),
            });
            assert!(result, "Insert {} should succeed", i);
        }

        assert_eq!(cache.len(), 10, "Cache should be full");
        assert!(!cache.has_capacity(), "Cache should have no capacity");
        assert_eq!(cache.fill_percentage(), 1.0, "Cache should be 100% full");

        // Try to insert 11th output - should fail (cache full)
        let result = cache.insert_prewarm(CachedOutput {
            output_id: "tx10:0".to_string(),
            output_index: 0,
            amount: 10000000,
            script_type: "P2PKH".to_string(),
            address: Some("addr_10".to_string()),
        });

        assert!(!result, "Insert should fail when cache is full in prewarm mode");
        assert_eq!(cache.len(), 10, "Cache should still have 10 entries");

        // Disable pre-warming mode
        cache.disable_prewarm_mode();

        // Now regular insert should work (with eviction)
        cache.insert(CachedOutput {
            output_id: "tx11:0".to_string(),
            output_index: 0,
            amount: 11000000,
            script_type: "P2PKH".to_string(),
            address: Some("addr_11".to_string()),
        });

        assert_eq!(cache.len(), 10, "Cache should still have 10 entries (with eviction)");

        // Verify tx11 is in cache (newest)
        assert!(cache.get("tx11:0").await.is_ok(), "tx11 should be in cache");
    }

    #[tokio::test]
    async fn test_fill_percentage_tracking() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(100, writer);

        // Empty cache
        assert_eq!(cache.fill_percentage(), 0.0);
        assert!(cache.has_capacity());

        // 25% full
        for i in 0..25 {
            cache.insert(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: 1000000,
                script_type: "P2PKH".to_string(),
                address: None,
            });
        }
        assert_eq!(cache.fill_percentage(), 0.25);
        assert!(cache.has_capacity());

        // 50% full
        for i in 25..50 {
            cache.insert(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: 1000000,
                script_type: "P2PKH".to_string(),
                address: None,
            });
        }
        assert_eq!(cache.fill_percentage(), 0.5);
        assert!(cache.has_capacity());

        // 100% full
        for i in 50..100 {
            cache.insert(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: 1000000,
                script_type: "P2PKH".to_string(),
                address: None,
            });
        }
        assert_eq!(cache.fill_percentage(), 1.0);
        assert!(!cache.has_capacity());
    }

    #[tokio::test]
    async fn test_prewarm_stops_when_full() {
        let writer = Arc::new(MockWriter::new());
        let cache = UtxoCache::new(50, writer);

        cache.enable_prewarm_mode();

        // Insert outputs until cache is full
        let mut inserted = 0;
        for i in 0..1000 {
            let result = cache.insert_prewarm(CachedOutput {
                output_id: format!("tx{}:0", i),
                output_index: 0,
                amount: 1000000,
                script_type: "P2PKH".to_string(),
                address: None,
            });

            if result {
                inserted += 1;
            } else {
                // Cache is full, pre-warming should stop
                break;
            }
        }

        assert_eq!(inserted, 50, "Should insert exactly 50 outputs (cache capacity)");
        assert_eq!(cache.len(), 50, "Cache should be full");
        assert!(!cache.has_capacity(), "Cache should have no capacity");
    }
}
