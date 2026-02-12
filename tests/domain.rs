//! Domain module tests
//!
//! Tests for src/domain/* modules

#[path = "domain/models.rs"]
mod models;

#[path = "domain/ingestion.rs"]
mod ingestion;

#[path = "domain/bip30.rs"]
mod bip30;

#[path = "domain/snapshot_resilience.rs"]
mod snapshot_resilience;

#[path = "domain/adaptive_transaction_memory.rs"]
mod adaptive_transaction_memory;

#[path = "domain/shutdown_height.rs"]
mod shutdown_height;

#[path = "domain/utxo"]
mod utxo {
    mod cache;
    mod no_neo4j_fallback;
    mod persistence;
}
