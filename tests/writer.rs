//! Writer module tests
//!
//! Tests for src/writer/* modules

#[path = "writer/mock.rs"]
mod mock;

#[path = "writer/neo4j"]
mod neo4j {
    #[path = "mod.rs"]
    mod tests;
}
