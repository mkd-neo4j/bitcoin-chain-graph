# Testing Strategy

Comprehensive testing approach for Bitcoin blockchain ingestion: unit tests, integration tests, property-based tests, and performance benchmarks.

---

## Overview

Testing pyramid for blockchain ingestion:
1. **Unit Tests**: Parse functions, address derivation, UTXO cache (no database)
2. **Integration Tests**: Domain logic with MockWriter (no database required)
3. **End-to-End Tests**: Full ingestion with real Neo4j (rare, slow)
4. **Property Tests**: Invariants (balance equations, UTXO consistency)
5. **Benchmarks**: Performance regression testing

---

## Testing Without Neo4j (Fast Tests)

### Why Avoid Database in Tests?

**Problems with testing against real Neo4j:**
- ❌ Slow (setup/teardown, network latency)
- ❌ Flaky (connection issues, race conditions)
- ❌ Complex (requires running Neo4j, managing state)
- ❌ Expensive (CI runners, developer machines)

**Solution**: Use MockWriter for domain logic tests
- ✅ Fast (in-memory, no network)
- ✅ Deterministic (no external dependencies)
- ✅ Simple (no setup/teardown)
- ✅ Parallelizable (no shared state)

### MockWriter Implementation

**Location**: `src/writer/mock.rs`

```rust
use crate::writer::GraphWriter;
use async_trait::async_trait;
use std::sync::Mutex;
use anyhow::Result;

/// Mock writer for testing - stores data in memory
pub struct MockWriter {
    blocks: Mutex<Vec<BlockData>>,
    transactions: Mutex<Vec<TransactionData>>,
    outputs: Mutex<Vec<OutputData>>,
    inputs: Mutex<Vec<InputData>>,
}

impl MockWriter {
    pub fn new() -> Self {
        Self {
            blocks: Mutex::new(Vec::new()),
            transactions: Mutex::new(Vec::new()),
            outputs: Mutex::new(Vec::new()),
            inputs: Mutex::new(Vec::new()),
        }
    }

    // Test helper methods (not part of trait)
    pub fn get_blocks(&self) -> Vec<BlockData> {
        self.blocks.lock().unwrap().clone()
    }

    pub fn get_outputs(&self) -> Vec<OutputData> {
        self.outputs.lock().unwrap().clone()
    }

    pub fn find_output(&self, output_id: &str) -> Option<OutputData> {
        self.outputs.lock().unwrap()
            .iter()
            .find(|o| o.output_id == output_id)
            .cloned()
    }
}

#[async_trait]
impl GraphWriter for MockWriter {
    async fn init_schema(&self) -> Result<()> {
        // No-op for mock
        Ok(())
    }

    async fn write_blocks(&self, blocks: &[BlockData]) -> Result<()> {
        self.blocks.lock().unwrap().extend_from_slice(blocks);
        Ok(())
    }

    async fn write_transactions(&self, txs: &[TransactionData]) -> Result<()> {
        self.transactions.lock().unwrap().extend_from_slice(txs);
        Ok(())
    }

    async fn write_outputs(&self, outputs: &[OutputData]) -> Result<()> {
        self.outputs.lock().unwrap().extend_from_slice(outputs);
        Ok(())
    }

    async fn write_inputs(&self, inputs: &[InputData]) -> Result<()> {
        self.inputs.lock().unwrap().extend_from_slice(inputs);
        Ok(())
    }

    async fn lookup_output(&self, output_id: &str) -> Result<OutputData> {
        self.outputs.lock().unwrap()
            .iter()
            .find(|o| o.output_id == output_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Output not found: {}", output_id))
    }

    async fn mark_output_spent(
        &self,
        output_id: &str,
        spent_in_txid: &str,
        spent_at_height: u32
    ) -> Result<()> {
        let mut outputs = self.outputs.lock().unwrap();
        if let Some(output) = outputs.iter_mut().find(|o| o.output_id == output_id) {
            output.is_spent = true;
            output.spent_in_txid = Some(spent_in_txid.to_string());
            output.spent_at_height = Some(spent_at_height);
        }
        Ok(())
    }

    // ... implement other trait methods
}
```

