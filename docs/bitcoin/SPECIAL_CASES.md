# Special Cases in Bitcoin Blockchain Ingestion

Edge cases and special handling required during ingestion into Neo4j.

---

## Overview

While most Bitcoin transactions follow standard patterns, several special cases require unique handling during ingestion. This document covers coinbase transactions, OP_RETURN outputs, the genesis block, and other edge cases.

---

## 1. Coinbase Transactions

### What is a Coinbase Transaction?

The **first transaction in every block** is the coinbase transaction - the mining reward that creates new bitcoin. It has unique properties:

**Characteristics:**
- Always the first transaction (index 0) in every block
- Has exactly **one input** with no previous output reference
- The input's `previousTxid` is all zeros (`0x00...00`)
- The input's `previousOutputIndex` is `0xFFFFFFFF` (4294967295)
- The input's `scriptSig` contains arbitrary data (block height + extra nonce)
- May contain `witness` data (post-SegWit blocks include witness commitment in coinbase)
- Creates new bitcoin (block reward + transaction fees)

### Detection

```python
def is_coinbase(transaction):
    return (
        len(transaction.inputs) == 1 and
        transaction.inputs[0].previousTxid == "0" * 64 and
        transaction.inputs[0].previousOutputIndex == 0xFFFFFFFF
    )
```

### Ingestion Handling

**Phase 2 - Create Outputs:**
- Process outputs normally - coinbase outputs are spendable like any other UTXO
- Derive addresses normally (usually P2PK in early blocks, P2PKH or P2WPKH in modern blocks)

**Phase 3 - Create Transaction (with amounts):**
```cypher
MERGE (t:Transaction {txid: tx.txid})
SET t.blockHeight = tx.blockHeight,
    t.isCoinbase = true,
    t.totalInput = 0,
    t.totalOutput = tx.totalOutput,
    t.fee = 0
    // ... other properties
```

> **Note:** Amounts (`totalInput`, `totalOutput`, `fee`) are stored as INTEGER values in satoshis (1 BTC = 100,000,000 satoshis). They are pre-calculated in Rust using the UTXO cache during Phase 3, not computed via graph traversal.

**Phase 4 - Create Input:**
```cypher
MERGE (i:Input {inputId: inp.inputId})
SET i.inputIndex = inp.inputIndex,
    i.scriptSig = inp.scriptSig,
    i.sequence = inp.sequence,
    i.witness = inp.witness
MERGE (t)-[:HAS_INPUT]->(i)
// DO NOT create SPENDS relationship - there is no previous output
// WHERE inp.previousOutputIndex <> 4294967295 filters out coinbase inputs
```

> **Note:** `previousTxid` and `previousOutputIndex` are passed in the batch parameters for the SPENDS relationship lookup but are NOT stored as properties on the Input node. The Input node only stores: `inputId`, `inputIndex`, `scriptSig`, `sequence`, `witness`.

**Critical:** Do NOT attempt to look up previous output or create SPENDS relationship for coinbase inputs. The Cypher query uses `WHERE inp.previousOutputIndex <> 4294967295` to skip coinbase inputs.

**Phase 6 - Simplified Layer:**
- **PERFORMS relationship:** Coinbase has no sender - DO NOT create PERFORMS relationship
- **BENEFITS_TO relationship:** Create normally to miner's address(es), with `outputCount` and `amountReceived` properties

---

## 2. OP_RETURN Outputs (NULL_DATA)

### What is OP_RETURN?

**OP_RETURN** creates provably unspendable outputs used to store arbitrary data on the blockchain.

**Characteristics:**
- ScriptPubKey starts with opcode `0x6a` (OP_RETURN)
- Followed by up to 80 bytes of arbitrary data (consensus limit)
- Has a value (usually 0, but can be non-zero and "burned")
- Cannot be spent (no unlocking script will validate)
- No Bitcoin address associated

**Common uses:**
- Timestamping documents (hash of document)
- Protocol messages (Omni Layer, Counterparty)
- Colored coins metadata
- NFT references

### Detection

```python
def is_op_return(scriptPubKey_bytes):
    return len(scriptPubKey_bytes) > 0 and scriptPubKey_bytes[0] == 0x6a
```

### Ingestion Handling

**Phase 2 - Create Output:**
```cypher
MERGE (o:Output {outputId: out.outputId})
ON CREATE SET
    o.outputIndex = out.outputIndex,
    o.amount = out.amount,  // Usually 0, but store actual value (in satoshis)
    o.scriptPubKey = out.scriptPubKey,
    o.scriptType = 'NULL_DATA',
    o.isSpent = false,
    o.spentInTxid = null,
    o.spentAtHeight = null
MERGE (t)-[:HAS_OUTPUT]->(o)
// DO NOT create LOCKED_TO relationship - there is no address
```

