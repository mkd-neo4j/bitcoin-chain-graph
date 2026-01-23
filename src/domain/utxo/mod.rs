//! UTXO (Unspent Transaction Output) Cache Module
//!
//! Provides an LRU-based cache for recent transaction outputs to dramatically
//! improve ingestion performance by avoiding expensive Neo4j graph traversals.
//!
//! # Performance Impact
//!
//! - **Before**: 3 expensive Neo4j queries per block (calculate amounts + simplified layer)
//! - **After**: In-memory cache lookups with ~1-5% Neo4j fallback for cache misses
//! - **Expected speedup**: 10-100x for ingestion pipeline
//!
//! # Cache Strategy
//!
//! - LRU (Least Recently Used) eviction policy
//! - Configurable capacity (default: 100,000 entries ≈ 15MB)
//! - Neo4j fallback for cache misses (dormant UTXOs)
//! - Exploit temporal locality: most inputs spend recent outputs

mod cache;

pub use cache::{UtxoCache, CachedOutput, UtxoCacheStats};