### Integration Tests Without Database

**Location**: `tests/integration_tests.rs`

```rust
use bitcoin_chain_graph::{
    domain::IngestionOrchestrator,
    writer::mock::MockWriter,
    parser::BlockFileReader,
};
use std::sync::Arc;

#[tokio::test]
async fn test_ingest_genesis_block() {
    // Use mock writer - NO Neo4j required!
    let writer = Arc::new(MockWriter::new());
    let mut orchestrator = IngestionOrchestrator::new(writer.clone());

    // Parse genesis block
    let genesis = create_genesis_block();

    // Run ingestion
    orchestrator.ingest_block(genesis).await.unwrap();

    // Assert using mock helper methods
    let blocks = writer.get_blocks();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].height, 0);
    assert_eq!(blocks[0].hash, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f");

    let outputs = writer.get_outputs();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].amount, 50.0);
    assert_eq!(outputs[0].is_spent, false);
}

#[tokio::test]
async fn test_ingest_block_with_spend() {
    let writer = Arc::new(MockWriter::new());
    let mut orchestrator = IngestionOrchestrator::new(writer.clone());

    // Create two blocks: block 0 creates output, block 1 spends it
    let block0 = create_test_block(0, vec![create_test_tx_with_output()]);
    let block1 = create_test_block(1, vec![create_test_tx_with_input("tx0:0")]);

    // Ingest both
    orchestrator.ingest_block(block0).await.unwrap();
    orchestrator.ingest_block(block1).await.unwrap();

    // Assert output was marked as spent
    let output = writer.find_output("tx0:0").unwrap();
    assert_eq!(output.is_spent, true);
    assert_eq!(output.spent_in_txid, Some("tx1".to_string()));
    assert_eq!(output.spent_at_height, Some(1));
}

#[tokio::test]
async fn test_utxo_cache_hit_rate() {
    let writer = Arc::new(MockWriter::new());
    let mut orchestrator = IngestionOrchestrator::new(writer.clone());

    // Ingest blocks 0-100
    for height in 0..100 {
        let block = create_test_block(height, vec![create_test_tx()]);
        orchestrator.ingest_block(block).await.unwrap();
    }

    // Check UTXO cache statistics
    let stats = orchestrator.get_utxo_cache_stats();
    assert!(stats.hit_rate > 0.95, "Cache hit rate should be >95%");
}
```

**Benefits**:
- ✅ Tests run in milliseconds (not seconds)
- ✅ No Neo4j setup/teardown
- ✅ Can run in parallel (no shared database state)
- ✅ Easy to debug (no network, no external process)
- ✅ CI-friendly (no Docker, no services)

### Unit Tests for Domain Logic

```rust
#[cfg(test)]
mod domain_tests {
    use super::*;

    #[tokio::test]
    async fn test_phase1_creates_blocks() {
        let writer = Arc::new(MockWriter::new());
        let orchestrator = IngestionOrchestrator::new(writer.clone());

        let blocks = vec![create_test_block(100, vec![])];
        orchestrator.ingest_blocks(blocks).await.unwrap();

        let written_blocks = writer.get_blocks();
        assert_eq!(written_blocks.len(), 1);
        assert_eq!(written_blocks[0].height, 100);
    }

    #[tokio::test]
    async fn test_phase3_creates_outputs_and_addresses() {
        let writer = Arc::new(MockWriter::new());
        let mut orchestrator = IngestionOrchestrator::new(writer.clone());

        let txs = vec![create_tx_with_p2pkh_output()];
        orchestrator.ingest_outputs(&txs).await.unwrap();

        let outputs = writer.get_outputs();
        assert_eq!(outputs.len(), 1);
        assert!(outputs[0].address.is_some());
        assert_eq!(outputs[0].script_type, "P2PKH");
    }

    #[tokio::test]
    async fn test_phase4_creates_spends() {
        let writer = Arc::new(MockWriter::new());
        let mut orchestrator = IngestionOrchestrator::new(writer.clone());

        // First create output
        let tx0 = create_tx_with_output("tx0");
        orchestrator.ingest_outputs(&vec![tx0]).await.unwrap();

        // Then spend it
        let tx1 = create_tx_with_input("tx0:0");
        orchestrator.ingest_inputs(&vec![tx1], 1).await.unwrap();

        // Assert output marked as spent
        let output = writer.find_output("tx0:0").unwrap();
        assert!(output.is_spent);
    }
}
```

