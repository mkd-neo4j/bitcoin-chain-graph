---
feature: neo4j-writer-transactions
status: complete
created: 2026-02-12
---

# Ideation: Neo4jWriter Transaction Methods

This folder is a workspace for exploring and shaping a feature before it becomes a formal feature doc.

## How to Use

Add files here as you think through the feature. Common artifacts:

- **Code reviews** — Analysis of existing code that this feature will touch
- **Research notes** — How other projects solve this, API docs, trade-offs
- **Design sketches** — Data flow diagrams, component trees, schema changes
- **Spike results** — Quick experiments to validate an approach
- **Conversation logs** — Key decisions and reasoning captured from Claude sessions

There are no rules about file names or formats. Use whatever helps you think.

## When You're Ready

When the feature is clear enough to write testable acceptance criteria:

1. Say "create the feature" in your current session, or source `feature-docs/new-feature.md`
2. Claude will read everything in this folder and draft a feature doc
3. Review and refine the draft
4. The final doc is saved to `feature-docs/ready/<feature-name>.md`
5. This README's status is updated to `complete`
6. Kick off the test-writer: `@test-writer Pick up feature-docs/ready/<feature-name>.md`

This folder stays as an archive of the thinking that led to the feature doc.

## Progress

### 2026-02-12 — Initial exploration
- **Summary**: Neo4jWriter panics on startup because `begin_transaction`, `commit_transaction`, and `rollback_transaction` are `todo!()` stubs. PR #7 added explicit transaction wrapping in the ingestion loop but only implemented MockWriter. The core challenge is neo4rs `Txn` ownership semantics (`commit(self)` consumes) vs GraphWriter's `&self` interface.
- **Open questions**:
  - Full implementation vs no-op unblock?
  - How to route write methods through active transaction vs auto-commit?
  - Retry semantics inside transactions (retry individual query vs rollback entire batch)?
  - Mutex guard lifetime across batched sub-chunk writes?
  - `tokio::sync::Mutex` vs `std::sync::Mutex` for async Txn?
  - Does WriterError need a new TransactionFailed variant?

### 2026-02-12 — Codebase analysis complete
- **What we did**: Four parallel agents reviewed Neo4jWriter (struct, run_with_retry, execute_batched, all write methods), GraphWriter trait + MockWriter transaction impl, ingestion loop transaction usage, and neo4rs Txn API ownership semantics.
- **Decisions made**:
  - Use `tokio::sync::Mutex<Option<Txn>>` (not std::sync — guard must span await points)
  - Add `TransactionFailed` variant to WriterError (non-retryable)
  - Disable per-query retries inside transactions (fail-fast, let caller rollback batch)
  - Phase 6 parallelism serializes under txn lock — acceptable since Neo4j single-connection anyway
  - Full implementation (Approach A) preferred over no-op unblock
- **Open questions**:
  - Exact refactor of `run_with_retry` vs new `run_in_txn` method?
  - Should `execute_batched` hold the txn lock for entire batch or per-sub-chunk?
- **Artifacts**: code-review-neo4j-writer.md, code-review-trait-mock.md, code-review-ingestion.md, research-neo4rs-txn.md, design-synthesis.md

### 2026-02-12 — API design complete
- **What we did**: Designed full data flow, struct changes, dual-path execution, Phase 6 serialization analysis, and all method signatures
- **Decisions made**:
  - Single `execute_query` approach rejected — Query not Clone, can't pass through retry closure
  - Instead: modify `execute_batched` with `is_some()` check + add `run_query_single` for non-batched methods
  - Guard against double-begin (error, not silent overwrite like MockWriter)
  - No client-side timeout in transactional mode (rely on server-side `dbms.transaction.timeout`)
  - Phase 6 parallelism serializes under txn — acceptable, single connection anyway
  - `update_checkpoint` and `mark_output_spent` also need transaction routing via `run_query_single`
- **Open questions**: None — ready to create feature doc
- **Artifacts**: api-design.md

### 2026-02-12 — Feature doc created
- **What we did**: Distilled all ideation artifacts into `feature-docs/ready/neo4j-writer-transactions.md`
- **Status**: Ideation complete. Feature doc ready for test-writer.
