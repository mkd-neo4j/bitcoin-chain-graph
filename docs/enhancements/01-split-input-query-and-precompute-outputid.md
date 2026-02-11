# Task 1: Drop isSpent Properties + Split Input Query + Pre-compute outputId

## Problem

The `CREATE_INPUTS_FAST_QUERY` in `src/writer/neo4j/queries.rs` is the main bottleneck.
It does 3 expensive operations per input in a single UNWIND:

1. CREATE Input node + SET properties
2. MATCH Transaction + CREATE HAS_INPUT relationship
3. MATCH Output + CREATE SPENDS + **SET isSpent/spentInTxid/spentAtHeight on Output**

Steps 1-2 are fast (~200ms per 5000 batch). Step 3 takes 3-27 seconds because it
writes 3 properties to each Output node, causing write locks.

## Key Insight

`isSpent`, `spentInTxid`, and `spentAtHeight` are **fully redundant**:

- **`isSpent`** = derivable from `EXISTS((o)<-[:SPENDS]-())`
- **`spentInTxid`** = derivable from `(o)<-[:SPENDS]-(i)<-[:HAS_INPUT]-(t:Transaction)` → `t.txid`
- **`spentAtHeight`** = derivable from same chain → `t.blockHeight`

The SPENDS relationship already encodes all spent information. Rollback queries follow
SPENDS relationships (not these properties) to revert spent status. These properties are
pure denormalized query-convenience metadata.

**Decision: Drop all 3 properties entirely.** This eliminates ALL writes to Output nodes
during input ingestion.

## Solution

### A. Drop isSpent/spentInTxid/spentAtHeight from Output nodes

Remove these properties from:
- Output creation queries (they currently initialize to false/null)
- Input creation queries (they currently SET to true/txid/height)
- Rollback queries (Step 1 becomes unnecessary)

### B. Split the input query into 2 queries

**Query 1** - Create Input nodes + HAS_INPUT (fast, pure creates):
```cypher
UNWIND $inputs AS inp
CREATE (i:Input {inputId: inp.inputId})
SET i.inputIndex = inp.inputIndex,
    i.scriptSig = inp.scriptSig,
    i.sequence = inp.sequence,
    i.witness = inp.witness
WITH i, inp
MATCH (t:Transaction {txid: inp.txid})
CREATE (t)-[:HAS_INPUT]->(i)
```

**Query 2** - Create SPENDS relationships only (no writes to Output nodes):
```cypher
UNWIND $inputs AS inp
WITH inp WHERE inp.previousOutputIndex <> 4294967295
MATCH (i:Input {inputId: inp.inputId})
MATCH (o:Output {outputId: inp.previousOutputId})
CREATE (i)-[:SPENDS]->(o)
```

Note: The two MATCHes in Query 2 are unavoidable - in Cypher you must have references
to both nodes to create a relationship. But they're O(1) indexed lookups (unique
constraints on Input.inputId and Output.outputId). The expensive part (SET on Output
nodes) is completely gone.

### C. Pre-compute previousOutputId in Rust

Pass `previousOutputId` as a pre-computed string parameter instead of doing
`inp.previousTxid + ':' + toString(inp.previousOutputIndex)` in Cypher.

## Files to Modify

### 1. `src/writer/neo4j/queries.rs`

**Add new split fast queries:**

```rust
/// Fast CREATE for Input nodes + HAS_INPUT only (no SPENDS)
pub const CREATE_INPUTS_NODES_FAST_QUERY: &str = r#"
    UNWIND $inputs AS inp
    CREATE (i:Input {inputId: inp.inputId})
    SET i.inputIndex = inp.inputIndex,
        i.scriptSig = inp.scriptSig,
        i.sequence = inp.sequence,
        i.witness = inp.witness
    WITH i, inp
    MATCH (t:Transaction {txid: inp.txid})
    CREATE (t)-[:HAS_INPUT]->(i)
"#;

/// Fast CREATE for SPENDS relationships only (no property updates on Output)
/// Uses pre-computed previousOutputId from Rust (no Cypher string concat)
pub const CREATE_INPUTS_SPENDS_FAST_QUERY: &str = r#"
    UNWIND $inputs AS inp
    WITH inp WHERE inp.previousOutputIndex <> 4294967295
    MATCH (i:Input {inputId: inp.inputId})
    MATCH (o:Output {outputId: inp.previousOutputId})
    CREATE (i)-[:SPENDS]->(o)
"#;
```

**Add MERGE equivalents for recovery mode:**