**Critical:** Do NOT attempt to derive address from OP_RETURN scripts. Do NOT create Address node or LOCKED_TO relationship.

**Phase 6 - Simplified Layer:**
- **BENEFITS_TO relationship:** DO NOT create - no address benefited from this output
- OP_RETURN outputs are excluded from the simplified "follow the money" layer

**Note on amounts:**
- If `amount > 0`, this value is effectively burned (destroyed) and unrecoverable
- Include in `totalOutput` calculation (contributes to fee if burned)

---

## 3. Genesis Block (Block 0)

### What is the Genesis Block?

The **first block in the Bitcoin blockchain** (height 0), hardcoded into Bitcoin Core software.

**Unique properties:**
- Block hash: `000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f`
- Contains one transaction (the genesis coinbase)
- Genesis coinbase output is **unspendable** (not included in UTXO set by protocol rule)
- Has no previous block (previousHash is all zeros)

### Genesis Block Handling

**Phase 1 - Create Block:**
```cypher
MERGE (genesis:Block {hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"})
SET genesis.height = 0,
    genesis.previousHash = "0000000000000000000000000000000000000000000000000000000000000000",
    genesis.timestamp = datetime({epochSeconds: 1231006505}),
    // ... other properties
// DO NOT create NEXT_BLOCK relationship from a previous block (there is none)
// The query uses WHERE block.height > 0 to skip genesis
```

**Genesis Coinbase Transaction:**
- Process as normal coinbase transaction
- Create Input node with null previous output (standard coinbase handling)
- Create Output node normally

**Genesis Coinbase Output Unspendability:**

The genesis coinbase output (50 BTC to `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa`) is **unspendable** due to a quirk in Bitcoin Core's implementation. However, for data integrity:

```cypher
MERGE (o:Output {outputId: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b:0"})
ON CREATE SET
    o.outputIndex = 0,
    o.amount = 5000000000,  // 50 BTC in satoshis
    o.scriptType = 'P2PK',
    o.isSpent = false,
    o.spentInTxid = null,
    o.spentAtHeight = null
```

**Why unspendable?** Bitcoin Core doesn't add the genesis coinbase output to its UTXO database. Any transaction attempting to spend it would be rejected. This is a protocol quirk, not a script rule.

---

## 4. P2PK Outputs (Pay to Public Key) - Obsolete

### What is P2PK?

An **obsolete output type** that locks funds directly to a public key (not a hash).

**Characteristics:**
- ScriptPubKey: `<pubkey> OP_CHECKSIG`
- Used in early Bitcoin (2009-2010)
- Genesis block and early mining rewards used P2PK
- Public key can be compressed (33 bytes) or uncompressed (65 bytes)
- Replaced by P2PKH (hashed version) for better security

### Ingestion Handling

**Phase 2 - Address Derivation (during output creation):**
```python
# Detect P2PK pattern via instruction parsing
# Code checks for exactly 2 instructions: PushBytes + OP_CHECKSIG
if script has exactly 2 instructions:
    if instructions == [PushBytes(pubkey), OP_CHECKSIG]:
        pubkey = PublicKey.from_slice(pubkey_bytes)
        address = Address.p2pkh(pubkey, network)
        script_type = 'P2PK'
```

**Important:** P2PK scripts contain the **full public key**, not a hash. To derive an address:
1. Extract the public key from script
2. Compute HASH160 of the public key
3. Encode as a P2PKH address (starts with `1`)

**Example - Genesis block output:**
- ScriptPubKey: `4104678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5fac`
- Public key: `04678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5f`
- Derived address: `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa`

See [ADDRESS_DERIVATION.md](ADDRESS_DERIVATION.md) for full algorithm.

---

## 5. Bare Multisig Outputs

### What is Bare Multisig?

An **unspendable or non-standard** output type for M-of-N multisig not wrapped in P2SH.

**Characteristics:**
- ScriptPubKey: `M <pubkey1> <pubkey2> ... <pubkeyN> N OP_CHECKMULTISIG`
- Example: 2-of-3 multisig requires 2 signatures from 3 public keys
- No single Bitcoin address (multiple pubkeys involved)
- Considered non-standard by most nodes (relaying disabled)
- Rarely seen on mainnet

### Ingestion Handling

**Phase 2 - Address Derivation (during output creation):**
```cypher
// Bare multisig does not match any known ScriptType variant
// Code maps it to UNKNOWN (no Multisig variant in ScriptType enum)
MERGE (o:Output {outputId: out.outputId})
ON CREATE SET
    o.scriptType = 'UNKNOWN',
    // ... other properties
// DO NOT create LOCKED_TO relationship - no single address derivable
```

