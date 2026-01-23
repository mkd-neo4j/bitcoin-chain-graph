//! Domain module tests
//!
//! Tests for src/domain/* modules

#[path = "domain/models.rs"]
mod models;

#[path = "domain/ingestion.rs"]
mod ingestion;

#[path = "domain/utxo"]
mod utxo {
    mod cache;
}