```rust
/// MERGE-based Input nodes + HAS_INPUT (for reprocessing)
pub const CREATE_INPUTS_NODES_QUERY: &str = r#"
    UNWIND $inputs AS inp
    MERGE (i:Input {inputId: inp.inputId})
    SET i.inputIndex = inp.inputIndex,
        i.scriptSig = inp.scriptSig,
        i.sequence = inp.sequence,
        i.witness = inp.witness
    WITH i, inp
    MATCH (t:Transaction {txid: inp.txid})
    MERGE (t)-[:HAS_INPUT]->(i)
"#;

/// MERGE-based SPENDS relationships (for reprocessing)
pub const CREATE_INPUTS_SPENDS_QUERY: &str = r#"
    UNWIND $inputs AS inp
    WITH inp WHERE inp.previousOutputIndex <> 4294967295
    MATCH (i:Input {inputId: inp.inputId})
    MATCH (o:Output {outputId: inp.previousOutputId})
    MERGE (i)-[:SPENDS]->(o)
"#;
```

**Update `CREATE_OUTPUTS_QUERY`** (~line 96) - remove isSpent initialization:

Before:
```rust
    ON CREATE SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType,
        o.isSpent = false,
        o.spentInTxid = null,
        o.spentAtHeight = null
    ON MATCH SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType
```

After:
```rust
    ON CREATE SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType
    ON MATCH SET
        o.outputIndex = out.outputIndex,
        o.amount = out.amount,
        o.scriptPubKey = out.scriptPubKey,
        o.scriptType = out.scriptType
```

**Update `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY`** (~line 516) - remove from CREATE block:

Before:
```rust
    CREATE (o:Output {
        outputId: out.outputId,
        outputIndex: out.outputIndex,
        amount: out.amount,
        scriptPubKey: out.scriptPubKey,
        scriptType: out.scriptType,
        isSpent: false,
        spentInTxid: null,
        spentAtHeight: null
    })
```

After:
```rust
    CREATE (o:Output {
        outputId: out.outputId,
        outputIndex: out.outputIndex,
        amount: out.amount,
        scriptPubKey: out.scriptPubKey,
        scriptType: out.scriptType
    })
```

**Remove `ROLLBACK_REVERT_SPENT_QUERY`** (~line 381):

This query is now unnecessary. When rollback Step 2 does `DETACH DELETE` on Input nodes,
the SPENDS relationships are removed automatically, and the Output is "unspent" again.
Replace with a comment explaining why it was removed:

```rust
// ROLLBACK Step 1 (REVERT_SPENT) has been removed.
// The isSpent/spentInTxid/spentAtHeight properties no longer exist on Output nodes.
// Spent status is derived from the SPENDS relationship, which is removed when
// Input nodes are DETACH DELETEd in Step 2.
```

**Leave `MARK_OUTPUT_SPENT_QUERY`** (~line 264) **as dead code**:

Never called in the pipeline (confirmed by grep). However, it is referenced by the
`mark_output_spent` method in `neo4j/mod.rs` (line 586), which implements the
`GraphWriter` trait (`traits.rs`) and `MockWriter` (`mock.rs`). Removing the query
constant would require removing the method from all three files. Since it's harmless
dead code and removing it touches 3 additional files, leave it for now. It can be
cleaned up in a follow-up if desired.

**Comment out or remove the old `CREATE_INPUTS_QUERY` and `CREATE_INPUTS_FAST_QUERY`.**

### 2. `src/writer/neo4j/conversions.rs`

In `input_to_bolt_map()` (line 134), add the pre-computed `previousOutputId`:

```rust
pub fn input_to_bolt_map(input: &InputData) -> BoltMap {
    let mut map = BoltMap::new();
    map.put("inputId".into(), input.input_id.as_str().into());
    map.put("inputIndex".into(), (input.input_index as i64).into());
    map.put("txid".into(), input.txid.as_str().into());
    map.put("previousTxid".into(), input.previous_txid.as_str().into());
    map.put(
        "previousOutputIndex".into(),
        (input.previous_output_index as i64).into(),
    );
    // Pre-computed outputId for SPENDS query (avoids Cypher string concat)
    map.put(
        "previousOutputId".into(),
        format!("{}:{}", input.previous_txid, input.previous_output_index).as_str().into(),
    );
    map.put("scriptSig".into(), input.script_sig.as_str().into());
    map.put("sequence".into(), (input.sequence as i64).into());

    let witness_list: Vec<BoltType> = input
        .witness
        .iter()
        .map(|w| BoltType::String(w.as_str().into()))
        .collect();
    map.put("witness".into(), BoltType::List(witness_list.into()));

    map.put("blockHeight".into(), (input.block_height as i64).into());
    map
}
```

