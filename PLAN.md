# Development Plan: Bitcoin Chain Graph Ingestion System

## Overview

This is an incremental, test-driven development plan that builds the Bitcoin blockchain ingestion system layer-by-layer. Each milestone delivers a **working, testable component** before moving to the next.

## Development Philosophy

✅ **Build → Test → Verify → Next**
- Each milestone produces working, testable code
- Tests written before or alongside implementation
- No "big bang" integration - continuous validation
- Fast feedback loops (compile + test in <10 seconds)

✅ **Bottom-up construction** (Parser → Domain → Writer)
- Start with zero dependencies (Parser layer)
- Add complexity only when needed
- Mock external dependencies (Neo4j) until integration phase

---

## Progress Tracking

This document will be updated as milestones are completed:

- [x] M1: Project Setup & Parser Foundation ✅ **Completed 2025-01-23**
- [x] M2: Address Derivation ✅ **Completed 2025-01-23**
- [x] M3: Domain Models ✅ **Completed 2025-01-23**
- [x] M4: GraphWriter Trait ✅ **Completed 2025-01-23**
- [ ] M5: Ingestion Orchestrator
- [ ] M6: Neo4j Writer Implementation
- [ ] M7: UTXO Cache
- [ ] M8: Checkpoint Management
- [ ] M9: CLI Application
- [ ] M10: Special Cases Handling
- [ ] M11: Validation & Error Handling
- [ ] M12: Performance Optimization
- [ ] M13: Full Integration Test
- [ ] M14: Documentation & Polish

---

## Milestone 1: Project Setup & Parser Foundation
**Goal**: Parse Genesis block from test data
**Duration**: 1-2 hours
**Deliverable**: Read and deserialize Genesis block

### Tasks
1. **Initialize Rust project**
   - `cargo init --name bitcoin-chain-graph`
   - Add dependencies: `bitcoin = "0.31"`, `tokio`, `anyhow`
   - Create 3-layer directory structure (parser/, domain/, writer/)

2. **Implement `BlockFileReader` (Parser Layer 1)**
   - Location: `src/parser/block_file.rs`
   - Read magic bytes + block size
   - Stream blocks from `.blk` file
   - Use `bitcoin` crate for deserialization
   - **Zero Neo4j dependencies**

3. **Write unit tests**
   - Test: Parse Genesis block from `test_data/blk00000.dat`
   - Test: Validate Genesis block hash
   - Test: Parse first 10 blocks
   - Test: Handle EOF gracefully

### Acceptance Criteria
- ✅ `cargo test` passes for Genesis block parsing
- ✅ Can stream 100 blocks from blk00000.dat
- ✅ Execution time: <100ms for first 100 blocks

### References
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) - Layer 1: Parser
- [rust/BINARY_PARSING.md](docs/rust/BINARY_PARSING.md) - Block file format
- [test_data/README.md](test_data/README.md) - Test data details

---

## Milestone 2: Address Derivation (Parser)
**Goal**: Extract addresses from scriptPubKey
**Duration**: 2-3 hours
**Deliverable**: Parse all standard address types

### Tasks
1. **Implement address derivation (pure functions)**
   - Location: `src/parser/address.rs`
   - Detect script types (P2PKH, P2SH, P2WPKH, P2WSH, P2TR)
   - Extract addresses using `bitcoin::Address`
   - Handle NULL_DATA and UNKNOWN types

2. **Write unit tests**
   - Test: P2PKH address from Genesis coinbase
   - Test: Each script type with known addresses
   - Test: NULL_DATA returns None
   - Test: Invalid scripts return None

### Acceptance Criteria
- ✅ `cargo test` passes for all address types
- ✅ Genesis coinbase output produces correct P2PKH address
- ✅ NULL_DATA outputs handled gracefully

### References
- [ADDRESS_DERIVATION.md](docs/ADDRESS_DERIVATION.md) - Complete address extraction guide
- [SPECIAL_CASES.md](docs/SPECIAL_CASES.md) - Edge cases

---

## Milestone 3: Domain Models (Domain Layer)
**Goal**: Define stable domain types
**Duration**: 1-2 hours
**Deliverable**: Type-safe domain models

### Tasks
1. **Create domain models**
   - Location: `src/domain/models.rs`
   - `BlockData`, `TransactionData`, `OutputData`, `InputData`
   - `CheckpointData` (for resume functionality)
   - Conversion from `bitcoin` crate types to domain types

2. **Write conversion tests**
   - Test: Convert `bitcoin::Block` → `BlockData`
   - Test: Convert `bitcoin::Transaction` → `TransactionData`
   - Test: Preserve all required fields

