# Parser Layer

Reads Bitcoin blockchain data from multiple sources and converts to types the Domain layer can use.

## Key Files

- `block_file.rs` — Memory-mapped `.blk` file reader using memmap2. Streams blocks one at a time.
- `block_index.rs` — Reads Bitcoin Core's LevelDB block index for height-to-file mapping.
- `single_block_loader.rs` — Height-addressed block loading with lazy index building and O(1) lookup.
- `address.rs` — Address extraction from scriptPubKey. Supports all 7 standard script types.
- `rpc_provider.rs` — Bitcoin Core JSON-RPC client for fetching blocks by height/hash.
- `zmq_listener.rs` — ZMQ subscriber for real-time block hash notifications.

## Address Extraction

`extract_address()` handles all standard Bitcoin script types:

| Type | Description | Address Format |
|------|-------------|----------------|
| P2PKH | Pay to Public Key Hash | Legacy `1...` |
| P2SH | Pay to Script Hash | Legacy `3...` |
| P2WPKH | SegWit v0 (20-byte witness) | Bech32 `bc1q...` |
| P2WSH | SegWit v0 (32-byte witness) | Bech32 `bc1q...` |
| P2TR | Taproot (SegWit v1) | Bech32m `bc1p...` |
| P2PK | Pay to Public Key (obsolete) | Derived via pubkey hash → `1...` |
| NullData | OP_RETURN (unspendable) | No address |
| Unknown | Non-standard scripts | No address |

For P2PK scripts (genesis era), we manually derive a P2PKH-style address by hashing the public key.

## Binary Parsing

- `.blk` files use magic bytes (`0xF9BEB4D9` mainnet) as block delimiters
- Block size is 4-byte little-endian after magic bytes
- All parsing uses `bitcoin::consensus::deserialize()`
- Memory-mapped I/O via memmap2 for efficient large file access
- Streaming: one block at a time, not full file load

## Conventions

- Functions return `anyhow::Result<T>` for file I/O operations
- `bitcoin::consensus::deserialize()` for block deserialization
- Network enum (Bitcoin, Testnet, Regtest) determines magic bytes and address prefixes
- `ScriptType` and `AddressInfo` are output types — never cross the boundary into domain models as bitcoin crate types
- All hex encoding/decoding uses the `hex` crate