---

## Test Block Selection

### Genesis Block (Block 0)
```rust
// Use for: P2PK parsing, coinbase handling, special cases
const GENESIS_BLOCK_HASH: &str = "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f";
```

### Early Blocks (1-1000)
- Simple transactions (1-2 inputs/outputs)
- P2PK and early P2PKH
- Small block sizes

### SegWit Activation (Block 481,824)
- First SegWit transactions
- Witness data parsing

### Taproot Activation (Block 709,632)
- First P2TR outputs
- Bech32m encoding

---

## Unit Tests

### Test Block File Parser

```rust
#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn test_parse_genesis_block() {
        // Genesis block data (hardcoded for deterministic testing)
        let genesis_data = include_bytes!("../../test_data/genesis.dat");

        let block: Block = bitcoin::consensus::deserialize(genesis_data).unwrap();

        assert_eq!(
            block.block_hash().to_string(),
            "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
        );
        assert_eq!(block.txdata.len(), 1);
        assert!(block.txdata[0].is_coin_base());

        // Check coinbase output
        let coinbase_output = &block.txdata[0].output[0];
        assert_eq!(coinbase_output.value, 50_0000_0000); // 50 BTC in satoshis
    }

    #[test]
    fn test_parse_block_file_stream() {
        let mut reader = BlockFileReader::new(
            "test_data/blk00000.dat",
            bitcoin::Network::Bitcoin
        ).unwrap();

        let mut block_count = 0;
        while let Some(block) = reader.next_block().unwrap() {
            assert!(block.txdata.len() > 0);
            block_count += 1;

            if block_count >= 10 {
                break;
            }
        }

        assert_eq!(block_count, 10);
    }

    #[test]
    fn test_invalid_magic_bytes() {
        // Create file with invalid magic bytes
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&[0x00, 0x00, 0x00, 0x00]).unwrap();  // Wrong magic

        let result = BlockFileReader::new(
            file.path().to_str().unwrap(),
            bitcoin::Network::Bitcoin
        );

        // Should succeed (reader doesn't validate until reading)
        let mut reader = result.unwrap();

        // Should fail when trying to read
        let result = reader.next_block();
        assert!(matches!(result, Err(ParseError::InvalidMagic { .. })));
    }
}
```

### Test Address Derivation

```rust
#[cfg(test)]
mod address_tests {
    use super::*;
    use bitcoin::{Address, Network, Script};

    #[test]
    fn test_derive_p2pkh_address() {
        // Known P2PKH scriptPubKey
        let script_hex = "76a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac";
        let script_bytes = hex::decode(script_hex).unwrap();
        let script = Script::from(script_bytes);

        let address = derive_address(&script, Network::Bitcoin).unwrap();
        assert_eq!(address.address_type().unwrap(), bitcoin::AddressType::P2pkh);
    }

    #[test]
    fn test_derive_p2wpkh_address() {
        // Known P2WPKH scriptPubKey
        let script_hex = "0014751e76e8199196d454941c45d1b3a323f1433bd6";
        let script_bytes = hex::decode(script_hex).unwrap();
        let script = Script::from(script_bytes);

        let address = derive_address(&script, Network::Bitcoin).unwrap();
        assert!(address.to_string().starts_with("bc1q"));
        assert_eq!(address.address_type().unwrap(), bitcoin::AddressType::P2wpkh);
    }

    #[test]
    fn test_derive_p2tr_address() {
        // Known P2TR scriptPubKey (Taproot)
        let script_hex = "51205c0e6a94e20dc88f7f9b8fc3e213f30ca0dcdeb7d09b8c89c19b7cc5cf3b2b73";
        let script_bytes = hex::decode(script_hex).unwrap();
        let script = Script::from(script_bytes);

        let address = derive_address(&script, Network::Bitcoin).unwrap();
        assert!(address.to_string().starts_with("bc1p"));
        assert_eq!(address.address_type().unwrap(), bitcoin::AddressType::P2tr);
    }

    #[test]
    fn test_op_return_no_address() {
        // OP_RETURN script
        let script_hex = "6a13636861726c6579206c6f766573206865696469";
        let script_bytes = hex::decode(script_hex).unwrap();
        let script = Script::from(script_bytes);

        let address = derive_address(&script, Network::Bitcoin);
        assert!(address.is_none(), "OP_RETURN should not have address");
    }
}
```

