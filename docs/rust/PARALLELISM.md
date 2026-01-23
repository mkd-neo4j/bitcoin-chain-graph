# Parallel Processing Strategies

Leveraging Rust's concurrency primitives (tokio, rayon) for high-performance Bitcoin blockchain ingestion.

---

## Overview

Parallelism strategies for ingestion:
1. **Async I/O with Tokio**: Overlap block parsing with Neo4j writes
2. **Data parallelism with Rayon**: Process independent blocks on multiple cores
3. **Channel-based pipeline**: Parser → Processor → Ingestor
4. **Batch-level parallelism**: Process multiple batches concurrently

---

## Strategy 1: Async I/O with Tokio

### Pattern: Overlap Parsing and Neo4j Writes

```rust
use tokio::sync::mpsc;
use tokio::task;

pub struct AsyncIngestion {
    neo4j_client: Neo4jClient,
}

impl AsyncIngestion {
    pub async fn ingest_blocks(mut self, block_files: Vec<String>) -> Result<()> {
        // Channel: parser sends blocks to ingestor
        let (tx, mut rx) = mpsc::channel::<Block>(100); // Buffer 100 blocks

        // Spawn parser task (runs concurrently)
        let parser_handle = task::spawn(async move {
            for file_path in block_files {
                let mut reader = BlockFileReader::new(&file_path, Network::Bitcoin)?;
                while let Some(block) = reader.next_block()? {
                    tx.send(block).await.map_err(|_| Error::ChannelClosed)?;
                }
            }
            Ok::<_, Error>(())
        });

        // Ingestor task (main thread)
        while let Some(block) = rx.recv().await {
            self.neo4j_client.ingest_block(&block).await?;
        }

        // Wait for parser to finish
        parser_handle.await??;

        Ok(())
    }
}
```

**Performance Gain**: 10-20% (overlaps disk I/O with network I/O)

---

## Strategy 2: Data Parallelism with Rayon

### Pattern: Parallel Block Parsing

```rust
use rayon::prelude::*;

pub fn parse_block_files_parallel(file_paths: Vec<String>) -> Result<Vec<Block>> {
    // Parse multiple files in parallel
    let all_blocks: Vec<Vec<Block>> = file_paths
        .par_iter()
        .map(|path| {
            let mut reader = BlockFileReader::new(path, Network::Bitcoin)?;
            let mut blocks = Vec::new();
            while let Some(block) = reader.next_block()? {
                blocks.push(block);
            }
            Ok(blocks)
        })
        .collect::<Result<Vec<_>>>()?;

    // Flatten into single vector
    Ok(all_blocks.into_iter().flatten().collect())
}
```

**Caveat**: Blocks must be re-sorted by height after parallel parsing (files contain blocks out-of-order).

---

## Strategy 3: Multi-Stage Pipeline

### Pattern: Parser → Processor → Ingestor

```rust
use tokio::sync::mpsc;

pub struct PipelineIngestion {
    neo4j_client: Neo4jClient,
}

impl PipelineIngestion {
    pub async fn run_pipeline(self, block_files: Vec<String>) -> Result<()> {
        // Stage 1: Parser (produces raw blocks)
        let (parse_tx, parse_rx) = mpsc::channel::<Block>(100);

        // Stage 2: Processor (derives addresses, prepares data)
        let (process_tx, process_rx) = mpsc::channel::<ProcessedBlock>(100);

        // Spawn parser task
        let parser_handle = tokio::spawn(async move {
            for file in block_files {
                let mut reader = BlockFileReader::new(&file, Network::Bitcoin)?;
                while let Some(block) = reader.next_block()? {
                    parse_tx.send(block).await?;
                }
            }
            Ok::<_, Error>(())
        });

        // Spawn processor task
        let processor_handle = tokio::spawn(async move {
            let mut parse_rx = parse_rx;
            while let Some(block) = parse_rx.recv().await {
                let processed = process_block(block)?;
                process_tx.send(processed).await?;
            }
            Ok::<_, Error>(())
        });

        // Ingestor (main thread)
        let mut process_rx = process_rx;
        while let Some(processed_block) = process_rx.recv().await {
            self.neo4j_client.ingest_processed_block(processed_block).await?;
        }

        // Wait for all stages
        parser_handle.await??;
        processor_handle.await??;

        Ok(())
    }
}

fn process_block(block: Block) -> Result<ProcessedBlock> {
    // Extract data, derive addresses (CPU-bound work)
    todo!()
}
```

