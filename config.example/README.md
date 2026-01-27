# Bitcoin Chain Graph - Configuration

This directory contains the example configuration template. Copy it and customise for your environment.

## Quick Start

1. **Copy the example config:**
   ```bash
   mkdir -p config
   cp config.example/config.toml.example config/default.toml
   ```

2. **Edit `config/default.toml`:**
   - Set `bitcoin.blocks_dir` to your Bitcoin blocks directory
   - Set `neo4j.password` to your Neo4j password
   - For live mode: uncomment the `[bitcoin_rpc]` section and set credentials

3. **Run:**
   ```bash
   bitcoin-chain-graph --config config/default.toml
   ```

The example file includes inline comments on every setting explaining how to scale up or down for your hardware.

## Scaling Guide

The defaults are tuned for a mid-range server (4-8 cores, 16-32GB RAM). Use the table below to adjust for your hardware:

| Setting | Low Resource | Default | High Performance | Ultra |
|---------|-------------|---------|-----------------|-------|
| **Hardware** | 1-2 cores, 2GB | 4-8 cores, 16-32GB | 16+ cores, 64GB+ | 40+ cores, 128GB+ |
| `neo4j.max_connections` | 4 | 20 | 50 | 100 |
| `neo4j.min_connections` | 1 | 2 | 4 | 16 |
| `neo4j.write_batch_size` | 1000 | 5000 | 10000 | 20000 |
| `ingestion.batch_size` | 10-50 | 5000 | 5000 | 5000-10000 |
| `ingestion.max_batch_memory_mb` | 128 | 512 | 2048 | 4096 |
| `performance.utxo_cache_memory_mb` | 2 | 140 | 500 | 1400 |
| `performance.parallel_batches` | 1 | 4 | 8-16 | 16-32 |
| `performance.progress_report_interval` | 50 | 500 | 500 | 1000 |

## Configuration Sections

### [bitcoin] - Blockchain Data

```toml
[bitcoin]
blocks_dir = "/path/to/bitcoin/blocks"
start_height = 0
# end_height = 100000   # optional: stop at this height
```

**Common Bitcoin Core paths:**
- **Linux:** `~/.bitcoin/blocks` or `/data/bitcoin/blocks`
- **macOS:** `~/Library/Application Support/Bitcoin/blocks`
- **Windows:** `%APPDATA%\Bitcoin\blocks`
- **Development:** `./test_data/blocks`

### [neo4j] - Database Connection

```toml
[neo4j]
uri = "bolt://localhost:7687"
user = "neo4j"
password = "CHANGE_ME"
database = "neo4j"
max_connections = 20
min_connections = 2
connection_timeout_secs = 30
fetch_size = 500
max_retries = 3
write_batch_size = 5000
```

### [ingestion] - Ingestion Process

```toml
[ingestion]
batch_size = 5000
max_batch_memory_mb = 512
enable_validation = true
validate_every_n_blocks = 10000
checkpoint_interval = 10
auto_resume = true
validate_on_resume = true
```

### [performance] - Performance Tuning

```toml
[performance]
utxo_cache_memory_mb = 140    # ~1.9M entries, hit rate 85-95%
utxo_prewarm_depth = 1000000  # blocks to scan backwards for cache warming
parallel_batches = 4
progress_report_interval = 500
```

**UTXO Cache Memory Guidelines:**
- **2 MB**: ~28k entries, hit rate ~40-50% (low resource)
- **15 MB**: ~208k entries (minimal)
- **140 MB**: ~1.9M entries, hit rate 85-95% (recommended)
- **500 MB**: ~6.9M entries (high performance)
- **1400 MB**: ~19.4M entries, hit rate 95-99% (ultra)

Higher memory = better cache hit rate = fewer Neo4j queries = faster ingestion.

### [bitcoin_rpc] - Live Mode (Optional)

Only needed for `live` command (RPC catchup + ZMQ real-time streaming). This section is commented out in the example by default.

```toml
[bitcoin_rpc]
url = "http://localhost:8332"
user = "btcgraph"
password = "CHANGE_ME"
batch_size = 200
timeout_secs = 30
zmq_endpoint = "tcp://127.0.0.1:28332"
```

Requires Bitcoin Core running with RPC and ZMQ enabled in `bitcoin.conf`:
```
rpcuser=btcgraph
rpcpassword=your-rpc-password
zmqpubhashblock=tcp://127.0.0.1:28332
```

### [logging] - Log Configuration

```toml
[logging]
level = "info"        # trace, debug, info, warn, error
json_format = false   # true for structured JSON output
```

## Security Best Practices

**Never commit actual configuration files to git!**

- Example files (`.toml.example`) are safe to commit (no secrets)
- Actual configs (`config/*.toml`) are gitignored automatically
- Always change default passwords before production use
- Use environment variables for CI/CD deployments

## Environment Variables

As an alternative to config files, use environment variables with the `BITCOIN_GRAPH_` prefix:

```bash
export BITCOIN_GRAPH_BITCOIN_BLOCKS_DIR="/data/bitcoin/blocks"
export BITCOIN_GRAPH_NEO4J_URI="bolt://neo4j-server:7687"
export BITCOIN_GRAPH_NEO4J_PASSWORD="secret"
export BITCOIN_GRAPH_INGESTION_BATCH_SIZE="5000"

# Then run without --config flag
bitcoin-chain-graph
```

## Troubleshooting

### Config file not found
```
Error: Failed to load config file: No such file or directory
```
**Solution:** Copy the example to `config/default.toml`:
```bash
cp config.example/config.toml.example config/default.toml
```

### Validation errors
```
Error: Invalid configuration: batch_size must be > 0
```
**Solution:** Check all required fields are set and have valid values.

### Neo4j connection failed
```
Error: Failed to connect to Neo4j: Connection refused
```
**Solution:** Verify Neo4j is running and the URI/credentials are correct.

## Performance Tuning Tips

### For Early Blockchain (2009-2012)
- High batch sizes (5000+ blocks)
- Maximum parallelism
- Less frequent validation
- **Expected:** 50-100+ blocks/sec

### For Modern Blockchain (2018+)
- Same batch size (5000 blocks) works fine
- Balanced parallelism
- More frequent validation if needed
- **Expected:** 1-5 blocks/sec

### Memory vs Speed Trade-offs
- Larger `utxo_cache_memory_mb` = fewer Neo4j lookups = faster ingestion
- More `parallel_batches` = higher throughput but more Neo4j connections
- More `max_connections` = more database resources needed