### Test UTXO Cache

```rust
#[cfg(test)]
mod utxo_cache_tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_hit() {
        let neo4j_client = create_mock_neo4j_client();
        let mut cache = UtxoCache::new(100, neo4j_client);

        // Insert output
        let output = CachedOutput {
            output_id: "txid:0".to_string(),
            amount: 50.0,
            script_pubkey: vec![],
            address: Some("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa".to_string()),
        };
        cache.insert(output.output_id.clone(), output.clone());

        // Retrieve (should be cache hit)
        let retrieved = cache.get("txid:0").await.unwrap();
        assert_eq!(retrieved.amount, 50.0);

        // Check stats
        let (hits, misses, _) = cache.stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 0);
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let neo4j_client = create_mock_neo4j_client();
        let mut cache = UtxoCache::new(2, neo4j_client); // Small cache

        // Insert 3 outputs (cache size = 2)
        cache.insert("txid:0".to_string(), create_test_output(0));
        cache.insert("txid:1".to_string(), create_test_output(1));
        cache.insert("txid:2".to_string(), create_test_output(2)); // Evicts txid:0

        // txid:0 should be evicted (causes Neo4j query)
        let result = cache.get("txid:0").await;
        // Would query Neo4j, which is mocked to return error for this test
        assert!(result.is_err());
    }
}
```

---

## Integration Tests

