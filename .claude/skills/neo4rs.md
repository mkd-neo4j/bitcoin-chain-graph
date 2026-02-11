---
name: neo4rs
description: neo4rs 0.8 Rust driver patterns for Neo4j database operations
---

# neo4rs 0.8 Driver Patterns

## Connection Setup

```rust
use neo4rs::{ConfigBuilder, Graph, query};

let config = ConfigBuilder::default()
    .uri("bolt://localhost:7687")
    .user("neo4j")
    .password("password")
    .db("neo4j")
    .max_connections(20)
    .fetch_size(500)
    .build()
    .map_err(|e| WriterError::ConnectionFailed(format!("Config error: {}", e)))?;

let graph = Graph::connect(config).await
    .map_err(|e| WriterError::ConnectionFailed(format!("Connection error: {}", e)))?;
```

## Executing Queries

```rust
// Fire-and-forget (no results needed)
graph.run(query("CREATE (n:Test {id: 1})")).await?;

// With parameters
graph.run(
    query("MATCH (n {id: $id}) SET n.prop = $value")
        .param("id", "some-id")
        .param("value", 42_i64)
).await?;

// Reading results
let mut result = graph.execute(
    query("MATCH (n:Block) RETURN n.height AS height LIMIT 10")
).await?;
while let Some(row) = result.next().await? {
    let height: i64 = row.get("height")?;
}
```

## BoltType Conversions

```rust
use neo4rs::{BoltMap, BoltType};

fn to_bolt_map(block: &BlockData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("height".into(), (block.height as i64).into());
    map.put("hash".into(), block.hash.as_str().into());
    map.put("timestamp".into(), block.timestamp.into()); // i64 natively
    map.put("difficulty".into(), block.difficulty.into()); // f64 natively
    map.put("isCoinbase".into(), block.is_coinbase.into()); // bool natively
    map
}

fn to_bolt_list(items: &[BlockData]) -> Vec<BoltType> {
    items.iter().map(|d| BoltType::Map(to_bolt_map(d))).collect()
}

// Pass to UNWIND query
let bolt_data = to_bolt_list(&blocks);
graph.run(
    query("UNWIND $items AS item CREATE (b:Block {height: item.height})")
        .param("items", bolt_data.as_slice())
).await?;
```

## Type Mapping: Rust → Neo4j

| Rust Type | Neo4j Type | Notes |
|-----------|------------|-------|
| `&str` / `String` | BoltString | Both work |
| `i64` | BoltInteger | **u32/u64 must cast with `as i64`!** |
| `f64` | BoltFloat | Direct |
| `bool` | BoltBoolean | Direct |
| `Vec<BoltType>` | BoltList | For UNWIND params |
| `BoltMap` | BoltMap | For objects in UNWIND |
| `Option<T>` | Use conditional insertion | Or `BoltType::Null` |

## Known Quirks

- **i64 -1 bug**: neo4rs may misread i64 `-1` as 255 (unsigned byte interpretation). Use sentinel value `-999` instead of `-1`.
- **All integers must be i64**: Even if your Rust type is u32 or u64, cast with `as i64` before passing to neo4rs.
- **BoltMap keys**: Must be `BoltString` — use `.into()` to convert `&str`.
- **Empty results**: `result.next().await?` returns `None` when no more rows.

## Batched Write Pattern (from this project)

```rust
async fn execute_batched<T, F>(
    &self,
    items: &[T],
    query_str: &str,
    param_name: &str,
    operation_name: &str,
    convert: F,
) -> Result<()>
where
    F: Fn(&[T]) -> Vec<BoltType>,
{
    for (i, chunk) in items.chunks(self.batch_size).enumerate() {
        let bolt_data = convert(chunk);
        let q = query(query_str).param(param_name, bolt_data.as_slice());
        self.run_with_retry(&q, operation_name).await?;
        tracing::debug!(
            batch = i + 1,
            chunk_size = chunk.len(),
            "{} batch complete",
            operation_name
        );
    }
    Ok(())
}
```

## Retry with Timeout

```rust
async fn run_with_retry(&self, q: &Query, operation: &str) -> Result<()> {
    let mut attempts = 0;
    let mut delay = Duration::from_millis(200);

    loop {
        attempts += 1;
        match tokio::time::timeout(self.query_timeout, self.graph.run(q.clone())).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) if attempts < self.max_retries => {
                tracing::warn!(attempt = attempts, error = %e, "Retrying {}", operation);
                tokio::time::sleep(delay).await;
                delay *= 2; // exponential backoff
            }
            Ok(Err(e)) => return Err(WriterError::QueryFailed(format!("{}: {}", operation, e))),
            Err(_) => {
                if attempts >= self.max_retries {
                    return Err(WriterError::QueryFailed(format!("{}: timeout", operation)));
                }
                tracing::warn!(attempt = attempts, "Timeout on {}, retrying", operation);
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
        }
    }
}
```

## Health Check Pattern

```rust
pub async fn health_check(&self) -> Result<()> {
    self.graph
        .run(query("RETURN 1"))
        .await
        .map_err(|e| WriterError::ConnectionFailed(format!("Health check failed: {}", e)))
}
```
