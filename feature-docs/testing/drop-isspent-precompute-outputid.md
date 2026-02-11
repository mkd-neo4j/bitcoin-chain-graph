---
title: Drop isSpent Properties and Pre-compute outputId
status: testing
priority: high
ideation-ref: feature-docs/ideation/split-input-query-drop-isspent/
affected-files:
  - src/writer/neo4j/queries.rs
  - src/writer/neo4j/conversions.rs
  - src/writer/neo4j/mod.rs
  - src/writer/neo4j/schema.rs
---

# Drop isSpent Properties and Pre-compute outputId

## Summary

Remove the redundant `isSpent`, `spentInTxid`, and `spentAtHeight` properties from Output nodes. These are fully derivable from the SPENDS relationship and their SET during input ingestion is the main performance bottleneck (3-27s per 5000-record batch due to write locks on Output nodes). Removing the SET eliminates all writes to Output nodes during Phase 4, reducing batch times to the ~200-700ms range of the other operations. Also pre-compute `previousOutputId` in Rust (currently concatenated in Cypher) and remove the now-unnecessary `ROLLBACK_REVERT_SPENT` rollback step. Remove `blockHeight` from the input bolt map since no Cypher query consumes it after this change.

## Acceptance Criteria

1. GIVEN the `CREATE_INPUTS_FAST_QUERY` constant WHEN inspected THEN it creates Input nodes, HAS_INPUT relationships, and SPENDS relationships in a single UNWIND query with no SET on Output nodes, and uses `inp.previousOutputId` instead of Cypher string concatenation

2. GIVEN the `CREATE_INPUTS_QUERY` constant (MERGE recovery variant) WHEN inspected THEN it follows the same pattern as the fast query — single UNWIND, no SET on Output, uses `inp.previousOutputId`

3. GIVEN the `CREATE_OUTPUTS_QUERY` constant WHEN inspected THEN its ON CREATE SET clause does not include `isSpent`, `spentInTxid`, or `spentAtHeight`

4. GIVEN the `CREATE_OUTPUTS_WITH_LOCKED_TO_FAST_QUERY` constant WHEN inspected THEN its CREATE block does not include `isSpent`, `spentInTxid`, or `spentAtHeight`

5. GIVEN the `input_to_bolt_map` function WHEN converting an InputData THEN the resulting BoltMap includes a `previousOutputId` field with value `"{previous_txid}:{previous_output_index}"` and does NOT include a `blockHeight` field

6. GIVEN the `rollback_block` method on Neo4jWriter WHEN called THEN it does NOT execute `ROLLBACK_REVERT_SPENT_QUERY` (the step is removed entirely since DETACH DELETE on Input nodes removes SPENDS relationships automatically)

7. GIVEN the `create_indexes` function in schema.rs WHEN called THEN it does NOT create an `output_spent` index

8. GIVEN the `write_inputs_fast` method on Neo4jWriter WHEN called THEN it executes exactly ONE `execute_batched` call (not two), preserving per-batch atomicity so that Input nodes cannot exist without their SPENDS relationships

9. GIVEN a batch of inputs containing both regular and coinbase inputs (previousOutputIndex = 4294967295) WHEN written via `write_inputs_fast` THEN coinbase inputs have no SPENDS relationship and regular inputs each have exactly one SPENDS relationship to the correct Output

## Edge Cases

- Coinbase inputs (previousOutputIndex = 0xFFFFFFFF) — filtered by WHERE clause, no SPENDS created, no previousOutputId lookup attempted
- Crash mid-batch during forward ingestion — per-batch atomicity (single UNWIND per `graph.run()` auto-commit) means partial UNWIND either fully commits or fully rolls back; recovery via `sync_checkpoint_with_db` + `rollback_block` + re-CREATE is safe
- Recovery/reprocessing mode — MERGE-based query follows same single-query structure, no orphan risk

## Out of Scope

- Removing existing `isSpent`/`spentInTxid`/`spentAtHeight` data from previously-ingested Output nodes (ingestion will start fresh)
- Dropping the `output_spent` index on the live database (manual cleanup)
- Removing `mark_output_spent` dead code from GraphWriter trait, Neo4jWriter, and MockWriter (harmless, avoids 3-file churn)
- Enhancing `check_block_complete` to verify input/output integrity beyond transaction count (pre-existing limitation, separate feature)

## Technical Notes

- The performance bottleneck was `SET o.isSpent = true, o.spentInTxid = ..., o.spentAtHeight = ...` on Output nodes during input ingestion, causing write locks. The MATCH on Output is an O(1) index lookup with zero write locks — no need to split the query.
- The staging doc (`docs/staging/01-split-input-query-and-precompute-outputid.md`) proposed splitting into two queries. This was rejected during ideation because it introduces orphan risk (Input nodes without SPENDS) if the process dies between the two `execute_batched` calls, and `sync_checkpoint_with_db` only checks transaction count so wouldn't detect or rollback the inconsistency.
- `ROLLBACK_REVERT_SPENT_QUERY` can be removed entirely. When `rollback_block` does DETACH DELETE on Input nodes, the SPENDS relationships are removed automatically, making the referenced Outputs "unspent" again (derivable from absence of SPENDS).
- The `ROLLBACK_REVERT_SPENT_QUERY` constant should be deleted. The `MARK_OUTPUT_SPENT_QUERY` constant should be left as dead code (it's referenced by the `mark_output_spent` trait method that requires changes across 3 files to remove).
- Follow the existing pattern in `write_inputs_fast`: single `execute_batched` call with `inputs_to_bolt_list` converter.
- Rollback step renumbering in `rollback_block`: remove Step 1 (revert spent), renumber Steps 2-5 to 1-4.