**Performance Gain**: 20-30% (overlaps parsing, processing, and Neo4j writes)

---

## Strategy 4: Parallel Independent Batches

### Identifying Independent Blocks

```rust
pub fn identify_independent_ranges(total_blocks: u32, batch_size: u32) -> Vec<(u32, u32)> {
    // Strategy: Process distant ranges in parallel (no UTXO overlap)
    // Example: [0-1000], [500000-501000], [700000-701000]

    let ranges = vec![
        (0, batch_size),
        (500_000, 500_000 + batch_size),
        (700_000, 700_000 + batch_size),
    ];

    ranges
}
```

### Parallel Batch Ingestion

```rust
use rayon::prelude::*;

pub async fn ingest_ranges_parallel(
    neo4j_uri: String,
    ranges: Vec<(u32, u32)>
) -> Result<()> {
    // Process each range in parallel
    let results: Vec<Result<()>> = ranges
        .par_iter()
        .map(|(start, end)| {
            // Each worker gets own Neo4j client
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            runtime.block_on(async {
                let client = Neo4jClient::new(&neo4j_uri, "neo4j", "password").await?;
                for height in *start..*end {
                    let block = read_block(height)?;
                    client.ingest_block(&block).await?;
                }
                Ok(())
            })
        })
        .collect();

    // Check for errors
    for result in results {
        result?;
    }

    Ok(())
}
```

**Caveat**: Only works for blocks with no UTXO dependencies. Most real-world ingestion requires sequential processing.

**Performance Gain**: 2-4x on multi-core CPUs (for independent ranges only)

---

## Strategy 5: Worker Pool Pattern

### Pattern: Fixed Number of Workers

```rust
use tokio::sync::mpsc;
use tokio::task::JoinSet;

pub struct WorkerPool {
    num_workers: usize,
    neo4j_uri: String,
}

impl WorkerPool {
    pub async fn ingest_blocks(&self, blocks: Vec<Block>) -> Result<()> {
        // Channel for distributing work
        let (tx, rx) = mpsc::channel::<Block>(self.num_workers * 2);
        let rx = Arc::new(Mutex::new(rx));

        // Spawn workers
        let mut workers = JoinSet::new();
        for worker_id in 0..self.num_workers {
            let neo4j_uri = self.neo4j_uri.clone();
            let rx = Arc::clone(&rx);

            workers.spawn(async move {
                let client = Neo4jClient::new(&neo4j_uri, "neo4j", "password").await?;

                loop {
                    // Get next block from queue
                    let block = {
                        let mut rx = rx.lock().await;
                        match rx.recv().await {
                            Some(b) => b,
                            None => break, // Channel closed
                        }
                    };

                    // Ingest block
                    client.ingest_block(&block).await?;
                }

                Ok::<_, Error>(())
            });
        }

        // Feed blocks to workers
        for block in blocks {
            tx.send(block).await?;
        }
        drop(tx); // Close channel

        // Wait for all workers
        while let Some(result) = workers.join_next().await {
            result??;
        }

        Ok(())
    }
}
```

**Configuration**:
- 1-2 workers: Low memory systems
- 4 workers: Standard (one per CPU core)
- 8+ workers: High-performance systems

---

## Backpressure Handling

### Problem: Parser Faster Than Ingestor

