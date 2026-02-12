//! Domain module tests
//!
//! Tests for src/domain/* modules

#[path = "domain/models.rs"]
mod models;

#[path = "domain/ingestion.rs"]
mod ingestion;

#[path = "domain/bip30.rs"]
mod bip30;

#[path = "domain/utxo"]
mod utxo {
    mod cache;
    mod no_neo4j_fallback;
    mod persistence;
}
