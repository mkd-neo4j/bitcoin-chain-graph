# Bitcoin Chain Graph - Configuration Examples

This directory contains example configuration files for different deployment scenarios. These are **templates only** and should be copied and customized for your environment.

## Quick Start

1. **Copy an example config:**
   ```bash
   mkdir -p config
   cp config.example/default.toml.example config/config.toml
   ```

2. **Edit `config/config.toml`:**
   - Set `bitcoin.blocks_dir` to your Bitcoin blocks directory
   - Set `neo4j.password` to your Neo4j password
   - Adjust performance settings for your hardware

3. **Run with custom config:**
   ```bash
   bitcoin-chain-graph --config config/config.toml
   ```

## Configuration Profiles

### **low-resource.toml.example** - Testing & Development
- **Hardware:** 1-2 cores, 2GB RAM
- **Use cases:** Laptop, Raspberry Pi, small VPS, local testing
- **Settings:**
  - 2 Neo4j connections
  - Batch size: 10 blocks
  - Sequential processing (no parallelism)
  - Small UTXO cache (10,000 entries)
- **blocks_dir default:** `./test_data/blocks` (for development)

### **default.toml.example** - Standard Server
- **Hardware:** 4-8 cores, 8-16GB RAM
- **Use cases:** Standard server, developer workstation, small production
- **Settings:**
  - 10 Neo4j connections
  - Batch size: 50 blocks
  - 4 parallel batches
  - Moderate UTXO cache (100,000 entries)
- **blocks_dir default:** `/data/bitcoin/blocks` (production path)

### **high-performance.toml.example** - Production Server
- **Hardware:** 16+ cores, 64GB+ RAM
- **Use cases:** Production ingestion, dedicated server
- **Settings:**
  - 32 Neo4j connections
  - Batch size: 200 blocks
  - 16 parallel batches
  - Large UTXO cache (1,000,000 entries)
- **blocks_dir default:** `/data/bitcoin/blocks`

### **ultra-performance.toml.example** - Maximum Throughput
- **Hardware:** 40+ cores, 128GB+ RAM
- **Use cases:** Bulk historical ingestion, dedicated hardware
- **Settings:**
  - 100 Neo4j connections
  - Batch size: 500 blocks
  - 32 parallel batches
  - Massive UTXO cache (10,000,000 entries)
- **blocks_dir default:** `/data/bitcoin/blocks`

## Configuration Sections

### [bitcoin] - Blockchain Data
```toml
[bitcoin]
# Path to directory containing blk*.dat files
blocks_dir = "/path/to/bitcoin/blocks"

# Starting block height (0 = genesis)
start_height = 0

# Optional ending height (commented = sync to tip)
# end_height = 100000
```

**Common Bitcoin Core paths:**
- **Linux:** `~/.bitcoin/blocks` or `/data/bitcoin/blocks`
- **macOS:** `~/Library/Application Support/Bitcoin/blocks`
- **Windows:** `%APPDATA%\Bitcoin\blocks`
- **Development:** `./test_data/blocks`

### [neo4j] - Database Connection
```toml
[neo4j]
uri = "bolt://localhost:7687"    # Neo4j Bolt URI
user = "neo4j"                    # Database username
password = "CHANGE_ME"            # Database password
database = "neo4j"                # Database name
max_connections = 10              # Connection pool max
min_connections = 2               # Connection pool min
connection_timeout_secs = 30      # Connection timeout
fetch_size = 500                  # Rows per fetch
max_retries = 3                   # Query retry attempts
```

### [ingestion] - Ingestion Process
```toml
[ingestion]
batch_size = 50                   # Blocks per batch
max_batch_memory_mb = 512         # Memory limit per batch
enable_validation = true          # Run validation queries
validate_every_n_blocks = 1000    # Validation frequency
```

### [performance] - Performance Tuning
```toml
[performance]
utxo_cache_memory_mb = 15         # UTXO cache memory budget in MB
                                  # 15 MB ≈ 108k entries (default)
                                  # 50 MB ≈ 362k entries (recommended for batch_size=500)
                                  # 140 MB ≈ 1M entries (high performance)
utxo_prewarm_depth = 50           # Blocks to pre-warm cache (backward loading)
parallel_batches = 4              # Concurrent batch writes
progress_report_interval = 100    # Progress every N blocks
```

**UTXO Cache Memory Guidelines:**
- **2 MB** (low-resource): ~14k entries, hit rate ~40-50%
- **15 MB** (default): ~108k entries, good for batch_size ≤ 200
- **50 MB** (recommended): ~362k entries, good for batch_size = 500
- **140 MB** (high-perf): ~1M entries, good for batch_size = 1000+
- **1400 MB** (ultra-perf): ~10M entries, maximum hit rate (95-99%)

Higher memory = better cache hit rate = fewer Neo4j queries = faster ingestion.

## Security Best Practices

⚠️ **Never commit actual configuration files to git!**

- Example files (`.toml.example`) are safe to commit (no secrets)
- Actual configs (`config/*.toml`) are gitignored automatically
- Always change default passwords before production use
- Use environment variables for CI/CD deployments

## Environment Variables

As an alternative to config files, you can use environment variables:

```bash
export BITCOIN_GRAPH_BITCOIN_BLOCKS_DIR="/data/bitcoin/blocks"
export BITCOIN_GRAPH_NEO4J_URI="bolt://neo4j-server:7687"
export BITCOIN_GRAPH_NEO4J_PASSWORD="secret"
export BITCOIN_GRAPH_INGESTION_BATCH_SIZE="100"

# Then run without --config flag
bitcoin-chain-graph
```

## Troubleshooting

### Config file not found
```
Error: Failed to load config file: No such file or directory
```
**Solution:** Make sure you've copied an example to `config/config.toml`

### Validation errors
```
Error: Invalid configuration: batch_size must be > 0
```
**Solution:** Check all required fields are set and have valid values

### Neo4j connection failed
```
Error: Failed to connect to Neo4j: Connection refused
```
**Solution:** Verify Neo4j is running and the URI/credentials are correct

## Performance Tuning Guide

### For Early Blockchain (2009-2012)
- High batch sizes (200-500 blocks)
- Maximum parallelism
- Less frequent validation
- **Expected:** 200-400 blocks/sec

### For Modern Blockchain (2018+)
- Moderate batch sizes (50-100 blocks)
- Balanced parallelism
- More frequent validation
- **Expected:** 10-20 blocks/sec

### Memory vs Speed Trade-offs
- Larger `batch_size` = More memory, higher throughput
- Larger `utxo_cache_memory_mb` = More memory, better cache hit rate, faster ingestion
- More `parallel_batches` = More connections, higher throughput
- More `max_connections` = More database resources needed

**Cache Size Recommendations:**
- For `batch_size = 500`: Set `utxo_cache_memory_mb = 50` (≈362k entries)
- For `batch_size = 1000`: Set `utxo_cache_memory_mb = 140` (≈1M entries)
- Rule of thumb: Cache should hold 1 batch + working set (50k-100k unspent outputs)

## Support

For issues or questions:
- GitHub Issues: https://github.com/yourusername/bitcoin-chain-graph/issues
- Documentation: See main README.md