```rust
// BAD: Unbounded channel (memory overflow)
let (tx, rx) = mpsc::unbounded_channel();

// GOOD: Bounded channel (automatic backpressure)
let (tx, rx) = mpsc::channel(100); // Buffer 100 blocks

// When buffer full, sender blocks until space available
tx.send(block).await?; // Blocks if buffer full
```

---

## Error Handling in Parallel Contexts

### Pattern: Collect Errors from Workers

```rust
use tokio::task::JoinSet;

pub async fn ingest_parallel(batches: Vec<BatchData>) -> Result<Vec<Error>> {
    let mut tasks = JoinSet::new();
    let mut errors = Vec::new();

    for batch in batches {
        tasks.spawn(async move {
            ingest_batch(batch).await
        });
    }

    // Collect results
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(_)) => {}, // Success
            Ok(Err(e)) => errors.push(e), // Worker error
            Err(e) => errors.push(Error::TaskPanic(e.to_string())), // Task panic
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::MultipleErrors(errors))
    }
}
```

---

## Synchronization Primitives

### Arc for Shared State

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SharedUtxoCache {
    cache: Arc<Mutex<UtxoCache>>,
}

impl SharedUtxoCache {
    pub fn new(capacity: usize, neo4j_client: Neo4jClient) -> Self {
        Self {
            cache: Arc::new(Mutex::new(UtxoCache::new(capacity, neo4j_client))),
        }
    }

    pub async fn get(&self, output_id: &str) -> Result<CachedOutput> {
        let mut cache = self.cache.lock().await;
        cache.get(output_id).await
    }

    pub fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
        }
    }
}

// Usage in workers
let shared_cache = SharedUtxoCache::new(100_000, neo4j_client);

for worker_id in 0..num_workers {
    let cache = shared_cache.clone();
    tokio::spawn(async move {
        let output = cache.get("txid:0").await?;
        // ...
    });
}
```

---

## Benchmarking Parallel Strategies

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_strategies(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("sequential ingestion", |b| {
        b.to_async(&runtime).iter(|| async {
            ingest_sequential(black_box(blocks.clone())).await
        });
    });

    c.bench_function("async pipeline", |b| {
        b.to_async(&runtime).iter(|| async {
            ingest_async_pipeline(black_box(blocks.clone())).await
        });
    });

    c.bench_function("worker pool (4 workers)", |b| {
        b.to_async(&runtime).iter(|| async {
            ingest_worker_pool(black_box(blocks.clone()), 4).await
        });
    });
}

criterion_group!(benches, benchmark_strategies);
criterion_main!(benches);
```

---

## Configuration Matrix

| Strategy | Throughput | Memory | Complexity | Best For |
|----------|-----------|--------|------------|----------|
| Sequential | 1x | Low | Simple | Small datasets, limited resources |
| Async I/O | 1.2x | Low | Medium | Overlapping I/O operations |
| Pipeline | 1.3x | Medium | Medium | Multi-stage processing |
| Worker Pool | 2-4x | Medium | High | CPU-bound processing |
| Parallel Batches | 3-8x | High | Very High | Independent data ranges |

---

## Recommended Configuration

```toml
# config.toml
[parallelism]
# Async I/O pipeline
enable_async_pipeline = true
parser_buffer_size = 100

# Worker pool
num_workers = 4                # Match CPU core count
neo4j_pool_size = 8            # 2x workers

# Backpressure
max_in_flight_batches = 10     # Limit concurrent batches
```

---

## References

- [Tokio Documentation](https://tokio.rs/)
- [Rayon Documentation](https://docs.rs/rayon/latest/rayon/)
- [Rust Async Book](https://rust-lang.github.io/async-book/)
- [PERFORMANCE.md](PERFORMANCE.md) - Performance optimization guide

---

## Next Steps

1. Read [PERFORMANCE.md](PERFORMANCE.md) for profiling parallel code
2. Read [TESTING.md](TESTING.md) for testing concurrent code
3. Experiment with different worker counts and buffer sizes for your hardware
