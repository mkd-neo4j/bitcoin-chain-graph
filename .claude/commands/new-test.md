---
name: new-test
description: Scaffold an integration test following project conventions
allowed-tools: Read, Write, Edit, Bash, Glob, Grep
---

# /new-test — Scaffold an Integration Test

Scaffolds a new test file following this project's testing conventions.

## Arguments

The user provides: `<category> <test_name>`

Where `<category>` is one of: `domain`, `parser`, `writer`, `integration`

Example: `/new-test integration reorg_handling`

## Steps

### 1. Create test file

Create `tests/<category>/<test_name>.rs`.

**For MockWriter tests (domain, integration):**

```rust
//! Tests for <test_name>
//!
//! <Description of what is being tested>

use bitcoin::Network;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::writer::{GraphWriter, MockWriter};

#[tokio::test]
async fn test_<test_name>_basic() {
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(
        writer.clone(),
        Network::Bitcoin,
        100_000,
    );
    orchestrator.init_schema().await.unwrap();

    // Test implementation here

    // Verify with MockWriter accessors
    let blocks = writer.get_blocks().await;
    assert_eq!(blocks.len(), 0, "Should start with no blocks");
}
```

**For parser tests:**

```rust
//! Tests for <test_name>
//!
//! <Description of what is being tested>

use bitcoin::Network;
use bitcoin_chain_graph::parser::BlockFileReader;

#[test]
fn test_<test_name>_basic() {
    let mut reader = BlockFileReader::new(
        "test_data/blk00000.dat",
        Network::Bitcoin,
    ).unwrap();
    let block = reader.next_block().unwrap().expect("Block should exist");
    // ... assertions
}
```

**For Neo4j tests (requires running database):**

```rust
//! Tests for <test_name>
//!
//! Requires running Neo4j instance. Run with: cargo test -- --ignored

use bitcoin_chain_graph::config::ConfigLoader;
use bitcoin_chain_graph::writer::Neo4jWriter;

#[tokio::test]
#[ignore]
async fn test_<test_name>_neo4j() {
    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");
    let writer = Neo4jWriter::new(config.neo4j.clone()).await
        .expect("Failed to connect to Neo4j");
    // ... test with real database
}
```

### 2. Register in parent module

Add to `tests/<category>.rs`:

```rust
#[path = "<category>/<test_name>.rs"]
mod <test_name>;
```

### 3. Run the test

```bash
cargo test <test_name>
```
