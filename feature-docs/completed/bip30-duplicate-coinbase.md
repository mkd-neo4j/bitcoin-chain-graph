---
title: BIP30 Duplicate Coinbase Handling
status: completed
priority: high
ideation-ref: feature-docs/ideation/bip30-duplicate-coinbase/
affected-files:
  - src/domain/ingestion.rs
  - src/writer/error.rs
  - src/writer/neo4j/mod.rs
---

# BIP30 Duplicate Coinbase Handling

## Summary

The Bitcoin blockchain contains exactly 2 known duplicate coinbase transactions (BIP30). Blocks 91,842 and 91,880 reuse coinbase txids from blocks 91,812 and 91,722 respectively. The fast ingestion path uses CREATE queries with UNWIND, which fail on the unique constraint for `outputId`, `txid`, and `inputId`. This causes a crash loop at block ~91,800 because the entire UNWIND batch is rejected (not just the duplicate row). The fix detects BIP30 heights in each batch chunk and falls back to the existing MERGE-based write methods for that chunk only, plus adds a `ConstraintViolation` error variant so deterministic failures are not retried.

## Acceptance Criteria

1. GIVEN a batch chunk containing block 91,842 WHEN `process_batch_chunk` runs THEN it calls `write_outputs()`, `write_transactions()`, and `write_inputs()` (MERGE variants) instead of `write_outputs_fast()`, `write_transactions_fast()`, and `write_inputs_fast()` (CREATE variants) for that chunk
2. GIVEN a batch chunk containing block 91,880 WHEN `process_batch_chunk` runs THEN it uses MERGE variants for outputs, transactions, and inputs for that chunk
3. GIVEN a batch chunk containing only blocks outside `BIP30_DUPLICATE_HEIGHTS` WHEN `process_batch_chunk` runs THEN it uses the fast CREATE variants as before (no behavior change)
4. GIVEN a neo4rs error with code `"Neo.ClientError.Schema.ConstraintValidationFailed"` WHEN `run_with_retry` handles the error THEN it returns `WriterError::ConstraintViolation` immediately without retrying
5. GIVEN a neo4rs error with code `"Neo.ClientError.Schema.ConstraintValidationFailed"` WHEN `WriterError::ConstraintViolation` is constructed THEN `is_retryable()` returns `false`
6. GIVEN a transient neo4rs error (e.g. connection reset) WHEN `run_with_retry` handles the error THEN it still retries as before (existing behavior preserved)
7. GIVEN block 91,842 with duplicate coinbase txid `e3bf3d07d4b0375638d5f1db5255fe07ba2c4cb067cd81b84ee974b6585fb468` WHEN the MERGE write path executes THEN all non-duplicate transactions in that block are written successfully and the duplicate coinbase outputs/inputs are idempotently merged
8. GIVEN the UTXO cache WHEN processing a BIP30 duplicate block THEN the cache `insert` overwrites the existing entry (HashMap semantics) and subsequent phases (transaction amounts, input resolution) work correctly

## Edge Cases

- Batch chunk contains BOTH a BIP30 block and normal blocks — the entire chunk uses MERGE, which is safe because MERGE is idempotent for new AND existing data. Performance cost is negligible (one chunk out of thousands).
- `BIP30_DUPLICATE_HEIGHTS` currently contains `[91842, 91880]` (the duplicate blocks). These are the blocks that DUPLICATE earlier coinbases. Blocks 91,722 and 91,812 (the originals) ingest normally with CREATE since they are first-seen.
- Constraint violation occurs on a non-BIP30 block (unexpected schema issue) — `WriterError::ConstraintViolation` propagates up and aborts ingestion, same as any other non-retryable error. The fix does NOT silently swallow constraint violations.

## Out of Scope

- Do NOT change the Cypher queries in `src/writer/neo4j/queries.rs` — the fast CREATE queries are correct for 99.9999% of blocks. The fix is in the Rust orchestration layer, not the query layer.
- Do NOT add MERGE variants of `write_has_output_relationships` or `write_performs`/`write_benefits_to` to the BIP30 path — HAS_OUTPUT relationships don't have unique constraints, and PERFORMS/BENEFITS_TO already use MERGE. Only outputs, transactions, and inputs need the fallback.
- Do NOT implement `begin_transaction()`/`commit_transaction()` (currently `todo!()` stubs) — that is a separate feature and would change the atomicity model for all batches, not just BIP30.

## Technical Notes

- The existing `BIP30_DUPLICATE_HEIGHTS` constant at `src/domain/ingestion.rs:52` already contains `[91842, 91880]` and is used for logging. Reuse it for the detection check.
- The check is `chunk.iter().any(|(h, _, _)| BIP30_DUPLICATE_HEIGHTS.contains(h))` — O(n) on a 2-element array, negligible cost.
- The MERGE write methods (`write_outputs`, `write_transactions`, `write_inputs`) are the default implementations of the `_fast` trait methods (see `src/writer/traits.rs:354-383`), so they are already tested and production-ready.
- For the `ConstraintViolation` detection in `run_with_retry`, match on `neo4rs::Error::Neo4j(neo4j_err)` and check `neo4j_err.code() == "Neo.ClientError.Schema.ConstraintValidationFailed"` BEFORE wrapping in `WriterError`. This preserves the structured error info that is currently lost by `format!("{}", e)`.
- The `write_outputs_fast` call at `ingestion.rs:710` runs concurrently with cache population via `tokio::join!`. When falling back to MERGE, the same `tokio::join!` pattern should be preserved — just swap `write_outputs_fast` for `write_outputs` in the future.
- **Rejected**: Changing `CREATE` to `MERGE` in the fast query — MERGE is heavier than CREATE for every single block. This is a 2-block edge case in all of Bitcoin history; it does not justify slowing down the hot path.
- **Rejected**: Try-catch (catch constraint violation and continue) — UNWIND is all-or-nothing. The entire batch fails, not just the duplicate row. Catching the error means losing all outputs/transactions/inputs for every block in that chunk.
- **Rejected**: Pre-filtering duplicate outputs from the UNWIND data — the UTXO cache is LRU and may have evicted the original entry, making this unreliable.
