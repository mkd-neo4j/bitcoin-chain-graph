# Bitcoin Chain Graph

High-performance Rust CLI that ingests Bitcoin blockchain data into Neo4j graph database for forensic analysis, network investigation, and blockchain analytics.

**This is a systems-level Rust project. There are no UI components, pages, or visualizations.**

## Stack

- **Language**: Rust (Edition 2021)
- **Async runtime**: Tokio (full features)
- **Database**: Neo4j via neo4rs 0.8
- **CLI**: clap 4.5 (derive macros)
- **Config**: TOML files via config 0.14 + toml 0.8
- **Logging**: tracing + tracing-subscriber (env-filter, fmt, ansi)
- **Error handling**: anyhow 1 + thiserror 1
- **Bitcoin**: bitcoin crate 0.31
- **Caching**: lru 0.12 (UTXO cache)
- **I/O**: memmap2 0.9 (memory-mapped files), reqwest 0.12 (RPC), zeromq 0.4 (real-time)
- **Serialization**: serde + serde_json, hex 0.4, chrono 0.4
- **Async utilities**: async-trait 0.1, futures 0.3, tokio-util 0.7
- **Storage**: rusty-leveldb 3.0 (block index)

## Architecture

Three-layer architecture with strict dependency direction:

```
Parser Layer (bitcoin crate types) → Domain Layer (primitive types) → Writer Layer (database ops)
```

- **Parser** (`src/parser/`): Reads Bitcoin Core `.blk` files via memory-mapped I/O, JSON-RPC, ZMQ
- **Domain** (`src/domain/`): Type-safe models, IngestionOrchestrator, 16-shard UTXO cache
- **Writer** (`src/writer/`): GraphWriter trait, Neo4jWriter (production), MockWriter (testing)
- **Config** (`src/config/`): TOML-based configuration with validation
- **CLI** (`src/main.rs`): 5 subcommands: init-schema, ingest, resume, status, live

### Key Abstraction

`IngestionOrchestrator<W: GraphWriter>` is the central coordinator. Generic over the GraphWriter trait, enabling MockWriter for testing and Neo4jWriter for production.

### Ingestion Phase Ordering

1. **Phase 1**: Block nodes + NEXT_BLOCK relationships
2. **Phase 2**: Output nodes + UTXO cache population (**BEFORE transactions!**)
3. **Phase 3**: Transaction nodes WITH amounts (calculated in Rust from cache)
4. **Phase 3.5**: HAS_OUTPUT relationships (Transaction → Output)
5. **Phase 4**: Input nodes + SPENDS relationships
6. **Phase 6**: PERFORMS + BENEFITS_TO relationships (pre-aggregated data)
7. **Phase 7**: Remove spent outputs from cache

**CRITICAL**: Phases 2 and 3 are swapped from naive ordering because Bitcoin allows same-block UTXO spending. Outputs must exist in cache before transaction amounts are calculated.

## Commands

```bash
cargo build                        # Debug build
cargo build --release              # Release build (optimized)
cargo check                        # Fast type checking (no codegen)
cargo fmt                          # Format all .rs files
cargo fmt -- --check               # Check formatting without modifying
cargo clippy -- -D warnings        # Lint with warnings-as-errors
cargo test                         # Run unit + integration tests (excludes #[ignore])
cargo test -- --ignored            # Run E2E tests requiring Neo4j
cargo test -- --nocapture          # Show stdout/stderr during tests
cargo doc --open                   # Generate and open rustdoc
```

### Application Commands

```bash
cargo run -- init-schema --config config/default.toml       # Initialize Neo4j schema
cargo run -- ingest --config config/default.toml             # Fresh ingestion from genesis
cargo run -- resume --config config/default.toml             # Resume from checkpoint
cargo run -- status --config config/default.toml             # Display checkpoint progress
cargo run -- live --config config/default.toml               # RPC catchup + ZMQ real-time
```

## Code Conventions

### Imports
- Absolute `crate::` imports only (never relative `super::` except within same file)
- Grouped: std → external crates → internal modules, separated by blank lines

### Naming
- PascalCase: types, traits, enums (`BlockData`, `GraphWriter`, `WriterError`)
- snake_case: functions, methods, modules, variables (`write_blocks`, `block_file`)
- SCREAMING_SNAKE_CASE: constants (`BIP30_DUPLICATE_HEIGHTS`, `EXPECTED_CONSTRAINTS`)

### Types
- Derive `Clone, Debug` on all domain types; add `Serialize, Deserialize` where needed
- Domain models use primitive types only (String, u32, u64, f64, i64, bool, Option, Vec)
- Generic over traits: `IngestionOrchestrator<W: GraphWriter>`
- `Arc<T>` for shared ownership across async boundaries
- `std::sync::Mutex` for interior mutability (not tokio::sync)

### Error Handling
- Custom error types with `#[derive(thiserror::Error, Debug)]` in each layer
- Result type aliases: `pub type Result<T> = std::result::Result<T, WriterError>;`
- `anyhow::Context` for error chain propagation in main.rs and orchestration code
- Never `.unwrap()` in production code (only in tests)
- `.expect()` only when the invariant is truly guaranteed

### Logging
- `tracing` macros everywhere: `tracing::info!()`, `tracing::warn!()`, `tracing::error!()`, `tracing::debug!()`
- Structured fields: `tracing::info!(height = height, hash = %hash, "Processing block")`
- NEVER use `println!` in library code (main.rs CLI output is the exception)

### Documentation
- `///` doc comments on ALL public items (structs, enums, functions, methods, traits, constants)
- Module-level `//!` doc comments at the top of every file
- Include `# Arguments`, `# Returns`, `# Errors`, `# Example` sections on complex public methods

### Async
- All database operations async using `#[async_trait]`
- `tokio::join!` for concurrent independent I/O
- `tokio::spawn` for parallel tasks with `Arc` shared state
- Graceful shutdown via `CancellationToken` from tokio-util

### Testing
- `#[test]` for sync unit tests, `#[tokio::test]` for async
- MockWriter for all domain/integration tests (no database needed)
- Real Neo4j tests marked `#[ignore]`
- Test data in `test_data/blk00000.dat`
- Test files mirror source: `tests/domain/`, `tests/parser/`, `tests/writer/`, `tests/integration/`

## Do NOTs

- **NEVER** use `.unwrap()` in `src/` code (only in tests)
- **NEVER** use `println!` in library code under `src/` (use tracing macros)
- **NEVER** put bitcoin crate types in domain model fields (conversion boundary in `domain/conversions.rs`)
- **NEVER** put neo4rs types in domain layer
- **NEVER** use `unsafe` code
- **NEVER** add TODO comments without a GitHub issue reference (`TODO(#123)`)
- **NEVER** skip `cargo fmt` after editing .rs files
- **NEVER** add new dependencies without documenting why in commit message
- **NEVER** modify the GraphWriter trait without updating BOTH MockWriter AND Neo4jWriter
- **NEVER** reorder ingestion phases without understanding same-block UTXO spending
- **NEVER** store secrets in config files committed to git (`config/` is gitignored)
- **NEVER** construct Cypher query strings at runtime — all queries are constants in `writer/neo4j/queries.rs`
- **NEVER** use individual database queries in loops — use UNWIND for bulk operations
