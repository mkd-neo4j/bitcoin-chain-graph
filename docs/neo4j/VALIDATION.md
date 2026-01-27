# Data Validation for Bitcoin Blockchain Ingestion

Validation rules and Cypher queries to ensure data integrity during and after ingestion.

---

## Overview

Bitcoin blockchain data has inherent properties that must be preserved during ingestion. Validation queries help detect:
- Ingestion bugs
- Data corruption
- Incomplete processing
- Relationship integrity issues

Run these validation queries:
1. **During ingestion** - After each block or batch for early error detection
2. **Post-ingestion** - After completing full ingestion for comprehensive verification

---

## 1. Transaction Balance Validation

### Rule: totalInput = totalOutput + fee (Non-Coinbase)

For all non-coinbase transactions, the sum of input amounts must equal output amounts plus fee.

```cypher
// Find transactions with incorrect balance
MATCH (t:Transaction {isCoinbase: false})
WHERE t.totalInput <> (t.totalOutput + t.fee)
RETURN t.txid, t.blockHeight, t.totalInput, t.totalOutput, t.fee,
       t.totalInput - t.totalOutput - t.fee AS discrepancy
LIMIT 100
```

**Expected:** 0 results

**If violations found:**
- Check Phase 3 amount calculation (amounts are calculated in Rust via UTXO cache — see [CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md))
- Verify input SPENDS relationships point to correct previous outputs
- Verify output amounts are correctly extracted from raw data

---

### Rule: Coinbase totalInput = 0, fee = 0

```cypher
// Find coinbase transactions with non-zero inputs or fees
MATCH (t:Transaction {isCoinbase: true})
WHERE t.totalInput <> 0 OR t.fee <> 0
RETURN t.txid, t.blockHeight, t.totalInput, t.fee
LIMIT 100
```

**Expected:** 0 results

---

## 2. Input/Output Relationship Integrity

### Rule: All Inputs Have Corresponding Transaction

```cypher
// Find orphaned inputs (not linked to transaction)
MATCH (i:Input)
WHERE NOT (:Transaction)-[:HAS_INPUT]->(i)
RETURN i.inputId
LIMIT 100
```

**Expected:** 0 results

---

### Rule: All Outputs Have Corresponding Transaction

```cypher
// Find orphaned outputs (not linked to transaction)
MATCH (o:Output)
WHERE NOT (:Transaction)-[:HAS_OUTPUT]->(o)
RETURN o.outputId
LIMIT 100
```

**Expected:** 0 results

---

### Rule: All Non-Coinbase Inputs Have SPENDS Relationship

```cypher
// Find non-coinbase inputs missing SPENDS relationship
MATCH (t:Transaction {isCoinbase: false})-[:HAS_INPUT]->(i:Input)
WHERE NOT (i)-[:SPENDS]->(:Output)
RETURN i.inputId, t.txid
LIMIT 100
```

**Expected:** 0 results

**If violations found:**
- Previous output may not have been created yet (out-of-order processing)
- Check Phase 4 logic for creating SPENDS relationships
- Verify blocks were processed in height order

---

### Rule: Coinbase Inputs Have NO SPENDS Relationship

```cypher
// Find coinbase inputs with SPENDS relationship (invalid)
MATCH (t:Transaction {isCoinbase: true})-[:HAS_INPUT]->(i:Input)
WHERE (i)-[:SPENDS]->(:Output)
RETURN i.inputId, t.txid
LIMIT 100
```

**Expected:** 0 results

---

## 3. Address Relationship Integrity

### Rule: All Non-NULL_DATA Outputs with Parseable Addresses Have LOCKED_TO

```cypher
// Find outputs missing LOCKED_TO relationship (excluding NULL_DATA and UNKNOWN)
MATCH (o:Output)
WHERE o.scriptType NOT IN ['NULL_DATA', 'UNKNOWN']
  AND NOT (o)-[:LOCKED_TO]->(:Address)
RETURN o.outputId, o.scriptType
LIMIT 100
```

**Expected:** 0 results (or very few for edge cases)

**If violations found:**
- Address derivation may have failed for a valid script type
- Check [ADDRESS_DERIVATION.md](../bitcoin/ADDRESS_DERIVATION.md) logic
- Manually inspect scriptPubKey to determine if address should be derivable

---

### Rule: NULL_DATA Outputs Have NO LOCKED_TO Relationship

```cypher
// Find OP_RETURN outputs with LOCKED_TO relationship (invalid)
MATCH (o:Output {scriptType: 'NULL_DATA'})-[:LOCKED_TO]->(a:Address)
RETURN o.outputId, a.address
LIMIT 100
```

