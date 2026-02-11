---
title: Neo4j Atomic Batch Transactions
status: completed
priority: high
affected-files:
  - src/writer/traits.rs
  - src/writer/neo4j/mod.rs
  - src/writer/mock.rs
  - src/domain/ingestion.rs
---

# Neo4j Atomic Batch Transactions

## Summary

Wrap each batch chunk's 7 ingestion phases in a single Neo4j explicit transaction so that a crash mid-batch leaves zero orphaned data. Currently every `execute_batched()` and `graph.run()` call is auto-committed, so a crash between phases leaves orphaned nodes (e.g., output nodes with no parent transaction). With explicit transactions, incomplete batches are never committed — eliminating the non-atomic checkpoint DELETE+CREATE and the incomplete crash recovery checks as separate problems.

## Acceptance Criteria

1. GIVEN a batch of blocks WHEN all 7 phases complete successfully THEN all data is committed atomically in a single Neo4j transaction
2. GIVEN a batch of blocks WHEN any phase (2–7) fails mid-batch THEN zero nodes/relationships from that batch are persisted in Neo4j
3. GIVEN a batch of blocks WHEN Phase 3 (transactions) fails after Phase 2 (outputs) succeeds THEN no output nodes from that batch exist in the database
4. GIVEN a successful batch commit THEN the checkpoint is written inside the same transaction (atomic with the data)
5. GIVEN a crash after checkpoint DELETE but before CREATE THEN on resume, the checkpoint still exists (because both happen in one transaction)

## Edge Cases

- Neo4j transaction timeout on very large batches (blocks with thousands of transactions) — transaction should fail cleanly and be retryable with the same batch
- Neo4j connection drops mid-transaction — uncommitted transaction is automatically rolled back by the server, next retry starts fresh
- Deadlock during parallel Phase 6 writes within a transaction — retry logic should handle this within the transaction boundary
- Transaction memory pressure on Neo4j heap for large batch sizes — fail with a clear error; operator can reduce batch chunk size in config

## Out of Scope

- Do NOT refactor `run_live_ingestion` in `src/main.rs` — separate cleanup task, touching it risks breaking ZMQ/RPC logic
- Do NOT fix Phase 6 parallel deadlock detection — existing retry logic already handles this; transaction boundaries don't change the behavior
- Do NOT add UTXO cache rollback on failure in `src/domain/ingestion.rs` — cache entries are overwritten on re-ingestion, and transactional writes make partial state less likely
- Do NOT change batch size configuration or auto-scaling — separate performance tuning concern

## Technical Notes

- `neo4rs` 0.8 supports explicit transactions via `graph.start_txn()` returning a `Txn` that has `.run()` and `.commit()`. All existing `graph.run()` calls within a batch need to route through `txn.run()` instead.
- The `GraphWriter` trait methods currently take `&self` — transaction state will need to be threaded through the batch call chain, likely by passing a transaction handle into each phase method or adding `begin_txn`/`commit_txn`/`rollback_txn` to the trait.
- **Rejected**: Making each individual phase its own transaction — doesn't solve the core problem since a crash between phase transactions still leaves orphaned data.
- **Rejected**: Application-level WAL (write-ahead log) — adds complexity when Neo4j already provides ACID transactions. Use the database's own guarantees.
- Follow existing pattern: all Cypher queries remain as constants in `writer/neo4j/queries.rs`, just executed via `txn.run()` instead of `graph.run()`.
