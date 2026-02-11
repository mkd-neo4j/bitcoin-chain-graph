# Test Conventions

## Framework

- `#[test]` for synchronous unit tests
- `#[tokio::test]` for async tests (anything calling GraphWriter methods)
- `MockWriter` for all tests that don't specifically need Neo4j
- `#[ignore]` on tests that require a running Neo4j instance

## Directory Structure

Tests mirror the source tree:

```
tests/
├── domain.rs              # Declares: domain/models, domain/ingestion, domain/utxo
├── domain/
│   ├── models.rs          # Domain model unit tests
│   ├── ingestion.rs       # IngestionOrchestrator tests with MockWriter
│   └── utxo/cache.rs      # UTXO cache tests
├── parser.rs              # Declares: parser/address, parser/block_file
├── parser/
│   ├── address.rs         # Address extraction tests (all 7 script types)
│   └── block_file.rs      # Block file parsing tests
├── writer.rs              # Declares: writer/mock, writer/neo4j
├── writer/
│   ├── mock.rs            # MockWriter integration tests
│   └── neo4j/mod.rs       # Neo4j-specific tests (#[ignore])
├── integration.rs         # Declares: integration/full_pipeline, batch_ingestion, checkpoint_resume
├── integration/
│   ├── full_pipeline.rs   # End-to-end with Neo4j (#[ignore])
│   ├── batch_ingestion.rs # Batch processing tests
│   └── checkpoint_resume.rs # Resume and crash recovery
├── utils.rs               # Declares: utils/clear_neo4j, drop_database, verify_status
└── utils/                 # Neo4j utility tests (#[ignore])
```

## Module Declaration Pattern

Each top-level test file declares its submodules:

```rust
// tests/domain.rs
#[path = "domain/models.rs"]
mod models;

#[path = "domain/ingestion.rs"]
mod ingestion;
```

For nested modules:

```rust
// tests/domain.rs
#[path = "domain/utxo"]
mod utxo {
    mod cache;
}
```

## MockWriter Test Template

```rust
use bitcoin::Network;
use bitcoin_chain_graph::domain::IngestionOrchestrator;
use bitcoin_chain_graph::writer::{GraphWriter, MockWriter};

#[tokio::test]
async fn test_feature_name() {
    // Setup
    let writer = MockWriter::new();
    let orchestrator = IngestionOrchestrator::new(writer.clone(), Network::Bitcoin, 100_000);
    orchestrator.init_schema().await.unwrap();

    // Action
    // ... perform the operation being tested

    // Verify using MockWriter accessors
    let blocks = writer.get_blocks().await;
    assert_eq!(blocks.len(), 1, "Should have exactly 1 block");
}
```

## Block File Test Template

```rust
use bitcoin::Network;
use bitcoin_chain_graph::parser::BlockFileReader;

#[test]
fn test_parse_feature() {
    let mut reader = BlockFileReader::new("test_data/blk00000.dat", Network::Bitcoin).unwrap();
    let block = reader.next_block().unwrap().expect("Block should exist");
    // ... assertions
}
```

## Test Naming

- `test_<feature>_<scenario>` — e.g., `test_ingest_genesis_block_all_phases`
- `test_<function>_<edge_case>` — e.g., `test_checkpoint_lifecycle`

## Test Data

- `test_data/blk00000.dat` — First Bitcoin block file (genesis block + early blocks)
- Genesis block hash: `000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f`
- Genesis coinbase address: `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa`
- Genesis block: 1 transaction, 1 output, 50 BTC (5,000,000,000 satoshis)

## Assertion Patterns

- Descriptive messages: `assert_eq!(blocks.len(), 1, "Should have 1 block after ingestion")`
- Error variants: `assert!(matches!(result.unwrap_err(), WriterError::OutputNotFound(_)))`
- Approximate floats: `assert!((value - expected).abs() < 0.01, "Should be approximately equal")`
- Option checks: `assert!(result.is_some(), "Should return a value")`