> **Implementation note:** The `ScriptType` enum in `src/parser/address.rs` has 8 variants: `P2PKH`, `P2SH`, `P2WPKH`, `P2WSH`, `P2TR`, `P2PK`, `NullData`, `Unknown`. There is no `Multisig` variant. Bare multisig scripts fall through to `Unknown` since they don't match any of the checked patterns.

**Recommendation:** Treat as UNKNOWN (which is what the code does). Modern multisig uses P2SH or P2WSH wrappers which have single addresses.

---

## 6. Unknown or Non-Standard Scripts

### What are Non-Standard Scripts?

Any scriptPubKey that doesn't match known patterns:
- Custom scripts
- Future script types (SegWit v2-v16 witness programs)
- Malformed scripts
- Experimental scripts

### Ingestion Handling

```cypher
MERGE (o:Output {outputId: out.outputId})
ON CREATE SET
    o.scriptPubKey = out.scriptPubKey,  // Store raw script for future analysis
    o.scriptType = 'UNKNOWN',
    o.amount = out.amount,  // Satoshis (INTEGER)
    o.isSpent = false
// DO NOT create LOCKED_TO relationship
```

**Phase 6 - Simplified Layer:**
- Skip UNKNOWN outputs in BENEFITS_TO derivation
- Cannot determine recipient address

**Future-proofing:** Store raw scriptPubKey so if future witness versions are defined, you can retroactively derive addresses.

---

## 7. Witness Data (SegWit Transactions)

### What is Witness Data?

**Segregated Witness (SegWit)** moves signature data into a separate witness structure, reducing transaction size and enabling new script types.

**Characteristics:**
- Applies to P2WPKH, P2WSH, P2TR outputs
- Input's `witness` field contains signature and/or script data
- Non-SegWit nodes see witness data as empty (backwards compatible)
- Affects `vsize` (virtual size) and `weight` calculations

### Ingestion Handling

**Phase 4 - Create Input:**
```cypher
MERGE (i:Input {inputId: inp.inputId})
SET i.scriptSig = inp.scriptSig,  // Empty or minimal for SegWit
    i.witness = inp.witness        // Array of hex strings (SegWit data)
```

**Storage:**
- Store `witness` as array of strings (each string is hex-encoded witness item)
- For P2WPKH: witness = `[signature, pubkey]`
- For P2WSH: witness = `[signature1, signature2, ..., witnessScript]`
- For P2TR: witness = `[signature]` or `[signature, ..., witnessScript, controlBlock]`
- For coinbase (post-SegWit): witness contains the witness commitment

**Note:** Neo4j supports array properties, so store directly as string array. The code stores witness for ALL inputs, including coinbase transactions (see `InputData::from_input()` in `src/domain/conversions.rs`).

---

## 8. Transaction Version and Locktime Edge Cases

### Transaction Version

**Versions:**
- Version 1: Original Bitcoin transactions (most common through 2017)
- Version 2: Introduced for BIP-68 (relative locktime) in 2016
- Future versions may add new features

**Handling:** Store version number directly. No special handling needed, but be aware future versions may introduce new semantics.

### Locktime Edge Cases

**Locktime = 0:** Transaction can be included in any block (no restriction)

**Locktime < 500,000,000:** Block height - transaction locked until blockchain reaches this height

**Locktime ≥ 500,000,000:** Unix timestamp - transaction locked until this time

**Handling during ingestion:**
```cypher
MERGE (t:Transaction {txid: tx.txid})
SET t.locktime = tx.locktime
// Only the raw locktime value (u32) is stored.
// The code does NOT derive or store a locktimeType property.
```

**Interpreting locktime values:**
- `0` = no restriction (transaction can be included in any block)
- `< 500,000,000` = block height (locked until chain reaches this height)
- `>= 500,000,000` = Unix timestamp (locked until this time)

**Note:** Locktime is a policy rule. Locked transactions exist in blocks but couldn't have been included until the locktime condition was met.

---

## 9. Replace-By-Fee (RBF) and Sequence Numbers

### Sequence Number

Each input has a `sequence` field (4 bytes).

**Special values:**
- `0xFFFFFFFF` (4294967295): Locktime disabled, RBF disabled (final)
- `< 0xFFFFFFFE`: Indicates RBF enabled or relative locktime

**BIP-125 (Opt-in RBF):**
- If any input has `sequence < 0xFFFFFFFE`, transaction signals RBF
- RBF allows unconfirmed transaction to be replaced with higher fee version

**Handling during ingestion:**
```cypher
MERGE (i:Input {inputId: inp.inputId})
SET i.sequence = inp.sequence
// Only the raw sequence value (u32) is stored.
// The code does NOT derive or store isRBF or isFinal properties.
```