### Test Neo4j Ingestion

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use testcontainers::*;

    #[tokio::test]
    async fn test_ingest_genesis_block() {
        // Start Neo4j testcontainer
        let docker = clients::Cli::default();
        let neo4j_container = docker.run(images::neo4j::Neo4j::default());
        let port = neo4j_container.get_host_port_ipv4(7687);

        // Connect to Neo4j
        let uri = format!("bolt://localhost:{}", port);
        let client = Neo4jClient::new(&uri, "neo4j", "test").await.unwrap();

        // Initialize schema
        client.init_schema().await.unwrap();

        // Load genesis block
        let genesis_data = include_bytes!("../../test_data/genesis.dat");
        let block: Block = bitcoin::consensus::deserialize(genesis_data).unwrap();

        // Ingest block
        client.ingest_block(&block, 0).await.unwrap();

        // Verify block was created
        let result = client.graph()
            .execute(query("MATCH (b:Block {height: 0}) RETURN b.hash as hash"))
            .await
            .unwrap();

        let row = result.next().await.unwrap().unwrap();
        let hash: String = row.get("hash").unwrap();
        assert_eq!(hash, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f");

        // Verify transaction was created
        let result = client.graph()
            .execute(query("MATCH (t:Transaction) RETURN count(t) as count"))
            .await
            .unwrap();

        let row = result.next().await.unwrap().unwrap();
        let count: i64 = row.get("count").unwrap();
        assert_eq!(count, 1);

        // Verify output was created
        let result = client.graph()
            .execute(query("MATCH (o:Output) RETURN o.amount as amount"))
            .await
            .unwrap();

        let row = result.next().await.unwrap().unwrap();
        let amount: f64 = row.get("amount").unwrap();
        assert_eq!(amount, 50.0);
    }

    #[tokio::test]
    async fn test_ingest_multiple_blocks() {
        let docker = clients::Cli::default();
        let neo4j_container = docker.run(images::neo4j::Neo4j::default());
        let port = neo4j_container.get_host_port_ipv4(7687);

        let uri = format!("bolt://localhost:{}", port);
        let client = Neo4jClient::new(&uri, "neo4j", "test").await.unwrap();
        client.init_schema().await.unwrap();

        // Ingest first 10 blocks
        let mut reader = BlockFileReader::new(
            "test_data/blk00000.dat",
            bitcoin::Network::Bitcoin
        ).unwrap();

        for height in 0..10 {
            let block = reader.next_block().unwrap().unwrap();
            client.ingest_block(&block, height).await.unwrap();
        }

        // Verify block count
        let result = client.graph()
            .execute(query("MATCH (b:Block) RETURN count(b) as count"))
            .await
            .unwrap();

        let row = result.next().await.unwrap().unwrap();
        let count: i64 = row.get("count").unwrap();
        assert_eq!(count, 10);

        // Verify NEXT_BLOCK relationships
        let result = client.graph()
            .execute(query("MATCH ()-[r:NEXT_BLOCK]->() RETURN count(r) as count"))
            .await
            .unwrap();

        let row = result.next().await.unwrap().unwrap();
        let count: i64 = row.get("count").unwrap();
        assert_eq!(count, 9); // 10 blocks = 9 relationships
    }
}
```

---

## Property-Based Tests

### Test Balance Invariants

```rust
#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_transaction_balance_invariant(
            input_amounts in prop::collection::vec(0.0..100.0f64, 1..10),
            output_amounts in prop::collection::vec(0.0..100.0f64, 1..10)
        ) {
            let total_input: f64 = input_amounts.iter().sum();
            let total_output: f64 = output_amounts.iter().sum();
            let fee = (total_input - total_output).max(0.0);

            // Invariant: totalInput = totalOutput + fee
            prop_assert!((total_input - (total_output + fee)).abs() < 0.0001);
        }

        #[test]
        fn test_utxo_spent_once(
            output_ids in prop::collection::vec("[a-z0-9]{64}:[0-9]", 10..100)
        ) {
            // Invariant: Each output can only be spent once
            let mut spent_set = std::collections::HashSet::new();

            for output_id in output_ids {
                // First spend should succeed
                if !spent_set.contains(&output_id) {
                    spent_set.insert(output_id.clone());
                }
            }

            // No duplicates in spent set
            prop_assert_eq!(spent_set.len(), output_ids.len());
        }
    }
}
```

---

## Validation Tests

### Test Data Integrity

```rust
#[cfg(test)]
mod validation_tests {
    use super::*;

    #[tokio::test]
    async fn test_transaction_balance_validation() {
        let client = setup_test_neo4j().await;

        // Ingest test blocks
        ingest_test_blocks(&client).await.unwrap();

        // Run validation query
        let cypher = "
            MATCH (t:Transaction {isCoinbase: false})
            WHERE t.totalInput <> (t.totalOutput + t.fee)
            RETURN count(t) as violations
        ";

        let mut result = client.graph().execute(query(cypher)).await.unwrap();
        let row = result.next().await.unwrap().unwrap();
        let violations: i64 = row.get("violations").unwrap();

        assert_eq!(violations, 0, "Transaction balance violations found");
    }

    #[tokio::test]
    async fn test_utxo_double_spend_validation() {
        let client = setup_test_neo4j().await;
        ingest_test_blocks(&client).await.unwrap();

        // Check for outputs spent multiple times
        let cypher = "
            MATCH (o:Output)<-[:SPENDS]-(i:Input)
            WITH o, count(i) AS spendCount
            WHERE spendCount > 1
            RETURN count(o) as doubleSpends
        ";

        let mut result = client.graph().execute(query(cypher)).await.unwrap();
        let row = result.next().await.unwrap().unwrap();
        let double_spends: i64 = row.get("doubleSpends").unwrap();

        assert_eq!(double_spends, 0, "Double-spend detected");
    }
}
```

---

## Performance Benchmarks

### Benchmark Block Parsing

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};

fn benchmark_block_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_parsing");

    // Benchmark different block sizes
    for (name, data) in [
        ("genesis", include_bytes!("../../test_data/genesis.dat")),
        ("block_100", include_bytes!("../../test_data/block_100.dat")),
        ("block_500k", include_bytes!("../../test_data/block_500k.dat")),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), data, |b, data| {
            b.iter(|| {
                let block: Block = bitcoin::consensus::deserialize(black_box(data)).unwrap();
                block
            });
        });
    }

    group.finish();
}

fn benchmark_address_derivation(c: &mut Criterion) {
    let mut group = c.benchmark_group("address_derivation");

    let scripts = [
        ("P2PKH", hex::decode("76a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac").unwrap()),
        ("P2WPKH", hex::decode("0014751e76e8199196d454941c45d1b3a323f1433bd6").unwrap()),
        ("P2TR", hex::decode("51205c0e6a94e20dc88f7f9b8fc3e213f30ca0dcdeb7d09b8c89c19b7cc5cf3b2b73").unwrap()),
    ];

    for (name, script_bytes) in scripts {
        let script = Script::from(script_bytes);
        group.bench_with_input(BenchmarkId::from_parameter(name), &script, |b, script| {
            b.iter(|| {
                derive_address(black_box(script), bitcoin::Network::Bitcoin)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, benchmark_block_parsing, benchmark_address_derivation);
criterion_main!(benches);
```