### Acceptance Criteria
- ✅ All models implement `Clone`, `Debug`
- ✅ Conversions preserve data integrity
- ✅ No `neo4rs` imports in domain layer

### References
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) - Domain models
- [DATA_MODEL.md](docs/DATA_MODEL.md) - Node properties

---

## Milestone 4: GraphWriter Trait (Writer Layer Interface)
**Goal**: Define database abstraction
**Duration**: 1 hour
**Deliverable**: Trait definition + Mock implementation

### Tasks
1. **Define `GraphWriter` trait**
   - Location: `src/writer/traits.rs`
   - Methods for 6-phase ingestion
   - Checkpoint management methods
   - UTXO lookup method

2. **Implement `MockWriter`**
   - Location: `src/writer/mock.rs`
   - Store data in `Vec<T>` in-memory
   - Provide test accessors (`get_blocks()`, `get_transactions()`)
   - Implement all trait methods as no-ops or memory storage

### Acceptance Criteria
- ✅ Trait compiles with `async_trait`
- ✅ MockWriter can store and retrieve blocks
- ✅ No Neo4j dependencies yet

### References
- [ARCHITECTURE.md](docs/ARCHITECTURE.md:208-256) - GraphWriter trait definition

---

## Milestone 5: Ingestion Orchestrator (Domain Layer)
**Goal**: Orchestrate 6-phase ingestion
**Duration**: 3-4 hours
**Deliverable**: End-to-end ingestion with MockWriter

### Tasks
1. **Implement `IngestionOrchestrator`**
   - Location: `src/domain/ingestion.rs`
   - Generic over `W: GraphWriter`
   - Implement 6 phases:
     - Phase 1: Block nodes
     - Phase 2: Transaction nodes
     - Phase 3: Output nodes + addresses
     - Phase 4: Input nodes + SPENDS (stub UTXO lookup)
     - Phase 5: Calculate amounts
     - Phase 6: Simplified layer (PERFORMS, BENEFITS_TO)

2. **Write integration tests with MockWriter**
   - Test: Ingest Genesis block
   - Test: Ingest first 10 blocks
   - Test: Verify all phases executed
   - Test: Verify data in MockWriter

### Acceptance Criteria
- ✅ Can ingest Genesis block through all 6 phases
- ✅ Tests run in <1 second (no database)
- ✅ All phases call GraphWriter trait methods

### References
- [INGESTION_ARCHITECTURE.md](docs/INGESTION_ARCHITECTURE.md) - 6-phase process
- [ARCHITECTURE.md](docs/ARCHITECTURE.md:130-193) - Orchestrator examples

---

## Milestone 6: Neo4j Writer Implementation (Writer Layer)
**Goal**: Implement real Neo4j writer
**Duration**: 4-5 hours
**Deliverable**: Working Neo4j integration

### Tasks
1. **Add Neo4j dependency**
   - Add `neo4rs = "0.7"` to Cargo.toml

2. **Implement `Neo4jWriter`**
   - Location: `src/writer/neo4j/mod.rs`
   - Connection pool management
   - Implement all `GraphWriter` trait methods

3. **Centralize Cypher queries**
   - Location: `src/writer/neo4j/queries.rs`
   - All queries as `const &str`
   - Parameterized queries (no string interpolation)

4. **Schema initialization**
   - Location: `src/writer/neo4j/schema.rs`
   - Create constraints (unique IDs)
   - Create indexes (performance)

### Acceptance Criteria
- ✅ Connects to Neo4j successfully
- ✅ Creates schema (constraints + indexes)
- ✅ No Cypher queries outside queries.rs
- ✅ All queries parameterized

### References
- [ARCHITECTURE.md](docs/ARCHITECTURE.md:259-573) - Neo4j implementation
- [rust/NEO4J_INTEGRATION.md](docs/rust/NEO4J_INTEGRATION.md) - Connection setup
- [CYPHER_EXAMPLES.md](docs/CYPHER_EXAMPLES.md) - All queries

---

## Milestone 7: UTXO Cache (Domain Layer)
**Goal**: Efficient UTXO lookups
**Duration**: 2-3 hours
**Deliverable**: LRU cache for outputs

### Tasks
1. **Implement `UtxoCache`**
   - Location: `src/domain/utxo/cache.rs`
   - Use `lru = "0.12"` crate
   - Cache-first lookups
   - Fallback to `GraphWriter::lookup_output()`
   - Track hit rate metrics

2. **Integration with orchestrator**
   - Add UTXO cache to `IngestionOrchestrator`
   - Use in Phase 4 (input SPENDS lookups)