**Expected:** 0 results

---

## 4. Spent Output Consistency

### Rule: Outputs Marked as Spent Have SPENDS Relationship Pointing to Them

```cypher
// Find outputs marked spent but no input spends them
MATCH (o:Output {isSpent: true})
WHERE NOT (:Input)-[:SPENDS]->(o)
RETURN o.outputId, o.spentInTxid, o.spentAtHeight
LIMIT 100
```

**Expected:** 0 results

---

### Rule: Outputs with SPENDS Relationship Are Marked as Spent

```cypher
// Find outputs with SPENDS relationship but not marked spent
MATCH (i:Input)-[:SPENDS]->(o:Output)
WHERE o.isSpent = false
RETURN o.outputId, i.inputId
LIMIT 100
```

**Expected:** 0 results

---

### Rule: spentInTxid Matches Actual Spending Transaction

```cypher
// Verify spentInTxid property matches actual spending transaction
MATCH (i:Input)-[:SPENDS]->(o:Output)
MATCH (t:Transaction)-[:HAS_INPUT]->(i)
WHERE o.spentInTxid <> t.txid
RETURN o.outputId, o.spentInTxid AS recorded, t.txid AS actual
LIMIT 100
```

**Expected:** 0 results

---

## 5. Block Chain Integrity

### Rule: All Blocks Except Genesis Have NEXT_BLOCK Relationship

```cypher
// Find blocks missing NEXT_BLOCK (excluding genesis and tip)
MATCH (b:Block)
WHERE b.height > 0  // Not genesis
  AND NOT (:Block)-[:NEXT_BLOCK]->(b)
WITH max(b.height) AS maxHeight
MATCH (b:Block)
WHERE b.height > 0
  AND b.height < maxHeight  // Not current tip
  AND NOT (:Block)-[:NEXT_BLOCK]->(b)
RETURN b.height, b.hash
LIMIT 100
```

**Expected:** 0 results (or only the highest block if chain is incomplete)

---

### Rule: Block Heights Are Sequential

```cypher
// Find gaps in block heights
MATCH (b:Block)
WITH collect(DISTINCT b.height) AS heights
UNWIND range(0, size(heights)-2) AS i
WITH heights[i] AS currentHeight, heights[i+1] AS nextHeight
WHERE nextHeight <> currentHeight + 1
RETURN currentHeight, nextHeight, nextHeight - currentHeight - 1 AS gap
```

**Expected:** 0 results

---

### Rule: previousHash Matches Actual Previous Block Hash

```cypher
// Verify previousHash property matches actual previous block
MATCH (prev:Block)-[:NEXT_BLOCK]->(current:Block)
WHERE current.previousHash <> prev.hash
RETURN current.height, current.previousHash AS recorded, prev.hash AS actual
LIMIT 100
```

**Expected:** 0 results

---

## 6. Transaction Count Consistency

### Rule: Block txCount Matches Actual Transaction Count

```cypher
// Find blocks where txCount doesn't match actual transactions
MATCH (b:Block)
OPTIONAL MATCH (b)<-[:INCLUDED_IN]-(t:Transaction)
WITH b, count(t) AS actualCount
WHERE b.txCount <> actualCount
RETURN b.height, b.txCount AS recorded, actualCount AS actual
LIMIT 100
```

**Expected:** 0 results

---

### Rule: Every Block Has At Least One Transaction (Coinbase)

```cypher
// Find blocks with no transactions
MATCH (b:Block)
WHERE NOT (b)<-[:INCLUDED_IN]-(:Transaction)
RETURN b.height, b.hash
LIMIT 100
```

**Expected:** 0 results

---

### Rule: Every Block's First Transaction Is Coinbase

```cypher
// Find blocks where first transaction is not coinbase
MATCH (b:Block)<-[:INCLUDED_IN]-(t:Transaction)
WITH b, t
ORDER BY t.txid  // Assuming transactions are ordered; may need adjustment
WITH b, collect(t)[0] AS firstTx
WHERE firstTx.isCoinbase = false
RETURN b.height, firstTx.txid
LIMIT 100
```

**Note:** This query assumes transactions have a consistent ordering property. If your ingestion doesn't maintain transaction order, you'll need to add an `index` property during ingestion.

---

## 7. Simplified Layer Consistency

### Rule: PERFORMS Relationships Only for Non-Coinbase Transactions

```cypher
// Find coinbase transactions with PERFORMS relationship
MATCH (addr:Address)-[:PERFORMS]->(t:Transaction {isCoinbase: true})
RETURN addr.address, t.txid
LIMIT 100
```