**Run benchmarks**:
```bash
cargo bench
```

---

## Test Data Preparation

### Extract Test Blocks

```bash
# Extract genesis block
bitcoin-cli getblock 000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f 0 > test_data/genesis.dat

# Extract block 100
bitcoin-cli getblockhash 100 | xargs bitcoin-cli getblock 0 > test_data/block_100.dat

# Extract SegWit block (481824)
bitcoin-cli getblockhash 481824 | xargs bitcoin-cli getblock 0 > test_data/block_segwit.dat
```

### Mock Neo4j Client for Unit Tests

```rust
pub fn create_mock_neo4j_client() -> MockNeo4jClient {
    MockNeo4jClient {
        responses: Arc::new(Mutex::new(HashMap::new())),
    }
}

impl MockNeo4jClient {
    pub async fn execute(&self, query: &str) -> Result<Vec<BoltMap>> {
        // Return mocked responses based on query
        let responses = self.responses.lock().await;
        responses.get(query).cloned().ok_or(Error::NotFound)
    }

    pub fn add_mock_response(&mut self, query: &str, response: Vec<BoltMap>) {
        let mut responses = self.responses.lock().await;
        responses.insert(query.to_string(), response);
    }
}
```

---

## CI/CD Testing

### GitHub Actions Workflow

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest

    services:
      neo4j:
        image: neo4j:5
        env:
          NEO4J_AUTH: neo4j/test
        ports:
          - 7687:7687
        options: >-
          --health-cmd "cypher-shell -u neo4j -p test 'RETURN 1'"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5

    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run unit tests
        run: cargo test --lib
      - name: Run integration tests
        run: cargo test --test '*'
        env:
          NEO4J_URI: bolt://localhost:7687
          NEO4J_PASSWORD: test
      - name: Run benchmarks
        run: cargo bench --no-run
```

---

## Test Coverage

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html --output-dir coverage

# View report
open coverage/index.html
```

**Target**: >80% code coverage

---

## Testing Checklist

- [ ] Unit tests for block file parser
- [ ] Unit tests for address derivation (all types)
- [ ] Unit tests for UTXO cache
- [ ] Integration tests with Neo4j (genesis block)
- [ ] Integration tests with Neo4j (multiple blocks)
- [ ] Property tests for balance invariants
- [ ] Property tests for UTXO uniqueness
- [ ] Validation tests (run queries from ../VALIDATION.md)
- [ ] Performance benchmarks
- [ ] CI/CD pipeline configured
- [ ] Test coverage >80%

---

## References

- [Rust Testing Documentation](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Criterion Benchmarking](https://bheisler.github.io/criterion.rs/book/)
- [Proptest Property Testing](https://proptest-rs.github.io/proptest/)
- [Testcontainers](https://docs.rs/testcontainers/latest/testcontainers/)
- [../VALIDATION.md](../VALIDATION.md) - Data integrity validation queries

---

## Next Steps

1. Implement test suite following this guide
2. Set up CI/CD with GitHub Actions
3. Add test data directory with sample blocks
4. Run validation tests after ingestion