3. **Write performance tests**
   - Test: Cache hit for recent outputs
   - Test: Cache miss queries database
   - Benchmark: Ingestion speed with cache

### Acceptance Criteria
- ✅ >99% cache hit rate on early blocks
- ✅ Graceful fallback to database
- ✅ Configurable cache size

### References
- [ARCHITECTURE.md](docs/ARCHITECTURE.md:168-190) - UTXO cache abstraction
- [rust/MEMORY_STRATEGY.md](docs/rust/MEMORY_STRATEGY.md) - Cache sizing

---

## Milestone 8: Checkpoint Management (Domain + Writer)
**Goal**: Resume-on-failure capability
**Duration**: 2-3 hours
**Deliverable**: Resumable ingestion

### Tasks
1. **Implement checkpoint methods in Neo4jWriter**
   - `create_checkpoint()`
   - `update_checkpoint()`
   - `get_checkpoint()`
   - `mark_checkpoint_complete()`

2. **Add checkpoint logic to orchestrator**
   - Create checkpoint on init
   - Update after each block
   - Resume from last checkpoint on startup

3. **Write checkpoint tests**
   - Test: Create and retrieve checkpoint
   - Test: Resume from checkpoint
   - Test: Verify last processed block

### Acceptance Criteria
- ✅ Can resume from any block height
- ✅ At most 1 block reprocessed on failure
- ✅ Checkpoint tracks .blk file name

### References
- [DATA_MODEL.md](docs/DATA_MODEL.md:206-222) - IngestionCheckpoint node
- [INGESTION_ARCHITECTURE.md](docs/INGESTION_ARCHITECTURE.md:226-315) - Checkpoint strategy
- [CYPHER_EXAMPLES.md](docs/CYPHER_EXAMPLES.md:94-227) - Checkpoint queries

---

## Milestone 9: CLI Application (Main)
**Goal**: Runnable CLI tool
**Duration**: 2-3 hours
**Deliverable**: `cargo run` works end-to-end

### Tasks
1. **Implement CLI with clap**
   - Location: `src/main.rs`
   - Commands: `init-schema`, `ingest`, `resume`, `status`
   - Config file support

2. **Wire layers together**
   - Create Neo4jWriter
   - Inject into IngestionOrchestrator
   - Parse .blk files in sequence

3. **Add configuration**
   - Neo4j connection string
   - Batch sizes
   - Cache sizes
   - .blk file directory

### Acceptance Criteria
- ✅ `cargo run -- ingest` processes blocks
- ✅ `cargo run -- resume` continues from checkpoint
- ✅ `cargo run -- status` shows progress

### References
- [rust/SETUP.md](docs/rust/SETUP.md) - CLI commands
- [ARCHITECTURE.md](docs/ARCHITECTURE.md:540-567) - Wiring example

---

## Milestone 10: Special Cases Handling
**Goal**: Handle edge cases
**Duration**: 2-3 hours
**Deliverable**: Robust ingestion

### Tasks
1. **Coinbase transactions**
   - Skip SPENDS relationship creation
   - Handle NULL previous output

2. **OP_RETURN outputs**
   - Skip LOCKED_TO relationship
   - Store scriptPubKey data

3. **Genesis block**
   - Handle unspendable coinbase output
   - No previous block link

4. **Write tests for edge cases**
   - Test: Coinbase handling
   - Test: OP_RETURN handling
   - Test: Genesis block special properties

### Acceptance Criteria
- ✅ All special cases handled gracefully
- ✅ No panics on edge cases
- ✅ Tests cover all documented special cases

### References
- [SPECIAL_CASES.md](docs/SPECIAL_CASES.md) - Complete edge case documentation

---

## Milestone 11: Validation & Error Handling
**Goal**: Data integrity checks
**Duration**: 2-3 hours
**Deliverable**: Validated ingestion

### Tasks
1. **Implement validation queries**
   - After each block: Check transaction amounts balance
   - After ingestion: Verify SPENDS relationships
   - Check for orphaned nodes

2. **Improve error handling**
   - Structured error types
   - Retry logic for transient failures
   - Detailed error messages

3. **Add validation tests**
   - Test: Detect unbalanced transactions
   - Test: Detect missing SPENDS relationships

### Acceptance Criteria
- ✅ Validation runs after each block (optional flag)
- ✅ Clear error messages on validation failure
- ✅ Failed blocks don't corrupt database

### References
- [VALIDATION.md](docs/VALIDATION.md) - Validation queries

---

## Milestone 12: Performance Optimization
**Goal**: Fast ingestion
**Duration**: 2-3 hours
**Deliverable**: >10 blocks/sec on early chain

