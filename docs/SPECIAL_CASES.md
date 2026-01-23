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
- No `witness` data (even in SegWit blocks)
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

**Phase 2 - Create Transaction:**
```cypher
CREATE (t:Transaction {
  txid: $txid,
  blockHeight: $blockHeight,
  isCoinbase: true,
  // ... other properties
})
```

**Phase 3 - Create Outputs:**
- Process outputs normally - coinbase outputs are spendable like any other UTXO
- Derive addresses normally (usually P2PK in early blocks, P2PKH or P2WPKH in modern blocks)

**Phase 4 - Create Input:**
```cypher
CREATE (i:Input {
  inputId: $txid + ':0',
  inputIndex: 0,
  previousTxid: "0000000000000000000000000000000000000000000000000000000000000000",
  previousOutputIndex: 4294967295,
  scriptSig: $coinbaseScriptSig,
  sequence: $sequence
})
CREATE (i)-[:HAS_INPUT]->(t)
// DO NOT create SPENDS relationship - there is no previous output
```

**Critical:** Do NOT attempt to look up previous output or create SPENDS relationship for coinbase inputs.

**Phase 5 - Calculate Amounts:**
```cypher
// For coinbase transactions:
SET t.totalInput = 0  // No inputs spent
SET t.totalOutput = $sumOfOutputs
SET t.fee = 0  // No fee (or negative fee conceptually, since new bitcoin created)
```

**Phase 6 - Simplified Layer:**
- **PERFORMS relationship:** Coinbase has no sender - DO NOT create PERFORMS relationship
- **BENEFITS_TO relationship:** Create normally to miner's address(es)

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

**Phase 3 - Create Output:**
```cypher
CREATE (o:Output {
  outputId: $txid + ':' + $outputIndex,
  outputIndex: $outputIndex,
  amount: $amount,  // Usually 0, but store actual value
  scriptPubKey: $scriptPubKey,
  scriptType: 'NULL_DATA',
  isSpent: false,  // Will never be spent
  spentInTxid: null,
  spentAtHeight: null
})
CREATE (t)-[:HAS_OUTPUT]->(o)
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
CREATE (genesis:Block {
  height: 0,
  hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f",
  previousHash: "0000000000000000000000000000000000000000000000000000000000000000",
  timestamp: datetime('2009-01-03T18:15:05Z'),
  // ... other properties
})
// DO NOT create NEXT_BLOCK relationship from a previous block (there is none)
```

**Genesis Coinbase Transaction:**
- Process as normal coinbase transaction
- Create Input node with null previous output (standard coinbase handling)
- Create Output node normally

**Genesis Coinbase Output Unspendability:**

The genesis coinbase output (50 BTC to `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa`) is **unspendable** due to a quirk in Bitcoin Core's implementation. However, for data integrity:

```cypher
// Mark genesis output with special flag (optional)
CREATE (o:Output {
  outputId: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b:0",
  outputIndex: 0,
  amount: 50.0,
  scriptType: 'P2PK',
  isSpent: false,
  spentInTxid: null,
  spentAtHeight: null,
  // Optional: genesisOutput: true
})
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

**Phase 3 - Address Derivation:**
```python
# Detect P2PK pattern
if script matches "<pubkey> OP_CHECKSIG":
    pubkey = extract_pubkey_from_script(script)
    pubkey_hash = RIPEMD160(SHA256(pubkey))  # HASH160
    address = base58_encode(version_byte + pubkey_hash + checksum)
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

**Phase 3 - Address Derivation:**
```cypher
// Detect bare multisig
if script matches "<m> <pubkeys> <n> OP_CHECKMULTISIG":
    // No single address can be derived
    CREATE (o:Output {
      scriptType: 'MULTISIG',  // or 'UNKNOWN'
      // ... other properties
    })
    // DO NOT create LOCKED_TO relationship
```

**Options:**
1. **Mark as UNKNOWN:** Set `scriptType = 'UNKNOWN'`, no address
2. **Mark as MULTISIG:** Set `scriptType = 'MULTISIG'`, no address
3. **Advanced:** Extract all pubkeys, create multiple relationships to multiple addresses (complex)

**Recommendation:** Treat as UNKNOWN. Modern multisig uses P2SH or P2WSH wrappers which have single addresses.

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
CREATE (o:Output {
  outputId: $outputId,
  scriptPubKey: $scriptPubKey,  // Store raw script for future analysis
  scriptType: 'UNKNOWN',
  amount: $amount,
  isSpent: false
})
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
CREATE (i:Input {
  inputId: $inputId,
  scriptSig: $scriptSig,  // Empty or minimal for SegWit
  witness: $witnessArray  // Array of hex strings (SegWit data)
})
```

**Storage:**
- Store `witness` as array of strings (each string is hex-encoded witness item)
- For P2WPKH: witness = `[signature, pubkey]`
- For P2WSH: witness = `[signature1, signature2, ..., witnessScript]`
- For P2TR: witness = `[signature]` or `[signature, ..., witnessScript, controlBlock]`

**Note:** Neo4j supports array properties, so store directly as string array.

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
CREATE (t:Transaction {
  locktime: $locktime,
  // Optional: derive locktimeType
  locktimeType: CASE
    WHEN $locktime = 0 THEN 'none'
    WHEN $locktime < 500000000 THEN 'block_height'
    ELSE 'timestamp'
  END
})
```

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
CREATE (i:Input {
  sequence: $sequence,
  // Optional flags:
  isRBF: $sequence < 0xFFFFFFFE,
  isFinal: $sequence = 0xFFFFFFFF
})
```

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
CREATE (t:Transaction {
  txid: $txid,
  blockHeight: $blockHeight,  // Include in uniqueness
  blockHash: $blockHash
})
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

- [ ] Handle coinbase transactions (no SPENDS relationship for input)
- [ ] Handle OP_RETURN outputs (no address, no LOCKED_TO)
- [ ] Handle genesis block (no previous block, unspendable coinbase output)
- [ ] Handle P2PK outputs (derive address from public key hash)
- [ ] Handle bare multisig (mark as UNKNOWN or MULTISIG)
- [ ] Handle unknown scripts (store raw scriptPubKey)
- [ ] Store witness data for SegWit inputs (as array)
- [ ] Store transaction version and locktime
- [ ] Store input sequence numbers
- [ ] Handle potential duplicate txids in pre-2013 blocks (if ingesting historical data)
- [ ] Handle empty blocks (coinbase only)

---

## References

- [Bitcoin Developer Reference - Transactions](https://developer.bitcoin.org/reference/transactions.html)
- [BIP-30: Duplicate Transactions](https://github.com/bitcoin/bips/blob/master/bip-0030.mediawiki)
- [BIP-34: Block Height in Coinbase](https://github.com/bitcoin/bips/blob/master/bip-0034.mediawiki)
- [BIP-68: Relative Locktime](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki)
- [BIP-125: Replace-By-Fee](https://github.com/bitcoin/bips/blob/master/bip-0125.mediawiki)
- [BIP-141: Segregated Witness](https://github.com/bitcoin/bips/blob/master/bip-0141.mediawiki)