**Interpreting sequence values:**
- `0xFFFFFFFF` (4294967295) = locktime disabled, RBF disabled (final)
- `< 0xFFFFFFFE` = RBF potentially enabled, or relative locktime active

**Note:** Once a transaction is in a block (confirmed), RBF is irrelevant. Only matters for mempool handling.

---

## 10. Duplicate Transactions (Historical Quirk)

### The Duplicate Transaction Bug

**Historical bug:** Bitcoin allowed duplicate transaction IDs before BIP-30 (2012).

**Blocks with duplicate txids:**
- Block 91842: Transaction `d5d27987d2a3dfc724e359870c6644b40e497bdc0589a033220fe15429d88599`
- Block 91880: Same txid as above

**Problem:** Two transactions with same txid but different content!

**BIP-30 Solution (2012):** Prevents duplicate txids. Cannot create transaction with same txid as unspent transaction.

**BIP-34 Solution (2013):** Requires coinbase scriptSig to contain block height, making coinbase txids unique.

### Ingestion Handling

**Strategy 1 - Ignore duplicates (recommended):**
- Use unique constraint: `CREATE CONSTRAINT transaction_unique FOR (t:Transaction) REQUIRE t.txid IS UNIQUE`
- On duplicate, skip or update (depending on your policy)
- Modern blocks (post-2013) have no duplicates

**Strategy 2 - Differentiate by block:**
```cypher
// Store composite key if handling pre-BIP-30 blocks
MERGE (t:Transaction {txid: $txid})
SET t.blockHeight = $blockHeight,
    t.blockHash = $blockHash
// Composite constraint:
// CREATE CONSTRAINT tx_block_unique FOR (t:Transaction) REQUIRE (t.txid, t.blockHeight) IS UNIQUE
```

**Recommendation:** Use Strategy 1 unless ingesting historical blocks 91842-91880.

---

## 11. Empty Blocks

### What are Empty Blocks?

Blocks containing **only the coinbase transaction** (no other transactions).

**Characteristics:**
- `txCount = 1` (only coinbase)
- Miner chose not to include any mempool transactions
- Rare but valid (miner's choice)

### Ingestion Handling

No special handling needed - process coinbase transaction normally. Empty blocks are valid and should be ingested like any other block.

---

## Summary Checklist

- [x] Handle coinbase transactions (no SPENDS relationship for input, `WHERE inp.previousOutputIndex <> 4294967295`)
- [x] Handle OP_RETURN outputs (no address, no LOCKED_TO, `scriptType = 'NULL_DATA'`)
- [x] Handle genesis block (no previous block, unspendable coinbase output)
- [x] Handle P2PK outputs (derive address via instruction parsing + `Address::p2pkh()`)
- [x] Handle bare multisig (mapped to `scriptType = 'UNKNOWN'`, no `Multisig` variant)
- [x] Handle unknown scripts (store raw scriptPubKey, `scriptType = 'UNKNOWN'`)
- [x] Store witness data for ALL inputs including coinbase (as string array)
- [x] Store transaction version and locktime (raw values only, no derived properties)
- [x] Store input sequence numbers (raw value only, no `isRBF`/`isFinal` properties)
- [x] Handle potential duplicate txids via MERGE (idempotent, last-write-wins)
- [x] Handle empty blocks (coinbase only, no special handling needed)

> **Implementation notes:**
> - All Cypher queries use **MERGE** (not CREATE) for idempotent reprocessing
> - All amounts are **INTEGER satoshis** (not FLOAT BTC). 1 BTC = 100,000,000 satoshis
> - Transaction amounts (`totalInput`, `totalOutput`, `fee`) are calculated in **Rust** using the UTXO cache during Phase 3, not via graph traversal
> - Phase 5 (Calculate Amounts) has been **removed** in M7 -- amounts are now part of Phase 3
> - Input nodes store only: `inputId`, `inputIndex`, `scriptSig`, `sequence`, `witness`
> - `previousTxid`/`previousOutputIndex` are used for SPENDS lookup, not stored on Input node

---

## References

- [Bitcoin Developer Reference - Transactions](https://developer.bitcoin.org/reference/transactions.html)
- [BIP-30: Duplicate Transactions](https://github.com/bitcoin/bips/blob/master/bip-0030.mediawiki)
- [BIP-34: Block Height in Coinbase](https://github.com/bitcoin/bips/blob/master/bip-0034.mediawiki)
- [BIP-68: Relative Locktime](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki)
- [BIP-125: Replace-By-Fee](https://github.com/bitcoin/bips/blob/master/bip-0125.mediawiki)
- [BIP-141: Segregated Witness](https://github.com/bitcoin/bips/blob/master/bip-0141.mediawiki)
