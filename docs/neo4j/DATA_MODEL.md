# Bitcoin Blockchain Data Model for Neo4j

A dual-layer graph model for storing raw Bitcoin blockchain data in Neo4j, designed for both financial crime investigation and blockchain forensics.

---

## Design Principles

1. **Raw data only** - Only store data that exists in the blockchain or can be deterministically derived from it. No external enrichment, clustering, or risk scoring in this layer.

2. **Dual-layer approach** - Provide both a simplified "follow the money" layer for investigations and a detailed UTXO layer for forensic analysis, connected through shared Transaction nodes.

3. **Semantic relationship direction** - Relationships flow in the direction of value movement. Inputs flow into transactions, outputs flow out.

---

## Blockchain Data Mapping

This model is designed to ingest data from Bitcoin Core raw block files (`.blk` files). Here's how blockchain data maps to our graph model:

### Block Data → Block Node
Raw block headers map directly to Block node properties:
- `height`, `hash`, `previousHash`, `merkleRoot`, `timestamp`, `bits`, `difficulty`, `nonce`, `version` → Direct mapping from block header
- `txCount` → Count of transactions in block
- `size`, `weight` → Calculated from block data

### Transaction Data → Transaction Node
Raw transaction data from blocks:
- `txid`, `version`, `locktime`, `size`, `vsize`, `weight` → Direct mapping from transaction
- `blockHeight`, `blockHash`, `timestamp` → Inherited from containing block
- `totalInput` → **Calculated**: Sum of all input amounts (requires looking up previous outputs)
- `totalOutput` → **Calculated**: Sum of all output amounts
- `fee` → **Calculated**: `totalInput - totalOutput` (zero for coinbase transactions)
- `isCoinbase` → **Derived**: True if transaction has exactly one input with no previous output reference

### Output Data → Output Node
Raw transaction output (vout) data:
- `outputId` → **Derived**: Concatenation of `{txid}:{outputIndex}`
- `outputIndex`, `amount`, `scriptPubKey` → Direct mapping from vout
- `scriptType` → **Derived**: Parsed from scriptPubKey (see [ADDRESS_DERIVATION.md](../bitcoin/ADDRESS_DERIVATION.md))
- `isSpent`, `spentInTxid`, `spentAtHeight` → **State tracking**: Updated when output is later consumed by an input

### Input Data → Input Node
Raw transaction input (vin) data:
- `inputId` → **Derived**: Concatenation of `{txid}:{inputIndex}`
- `inputIndex`, `previousTxid`, `previousOutputIndex`, `scriptSig`, `sequence` → Direct mapping from vin
- `witness` → Direct mapping from vin.txinwitness (SegWit transactions only)

### Address Data → Address Node
**Not present in raw blockchain** - Addresses are **deterministically derived** by:
1. Parsing the `scriptPubKey` from each output
2. Extracting the address based on script type (P2PKH, P2SH, P2WPKH, P2WSH, P2TR)
3. For details see [ADDRESS_DERIVATION.md](../bitcoin/ADDRESS_DERIVATION.md)

### Relationship Data
- `HAS_INPUT`, `HAS_OUTPUT`, `INCLUDED_IN`, `LOCKED_TO` → **Directly derivable** from transaction structure
- `SPENDS` → **Requires lookup**: Match `input.previousTxid:previousOutputIndex` to existing Output nodes
- `PERFORMS` → **Derived**: Follow `Input → SPENDS → Output → LOCKED_TO → Address` to find sender
- `BENEFITS_TO` → **Derived**: Follow `Output → LOCKED_TO → Address` to find recipient
- `NEXT_BLOCK` → **Derived**: Link blocks by sequential height

---

## Conceptual Overview