### Tasks
1. **Implement batch accumulation**
   - Buffer 50-100 blocks before writing
   - Bulk Cypher queries with UNWIND

2. **Optimize UTXO cache**
   - Tune cache size based on profiling
   - Pre-warm cache on resume

3. **Benchmark ingestion**
   - Measure blocks/sec
   - Profile memory usage
   - Identify bottlenecks

### Acceptance Criteria
- ✅ 10-20 blocks/sec on early blocks (blk00000.dat)
- ✅ <2GB memory usage
- ✅ Neo4j query performance acceptable

### References
- [rust/PERFORMANCE.md](docs/rust/PERFORMANCE.md) - Performance targets
- [rust/MEMORY_STRATEGY.md](docs/rust/MEMORY_STRATEGY.md) - Memory optimization

---

## Milestone 13: Full Integration Test
**Goal**: Ingest 100,000+ blocks
**Duration**: 1 day (plus ingestion time)
**Deliverable**: Validated production run

### Tasks
1. **Run full ingestion**
   - Ingest first 100,000 blocks
   - Monitor progress, memory, errors

2. **Validate results**
   - Run validation queries
   - Spot-check known transactions
   - Verify graph structure

3. **Performance analysis**
   - Measure average blocks/sec
   - Analyze slow queries
   - Document bottlenecks

### Acceptance Criteria
- ✅ 100,000 blocks ingested successfully
- ✅ All validation checks pass
- ✅ Resume works correctly

---

## Milestone 14: Documentation & Polish
**Goal**: Production-ready code
**Duration**: 1-2 hours
**Deliverable**: Clean, documented codebase

### Tasks
1. **Code documentation**
   - Public API docs
   - Module-level documentation
   - Inline comments for complex logic

2. **Update README.md**
   - Installation instructions
   - Usage examples
   - Configuration guide

3. **Create examples/**
   - Example: Basic ingestion
   - Example: Resume from checkpoint
   - Example: Query the graph

### Acceptance Criteria
- ✅ `cargo doc --open` produces clear documentation
- ✅ README has complete setup instructions
- ✅ Examples run successfully

---

## Testing Strategy

### Unit Tests (per milestone)
- **Parser tests**: No external dependencies
- **Domain tests**: Use MockWriter
- **Writer tests**: Use Neo4j testcontainer (optional)

### Integration Tests
- **Milestone 5**: End-to-end with MockWriter
- **Milestone 13**: End-to-end with real Neo4j

### Performance Tests
- **Milestone 12**: Benchmark parsing, ingestion, query speed

---

## Quick Reference: Documentation Map

| Phase | Primary Docs | Supporting Docs |
|-------|-------------|-----------------|
| M1-M2 | ARCHITECTURE.md (Parser), BINARY_PARSING.md | ADDRESS_DERIVATION.md |
| M3-M5 | ARCHITECTURE.md (Domain), INGESTION_ARCHITECTURE.md | DATA_MODEL.md |
| M6 | ARCHITECTURE.md (Writer), NEO4J_INTEGRATION.md | CYPHER_EXAMPLES.md |
| M7 | MEMORY_STRATEGY.md | PERFORMANCE.md |
| M8 | INGESTION_ARCHITECTURE.md (Checkpoint) | DATA_MODEL.md (Checkpoint node) |
| M10 | SPECIAL_CASES.md | ADDRESS_DERIVATION.md |
| M11 | VALIDATION.md | - |

---

## Total Estimated Time: 3-5 days

- **Core implementation** (M1-M9): 2-3 days
- **Robustness** (M10-M11): 1 day
- **Optimization & Testing** (M12-M13): 1-2 days
- **Polish** (M14): Few hours

---

## Development Notes

### Current Milestone: M5 - Ingestion Orchestrator

### Blockers: None

### Recent Updates:
- 2025-01-23: **M4 completed** - GraphWriter trait with MockWriter implementation, 14 integration tests passing, no Neo4j dependencies yet
- 2025-01-23: **M3 completed** - Domain models (BlockData, TransactionData, OutputData, InputData, CheckpointData) with conversion functions from bitcoin crate types, 5 comprehensive tests passing
- 2025-01-23: **M2 completed** - Address derivation for all script types (P2PKH, P2SH, P2WPKH, P2WSH, P2TR, P2PK, NULL_DATA), all tests passing including Genesis block
- 2025-01-23: **M1 completed** - BlockFileReader parser with streaming API, all tests passing (<10ms for 100 blocks)
- 2025-01-23: Plan created with 14 incremental milestones

---

**This plan serves as the living roadmap for the Bitcoin Chain Graph project. Update status, add notes, and track blockers as development progresses.**