### 3. `src/writer/neo4j/mod.rs`

**Replace `write_inputs_fast`** (line 319):

```rust
pub async fn write_inputs_fast(&self, inputs: &[InputData]) -> Result<()> {
    // Step 1: Create Input nodes + HAS_INPUT relationships (fast, pure creates)
    self.execute_batched(
        inputs,
        queries::CREATE_INPUTS_NODES_FAST_QUERY,
        "inputs",
        "write_inputs_fast:nodes",
        inputs_to_bolt_list,
    )
    .await?;

    // Step 2: Create SPENDS relationships (no property writes on Output nodes)
    self.execute_batched(
        inputs,
        queries::CREATE_INPUTS_SPENDS_FAST_QUERY,
        "inputs",
        "write_inputs_fast:spends",
        inputs_to_bolt_list,
    )
    .await
}
```

**Replace `write_inputs`** in the `GraphWriter` impl block (line 450):

```rust
async fn write_inputs(&self, inputs: &[InputData]) -> Result<()> {
    // Step 1: Create/merge Input nodes + HAS_INPUT
    self.execute_batched(
        inputs,
        queries::CREATE_INPUTS_NODES_QUERY,
        "inputs",
        "write_inputs:nodes",
        inputs_to_bolt_list,
    )
    .await?;

    // Step 2: Create/merge SPENDS relationships
    self.execute_batched(
        inputs,
        queries::CREATE_INPUTS_SPENDS_QUERY,
        "inputs",
        "write_inputs:spends",
        inputs_to_bolt_list,
    )
    .await
}
```

See section 5 below for `rollback_block` changes (removing Step 1).

### 4. `src/writer/neo4j/schema.rs`

Remove the `output_spent` index from the `create_indexes()` function (line 71-72):

```rust
// REMOVE this line — indexes Output.isSpent which no longer exists:
"CREATE INDEX output_spent IF NOT EXISTS
 FOR (o:Output) ON (o.isSpent)",
```

After removing, the `indexes` vec will have 6 entries instead of 7.

### 5. `src/writer/neo4j/mod.rs` — Update `rollback_block`

In `rollback_block()` (~line 753), remove the call to `ROLLBACK_REVERT_SPENT_QUERY`
(Step 1, lines 756-765). Renumber remaining steps (2→1, 3→2, 4→3, 5→4) in comments.
The SPENDS relationships are already removed when Input nodes are `DETACH DELETE`d in
the new Step 1 (formerly Step 2), so Output nodes automatically become "unspent".

### 6. No changes needed to:

- `src/writer/traits.rs` - trait signatures unchanged (including `mark_output_spent`,
  which is dead code but removing it would require changes to traits.rs, mock.rs,
  and neo4j/mod.rs — not worth the churn)
- `src/domain/ingestion.rs` - orchestrator unchanged (still calls `write_inputs_fast`)
- `src/domain/models.rs` - domain models unchanged

## Why This Helps

1. **Zero writes to Output nodes during input ingestion** - the entire bottleneck eliminated
2. **Query 1** only touches Input and Transaction nodes (fast creates + 1 indexed lookup)
3. **Query 2** only creates SPENDS relationships with read-only lookups (no write locks)
4. **Simpler rollback** - one fewer step (no spent status to revert)
5. **Less storage** - 3 fewer properties on ~43M Output nodes

## Existing Data Migration (Optional)

The ~43M existing Output nodes still have isSpent/spentInTxid/spentAtHeight properties.
These can be cleaned up later with a one-time Cypher query if desired:

```cypher
// Run in batches to avoid OOM:
CALL apoc.periodic.iterate(
  'MATCH (o:Output) WHERE o.isSpent IS NOT NULL RETURN o',
  'REMOVE o.isSpent, o.spentInTxid, o.spentAtHeight',
  {batchSize: 10000}
)
```

Or just leave them - they won't cause any issues, they're just unused properties.
Also drop the index: `DROP INDEX output_spent IF EXISTS`

## Testing

After making changes:
```bash
cd /data/bitcoin-chain-graph
cargo build --release
# Then on the server:
systemctl restart bitcoin-chain-graph
journalctl -u bitcoin-chain-graph -f
```

Watch for:
- `write_inputs_fast:nodes` and `write_inputs_fast:spends` log lines (split confirmed)
- Significantly faster batch times (no SET on Output = no write locks)
- No errors from missing properties