```
╔═══════════════════════════════════════════════════════════════════════════════════════╗
║  SIMPLIFIED LAYER (Follow the Money)                                                  ║
║                                                                                       ║
║     Alice ──PERFORMS──► Transaction ──BENEFITS_TO──► Bob                              ║
║                                     ──BENEFITS_TO──► Alice (change)                   ║
║                                                                                       ║
╠═══════════════════════════════════════════════════════════════════════════════════════╣
║  DETAILED LAYER (UTXO Mechanics)                                                      ║
║                                                                                       ║
║              Previous Output ──LOCKED_TO──► Alice                                     ║
║                      │                                                                ║
║                   SPENDS                                                              ║
║                      │                                                                ║
║                      ▼                                                                ║
║                   Input 0 ──HAS_INPUT──► Transaction ──HAS_OUTPUT──► Output 0         ║
║                                                       ──HAS_OUTPUT──► Output 1         ║
║                                                                          │             ║
║                                                                          │             ║
║                                                          Output 0 ──LOCKED_TO──► Bob  ║
║                                                          Output 1 ──LOCKED_TO──► Alice║
║                                                                                       ║
╚═══════════════════════════════════════════════════════════════════════════════════════╝
```

The Transaction node is shared between both layers. This allows simple money-flow queries while preserving the ability to drill into UTXO-level detail when needed.

---

## Why Two Layers?

**Simplified layer** answers: "Who sent money to whom?"
- Fast traversal for following funds across multiple hops
- Pattern matching for financial crime investigation
- Clean Address → Transaction → Address paths

**Detailed layer** answers: "What exactly happened in this transaction?"
- Which specific UTXOs were consumed (inputs)
- Which new UTXOs were created (outputs)
- Cryptographic proof data (scriptSig, witness)
- Precise UTXO tracking for balance calculation

---

## Node Definitions

### Address

A Bitcoin address that can send or receive funds.

| Property | Type | Description |
|----------|------|-------------|
| address | STRING | Bitcoin address (e.g., `1A1zP1...`, `bc1q...`, `bc1p...`) |
| type | STRING | Address type: `P2PKH`, `P2SH`, `P2WPKH`, `P2WSH`, `P2TR` |

Note: Address nodes are derived by parsing the scriptPubKey of outputs. They don't exist explicitly in raw blockchain data but are deterministically derivable.

---

### Transaction

A Bitcoin transaction that moves value.

| Property | Type | Description |
|----------|------|-------------|
| txid | STRING | Transaction hash (unique identifier) |
| blockHeight | INTEGER | Block number containing this transaction |
| blockHash | STRING | Hash of the containing block |
| timestamp | DATETIME | Block timestamp |
| totalInput | FLOAT | Sum of all input amounts (BTC) |
| totalOutput | FLOAT | Sum of all output amounts (BTC) |
| fee | FLOAT | Miner fee (totalInput - totalOutput) |
| size | INTEGER | Transaction size in bytes |
| vsize | INTEGER | Virtual size (SegWit) |
| weight | INTEGER | Transaction weight units |
| version | INTEGER | Transaction version |
| locktime | INTEGER | Locktime value |
| isCoinbase | BOOLEAN | True if this is a coinbase (mining reward) transaction |

---

### Output

A transaction output representing a "coin" that can be spent once.

| Property | Type | Description |
|----------|------|-------------|
| outputId | STRING | Unique identifier: `{txid}:{outputIndex}` |
| outputIndex | INTEGER | Position in transaction outputs (0, 1, 2...) |
| amount | FLOAT | Amount in BTC |
| scriptPubKey | STRING | Locking script (defines who can spend) |
| scriptType | STRING | Script type: `P2PKH`, `P2SH`, `P2WPKH`, `P2WSH`, `P2TR`, `NULL_DATA`, `UNKNOWN` |
| isSpent | BOOLEAN | Has this output been spent? |
| spentInTxid | STRING | Transaction ID that spent this output (null if unspent) |
| spentAtHeight | INTEGER | Block height when spent (null if unspent) |

An unspent output is called a UTXO (Unspent Transaction Output). The set of all UTXOs represents all spendable Bitcoin.

---

### Input

A transaction input that spends a previous output.

| Property | Type | Description |
|----------|------|-------------|
| inputId | STRING | Unique identifier: `{txid}:{inputIndex}` |
| inputIndex | INTEGER | Position in transaction inputs (0, 1, 2...) |
| previousTxid | STRING | Transaction ID containing the output being spent |
| previousOutputIndex | INTEGER | Index of the output being spent |
| scriptSig | STRING | Unlocking script (proves authorisation to spend) |
| sequence | INTEGER | Sequence number |
| witness | [STRING] | Witness data array (SegWit transactions) |

Coinbase transactions have a single input with no previous output reference.

---

### Block

A block in the blockchain.