**Expected:** 0 results

---

### Rule: PERFORMS Relationships Match Actual Input Addresses

```cypher
// Verify PERFORMS relationships point to correct addresses
MATCH (addr:Address)-[:PERFORMS]->(t:Transaction {isCoinbase: false})
WHERE NOT (t)-[:HAS_INPUT]->(:Input)-[:SPENDS]->(:Output)-[:LOCKED_TO]->(addr)
RETURN addr.address, t.txid
LIMIT 100
```

**Expected:** 0 results

**Note:** This query can be slow on large graphs. Run on sample subset for performance.

---

### Rule: BENEFITS_TO Relationships Match Actual Output Addresses

```cypher
// Verify BENEFITS_TO relationships point to correct addresses
MATCH (t:Transaction)-[:BENEFITS_TO]->(addr:Address)
WHERE NOT (t)-[:HAS_OUTPUT]->(:Output)-[:LOCKED_TO]->(addr)
RETURN t.txid, addr.address
LIMIT 100
```

**Expected:** 0 results

---

### Rule: PERFORMS Relationships Have Required Properties

```cypher
// Find PERFORMS relationships missing inputCount or amountSpent
MATCH (addr:Address)-[r:PERFORMS]->(t:Transaction)
WHERE r.inputCount IS NULL OR r.amountSpent IS NULL
RETURN addr.address, t.txid, r.inputCount, r.amountSpent
LIMIT 100
```

**Expected:** 0 results

---

### Rule: BENEFITS_TO Relationships Have Required Properties

```cypher
// Find BENEFITS_TO relationships missing outputCount or amountReceived
MATCH (t:Transaction)-[r:BENEFITS_TO]->(addr:Address)
WHERE r.outputCount IS NULL OR r.amountReceived IS NULL
RETURN t.txid, addr.address, r.outputCount, r.amountReceived
LIMIT 100
```

**Expected:** 0 results

---

### Rule: PERFORMS amountSpent Matches Sum of Input Amounts

```cypher
// Verify PERFORMS amountSpent matches the actual sum of input amounts from the detailed layer
MATCH (addr:Address)-[r:PERFORMS]->(t:Transaction)
MATCH (t)-[:HAS_INPUT]->(i:Input)-[:SPENDS]->(o:Output)-[:LOCKED_TO]->(addr)
WITH addr, t, r, sum(o.amount) AS actualAmount, count(i) AS actualCount
WHERE r.amountSpent <> actualAmount OR r.inputCount <> actualCount
RETURN addr.address, t.txid, r.amountSpent AS recorded, actualAmount AS actual
LIMIT 100
```

**Expected:** 0 results

**Note:** This query can be slow on large graphs. Run on a sample subset for performance.

---

### Rule: BENEFITS_TO amountReceived Matches Sum of Output Amounts

```cypher
// Verify BENEFITS_TO amountReceived matches the actual sum of output amounts
MATCH (t:Transaction)-[r:BENEFITS_TO]->(addr:Address)
MATCH (t)-[:HAS_OUTPUT]->(o:Output)-[:LOCKED_TO]->(addr)
WITH t, addr, r, sum(o.amount) AS actualAmount, count(o) AS actualCount
WHERE r.amountReceived <> actualAmount OR r.outputCount <> actualCount
RETURN t.txid, addr.address, r.amountReceived AS recorded, actualAmount AS actual
LIMIT 100
```

**Expected:** 0 results

**Note:** This query can be slow on large graphs. Run on a sample subset for performance.

---

## 8. Data Completeness Checks

### Rule: All Transactions Have Complete Amount Data (After Phase 3)

```cypher
// Find transactions missing calculated amounts
MATCH (t:Transaction)
WHERE t.totalInput IS NULL
   OR t.totalOutput IS NULL
   OR t.fee IS NULL
RETURN t.txid, t.blockHeight, t.isCoinbase
LIMIT 100
```

**Expected:** 0 results (after Phase 3 is complete — amounts are calculated in Rust during transaction ingestion)

---

### Rule: All Outputs Have Required Properties

```cypher
// Find outputs with missing required properties
MATCH (o:Output)
WHERE o.outputId IS NULL
   OR o.amount IS NULL
   OR o.scriptType IS NULL
   OR o.isSpent IS NULL
RETURN o
LIMIT 100
```

**Expected:** 0 results

---

### Rule: All Inputs Have Required Properties

