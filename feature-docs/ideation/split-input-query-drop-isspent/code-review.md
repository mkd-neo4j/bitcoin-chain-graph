# Code Review: Split Input Query + Drop isSpent

Validated the staging doc (`docs/staging/01-split-input-query-and-precompute-outputid.md`) against the actual codebase. Source references below.

## 1. queries.rs — Verified

### CREATE_OUTPUTS_QUERY (line 96)
**Staging doc: correct.** Lines 104-106 set `isSpent = false, spentInTxid = null, spentAtHeight = null` on ON CREATE. Matches the "Before" snippet exactly.

### CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY (line 516)
**Staging doc: correct.** Lines 524-526 include `isSpent: false, spentInTxid: null, spentAtHeight: null` in the CREATE block. Matches exactly.

### CREATE_INPUTS_QUERY (line 156, MERGE variant)
**Staging doc: correct.** Lines 168-172 do the Cypher string concat `inp.previousTxid + ':' + toString(inp.previousOutputIndex)` and SET `o.isSpent = true, o.spentInTxid = inp.txid, o.spentAtHeight = inp.blockHeight`.

### CREATE_INPUTS_FAST_QUERY (line 541, CREATE variant)
**Staging doc: correct.** Lines 553-557 identical pattern — Cypher string concat for outputId, SET isSpent/spentInTxid/spentAtHeight on Output.

### ROLLBACK_REVERT_SPENT_QUERY (line 381)
**Staging doc: correct.** Line 383 does `SET o.isSpent = false, o.spentInTxid = null, o.spentAtHeight = null`. This becomes unnecessary when isSpent properties are removed.

### MARK_OUTPUT_SPENT_QUERY (line 264)
**Staging doc: correct.** Dead code — only referenced by `mark_output_spent()` method which is never called in the pipeline (grep confirmed: no calls from ingestion.rs or any orchestrator code, only trait definition + impls).

## 2. conversions.rs — Verified

### input_to_bolt_map (line 134)
**Staging doc: correct.** Current function does NOT include `previousOutputId`. The staging doc proposes adding it as a pre-computed field:
```rust
map.put("previousOutputId".into(),
    format!("{}:{}", input.previous_txid, input.previous_output_index).as_str().into());
```
This replaces the Cypher-side `inp.previousTxid + ':' + toString(inp.previousOutputIndex)`.

**Line number accuracy**: Staging doc says "line 134" — actual is line 134. Correct.

## 3. neo4j/mod.rs — Verified

### write_inputs_fast (line 319)
**Staging doc: correct.** Currently a single `execute_batched` call using `CREATE_INPUTS_FAST_QUERY`. The staging doc proposes splitting into two calls: one for `CREATE_INPUTS_NODES_FAST_QUERY`, one for `CREATE_INPUTS_SPENDS_FAST_QUERY`.

### write_inputs (line 450, GraphWriter impl)
**Staging doc: correct.** Currently a single `execute_batched` call using `CREATE_INPUTS_QUERY`. Same split proposed.

### rollback_block (line 753)
**Staging doc: correct.** Step 1 (lines 756-765) calls `ROLLBACK_REVERT_SPENT_QUERY`. Staging doc proposes removing this step entirely and renumbering 2→1, 3→2, 4→3, 5→4.

### mark_output_spent (line 586)
**Staging doc: correct.** Dead code. Method exists in trait + Neo4jWriter + MockWriter. Never called from pipeline. Staging doc recommends leaving it as dead code to avoid 3-file churn.

## 4. schema.rs — Verified

### output_spent index (lines 71-72)
**Staging doc: correct.** The index `CREATE INDEX output_spent IF NOT EXISTS FOR (o:Output) ON (o.isSpent)` exists at line 71-72. Staging doc proposes removing it. After removal, the `indexes` vec drops from 7 to 6 entries.

## 5. traits.rs — No Changes Needed (Verified)

The `write_inputs_fast` default impl (line 413) delegates to `write_inputs()`. Trait signatures are unchanged — both methods take `&[InputData]` and return `Result<()>`. The split happens entirely inside the Neo4jWriter impl, not at the trait level.

The `mark_output_spent` method (line 226) stays as dead code per staging doc.

## 6. mock.rs — No Changes Needed (Verified)

MockWriter's `write_inputs` (line 172) and `mark_output_spent` (line 233) are unchanged. MockWriter doesn't use fast queries — it delegates via the default trait impl.

---

## Discrepancies Found

### Minor: Line number drift
- Staging doc says `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` is "~line 516" → actual is exactly line 516. Correct.
- Staging doc says `ROLLBACK_REVERT_SPENT_QUERY` is "~line 381" → actual is exactly line 381. Correct.
- Staging doc says `MARK_OUTPUT_SPENT_QUERY` is "~line 264" → actual is exactly line 264. Correct.
- Staging doc says `write_inputs_fast` is "line 319" → actual is exactly line 319. Correct.
- Staging doc says `write_inputs` is "line 450" → actual is exactly line 450. Correct.
- Staging doc says `mark_output_spent` is "line 586" → actual is exactly line 586. Correct.
- Staging doc says `rollback_block` is "~line 753" → actual is exactly line 753. Correct.

**All line numbers are accurate.**

### No discrepancies found in query content.

All "Before" code snippets in the staging doc match the actual source exactly.

---

## Open Questions

1. **MockWriter impact**: The staging doc says MockWriter needs no changes, and that's correct for the trait signatures. But should the MockWriter's `write_inputs` change behavior for testing purposes? Currently it just pushes to `self.inputs`. Since we're splitting into two queries on the Neo4j side only, the MockWriter stays as-is. The test-writer should be aware that `write_inputs_fast` now involves two separate database calls — tests validating error handling may want to consider partial failure (first query succeeds, second fails).

2. **`blockHeight` in input_to_bolt_map**: The current `input_to_bolt_map` includes `blockHeight` (line 155). The staging doc's proposed version also includes it. However, with isSpent removal, `blockHeight` is no longer consumed by the Cypher queries (it was only used for `SET o.spentAtHeight = inp.blockHeight`). It's harmless to keep passing it (Cypher ignores unused params in UNWIND maps), but it's technically dead data. Should we clean it up or leave it?

3. **Existing data migration**: The staging doc mentions the ~43M existing Output nodes still have isSpent/spentInTxid/spentAtHeight properties. The `apoc.periodic.iterate` cleanup is optional. Should the feature doc include this as an acceptance criterion, or explicitly out-of-scope it?

4. **Index drop on existing database**: The feature removes the index creation from `schema.rs`, but the existing `output_spent` index remains on the live database. Should we add a `DROP INDEX output_spent IF EXISTS` to the schema init, or handle it manually?
