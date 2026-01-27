//! UTXO (Unspent Transaction Output) Cache Module
//!
//! Provides a sharded LRU cache with compact binary keys for recent transaction
//! outputs, dramatically improving ingestion performance by avoiding expensive
//! Neo4j graph traversals.
//!
//! # Performance Impact
//!
//! - **Before**: 3 expensive Neo4j queries per block (calculate amounts + simplified layer)
//! - **After**: In-memory cache lookups with ~1-5% Neo4j fallback for cache misses
//! - **Expected speedup**: 10-100x for ingestion pipeline
//!
//! # Cache Strategy
//!
//! - 16-shard LRU with lock-free atomic statistics
//! - Compact 36-byte binary keys (UtxoKey) — zero allocation on lookup
//! - Configurable capacity (default: 100,000 entries ≈ 7MB)
//! - Neo4j fallback for cache misses (dormant UTXOs)
//! - Exploit temporal locality: most inputs spend recent outputs

mod cache;

pub use cache::{UtxoCache, UtxoKey, CachedOutput, ScriptTypeTag, UtxoCacheStats};