```cypher
// Find inputs with missing required properties
MATCH (i:Input)
WHERE i.inputId IS NULL
   OR i.inputIndex IS NULL
RETURN i
LIMIT 100
```

**Expected:** 0 results

**Note:** `previousTxid` and `previousOutputIndex` are NOT stored as Input node properties. They are used during ingestion to create the SPENDS relationship, then discarded. See [DATA_MODEL.md](DATA_MODEL.md) for details.

---

## 9. UTXO Set Validation

### Rule: No Output Can Be Spent Multiple Times

```cypher
// Find outputs spent by multiple inputs (double-spend detection)
MATCH (o:Output)<-[:SPENDS]-(i:Input)
WITH o, count(i) AS spendCount
WHERE spendCount > 1
RETURN o.outputId, spendCount, o.spentInTxid
LIMIT 100
```

**Expected:** 0 results

---

### Rule: Genesis Coinbase Output Is Unspent

```cypher
// Check if genesis coinbase output is marked unspent (should be unspendable by protocol)
MATCH (genesis:Block {height: 0})<-[:INCLUDED_IN]-(t:Transaction {isCoinbase: true})
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
RETURN o.outputId, o.isSpent, o.spentInTxid
```

**Expected:** `isSpent = false`, `spentInTxid = null` (genesis coinbase is unspendable)

---

### Rule: Calculate Current UTXO Set Size

```cypher
// Count unspent outputs (current UTXO set)
MATCH (o:Output {isSpent: false})
RETURN count(o) AS utxoCount, sum(o.amount) AS totalSatoshis
```

**Expected:** Should match known UTXO set size for the block height ingested. Note: amounts are in satoshis (1 BTC = 100,000,000 satoshis).

---

## 10. Anomaly Detection

### Find Transactions with Unusually High Fees

```cypher
// Find potential fee calculation errors or anomalies
MATCH (t:Transaction {isCoinbase: false})
WHERE t.fee > 100000000  // Fees over 1 BTC (100,000,000 satoshis) are suspicious
RETURN t.txid, t.blockHeight, t.fee, t.totalInput, t.totalOutput
ORDER BY t.fee DESC
LIMIT 100
```

**Expected:** Very few or zero results (1 BTC fee is extremely rare)

---

### Find Transactions with Negative Fees

```cypher
// Negative fees are impossible (except coinbase by convention)
MATCH (t:Transaction {isCoinbase: false})
WHERE t.fee < 0
RETURN t.txid, t.blockHeight, t.fee
LIMIT 100
```

**Expected:** 0 results

---

## 11. Performance & Indexing Validation

### Check Constraint Existence

```cypher
SHOW CONSTRAINTS
```

**Expected:** Should list all constraints defined in [CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md):
- `block_height_unique`
- `block_hash_unique`
- `transaction_unique`
- `output_unique`
- `input_unique`
- `address_unique`

---

### Check Index Existence

```cypher
SHOW INDEXES
```

**Expected:** Should list all 7 indexes defined in [CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md):
- `transaction_timestamp` — Transaction (timestamp)
- `transaction_block` — Transaction (blockHeight)
- `transaction_coinbase` — Transaction (isCoinbase)
- `output_spent` — Output (isSpent)
- `output_amount` — Output (amount)
- `output_script_type` — Output (scriptType)
- `block_timestamp` — Block (timestamp)

---

## 12. Summary Validation Report

### Generate Complete Validation Report

```cypher
// Block count
CALL {
  MATCH (b:Block)
  RETURN count(b) AS blockCount
}

// Transaction count
CALL {
  MATCH (t:Transaction)
  RETURN count(t) AS txCount
}

// Output count
CALL {
  MATCH (o:Output)
  RETURN count(o) AS outputCount
}

// Input count
CALL {
  MATCH (i:Input)
  RETURN count(i) AS inputCount
}

// Address count
CALL {
  MATCH (a:Address)
  RETURN count(a) AS addressCount
}

// UTXO count (amounts in satoshis)
CALL {
  MATCH (o:Output {isSpent: false})
  RETURN count(o) AS utxoCount, sum(o.amount) AS totalSatoshis
}

// Failed balance checks
CALL {
  MATCH (t:Transaction {isCoinbase: false})
  WHERE t.totalInput <> (t.totalOutput + t.fee)
  RETURN count(t) AS balanceErrors
}

// Orphaned inputs
CALL {
  MATCH (i:Input)
  WHERE NOT (:Transaction)-[:HAS_INPUT]->(i)
  RETURN count(i) AS orphanedInputs
}

// Orphaned outputs
CALL {
  MATCH (o:Output)
  WHERE NOT (:Transaction)-[:HAS_OUTPUT]->(o)
  RETURN count(o) AS orphanedOutputs
}

// Missing SPENDS
CALL {
  MATCH (t:Transaction {isCoinbase: false})-[:HAS_INPUT]->(i:Input)
  WHERE NOT (i)-[:SPENDS]->(:Output)
  RETURN count(i) AS missingSpends
}

RETURN
  blockCount,
  txCount,
  outputCount,
  inputCount,
  addressCount,
  utxoCount,
  totalSatoshis,
  balanceErrors,
  orphanedInputs,
  orphanedOutputs,
  missingSpends
```