| Property | Type | Description |
|----------|------|-------------|
| height | INTEGER | Block number (0 = genesis) |
| hash | STRING | Block hash |
| previousHash | STRING | Hash of previous block |
| merkleRoot | STRING | Merkle root of transactions |
| timestamp | DATETIME | When block was mined |
| txCount | INTEGER | Number of transactions |
| size | INTEGER | Block size in bytes |
| weight | INTEGER | Block weight (SegWit) |
| bits | STRING | Difficulty target (compact) |
| difficulty | FLOAT | Mining difficulty |
| nonce | INTEGER | Mining nonce |
| version | INTEGER | Block version |
| chainwork | STRING | Cumulative chain work |

---

### IngestionCheckpoint

Tracks the progress of blockchain ingestion to enable resume-on-failure.

| Property | Type | Description |
|----------|------|-------------|
| lastProcessedHeight | INTEGER | Last successfully ingested block height |
| lastProcessedHash | STRING | Hash of last processed block (for verification) |
| lastProcessedFile | STRING | Name of `.blk` file being processed (e.g., "blk00000.dat") |
| lastProcessedFileOffset | INTEGER | Byte offset within the file (optional optimization) |
| timestamp | DATETIME | When checkpoint was last updated |
| status | STRING | Current status: `in_progress`, `completed`, `paused`, `error` |

**Purpose**: Allows ingestion to resume from the last successfully processed block after a failure or interruption. Since each block is ingested in a single Neo4j transaction, at most one block needs to be reprocessed on resume.

**Note**: This is metadata for the ingestion process, not part of the blockchain data itself. It can be safely deleted and recreated without affecting the blockchain graph.

---

## Relationship Definitions

### Simplified Layer (Follow the Money)

| Relationship | Direction | Description |
|--------------|-----------|-------------|
| PERFORMS | `(:Address)-[:PERFORMS]->(:Transaction)` | Address whose UTXO was spent as an input |
| BENEFITS_TO | `(:Transaction)-[:BENEFITS_TO]->(:Address)` | Address that received an output |

These relationships are derived during ingest by examining which addresses controlled the spent inputs and which addresses received the outputs.

---

### Detailed Layer (UTXO Mechanics)

| Relationship | Direction | Description |
|--------------|-----------|-------------|
| HAS_INPUT | `(:Input)-[:HAS_INPUT]->(:Transaction)` | Input feeds into transaction |
| HAS_OUTPUT | `(:Transaction)-[:HAS_OUTPUT]->(:Output)` | Transaction produces output |
| SPENDS | `(:Input)-[:SPENDS]->(:Output)` | Input consumes a previous output |
| LOCKED_TO | `(:Output)-[:LOCKED_TO]->(:Address)` | Output is controlled by address |

---

### Block Structure

| Relationship | Direction | Description |
|--------------|-----------|-------------|
| INCLUDED_IN | `(:Transaction)-[:INCLUDED_IN]->(:Block)` | Transaction is in this block |
| NEXT_BLOCK | `(:Block)-[:NEXT_BLOCK]->(:Block)` | Links blocks in chain order |

---

## Complete Relationship Diagram

```
                                    NEXT_BLOCK
                              ┌───────────────────┐
                              │                   │
                              ▼                   │
                           Block N ◄────────── Block N+1
                              ▲
                              │
                         INCLUDED_IN
                              │
┌─────────────────────────────┴─────────────────────────────┐
│                                                           │
│                       Transaction                         │
│                                                           │
│    ┌──────────────────────┬───────────────────────┐       │
│    │                      │                       │       │
│    │    PERFORMS          │          BENEFITS_TO  │       │
│    │    (address whose    │          (address     │       │
│    │    UTXO was spent)   │          receiving    │       │
│    │                      │          output)      │       │
│    ▼                      │                       ▼       │
│  Address                  │                    Address    │
│    ▲                      │                       ▲       │
│    │                      │                       │       │
│    │ LOCKED_TO            │            LOCKED_TO  │       │
│    │                      │                       │       │
│  Output ◄────── SPENDS ── Input ─── HAS_INPUT ──► Transaction ─── HAS_OUTPUT ──► Output
│ (previous)                                                                          │
│                                                                                     │
│                                                                                     │
│                                                                          LOCKED_TO  │
│                                                                                     │
│                                                                                  Address
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘
```