**Expected output example:**
```
blockCount: 100000
txCount: 523000
outputCount: 1200000
inputCount: 680000
addressCount: 450000
utxoCount: 520000
totalSatoshis: 1950000000000000
balanceErrors: 0
orphanedInputs: 0
orphanedOutputs: 0
missingSpends: 0
```

---

## 13. Sample Data Spot Checks

### Validate Genesis Block

```cypher
MATCH (genesis:Block {height: 0})
MATCH (genesis)<-[:INCLUDED_IN]-(t:Transaction)
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
MATCH (o)-[:LOCKED_TO]->(addr:Address)
RETURN
  genesis.hash AS blockHash,
  t.txid AS genesisTxid,
  o.amount AS genesisAmount,
  addr.address AS genesisAddress
```

**Expected:**
- `blockHash`: `000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f`
- `genesisTxid`: `4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b`
- `genesisAmount`: `5000000000` (50 BTC in satoshis)
- `genesisAddress`: `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa`

---

### Validate Known Transaction

Pick a well-known transaction (e.g., first Bitcoin pizza transaction) and verify all details match blockchain explorers.

```cypher
MATCH (t:Transaction {txid: $knownTxid})
MATCH (t)-[:HAS_INPUT]->(i:Input)-[:SPENDS]->(prevOut:Output)
MATCH (t)-[:HAS_OUTPUT]->(o:Output)
RETURN t, collect(DISTINCT i) AS inputs, collect(DISTINCT o) AS outputs
```

Compare with blockchain explorer data.

---

## Validation Checklist

Run these queries after ingestion:

- [ ] Transaction balance validation (non-coinbase)
- [ ] Coinbase transaction validation
- [ ] Input relationship integrity
- [ ] Output relationship integrity
- [ ] Address relationship integrity (LOCKED_TO)
- [ ] Spent output consistency
- [ ] Block chain integrity (NEXT_BLOCK, sequential heights)
- [ ] Transaction count consistency
- [ ] Simplified layer consistency (PERFORMS, BENEFITS_TO)
- [ ] Data completeness (no NULL values in required fields)
- [ ] UTXO set validation (no double-spends)
- [ ] Genesis block validation
- [ ] Anomaly detection (high fees, negative fees)
- [ ] Constraint existence
- [ ] Index existence
- [ ] Summary validation report
- [ ] Sample data spot checks (genesis block, known transactions)

---

## Troubleshooting Validation Failures

### If balance validation fails:
1. Check Phase 3 logic — amounts (`totalInput`, `totalOutput`, `fee`) are calculated in Rust using the UTXO cache during transaction ingestion
2. Verify SPENDS relationships point to correct previous outputs
3. Ensure input amounts are looked up correctly from previous outputs via the UTXO cache (with Neo4j fallback)

### If relationship integrity fails:
1. Verify ingestion phases ran in correct order (1→2→3→4→6→7)
2. Check for race conditions if processing in parallel
3. Ensure Neo4j transaction boundaries are correct

### If UTXO consistency fails:
1. Verify outputs are correctly marked as spent during Phase 4
2. Check for duplicate SPENDS relationships
3. Ensure blocks were processed in sequential height order

### If simplified layer fails:
1. PERFORMS and BENEFITS_TO relationships are created with pre-aggregated data from Rust (Phase 6) — not derived from graph traversals
2. Verify the Rust aggregation logic in `ingestion.rs` correctly groups inputs/outputs by address
3. Verify all outputs have LOCKED_TO relationships (needed for correct address aggregation)

---

## References

- [DATA_MODEL.md](DATA_MODEL.md) - Schema definition
- [INGESTION_ARCHITECTURE.md](../architecture/INGESTION_ARCHITECTURE.md) - Processing phases
- [CYPHER_EXAMPLES.md](CYPHER_EXAMPLES.md) - Query patterns
- [Bitcoin Developer Reference](https://developer.bitcoin.org/reference/transactions.html)
