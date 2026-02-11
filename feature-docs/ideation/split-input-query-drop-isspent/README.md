---
feature: split-input-query-drop-isspent
status: complete
created: 2026-02-11
---

# Ideation: Split Input Query + Drop isSpent

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

### 2026-02-11 — Initial exploration
- **Summary**: Reviewing staging doc `docs/staging/01-split-input-query-and-precompute-outputid.md` which proposes dropping redundant isSpent/spentInTxid/spentAtHeight properties from Output nodes, splitting the monolithic input query into two, and pre-computing previousOutputId in Rust.
- **Open questions**: Need to validate that the staging doc's code snippets match the actual codebase (line numbers, query content, function signatures).

### 2026-02-11 — Code review validation
- **What we did**: Read all 4 affected source files (queries.rs, conversions.rs, neo4j/mod.rs, schema.rs) plus traits.rs and mock.rs. Compared every "Before" snippet and line number in the staging doc against actual source.
- **Decisions made**: All staging doc claims are accurate — line numbers, query content, function signatures all match exactly. `mark_output_spent` confirmed dead code (never called from pipeline).
- **Open questions**:
  1. Should `blockHeight` be cleaned out of `input_to_bolt_map` since it's no longer consumed by any Cypher query?
  2. Should existing data migration (removing isSpent from ~43M Output nodes) be in-scope or explicitly out-of-scope?
  3. Should `DROP INDEX output_spent IF EXISTS` be added to schema init for the live database?
  4. Should tests account for partial failure when `write_inputs_fast` becomes two separate queries?

### 2026-02-11 — Revised approach: no query split
- **What we did**: Analysed orphan risk from splitting into two `execute_batched` calls. Traced crash recovery path (`sync_checkpoint_with_db` only checks tx count, not input integrity). Concluded that splitting introduces unacceptable orphan risk.
- **Decisions made**:
  1. **Do NOT split the query.** Keep a single UNWIND query — just remove the `SET o.isSpent` and use pre-computed `previousOutputId`. The bottleneck was the SET (write locks on Output), not the MATCH.
  2. Remove `blockHeight` from `input_to_bolt_map` — no longer consumed by any Cypher query.
  3. Existing data migration is out of scope (starting fresh).
  4. Remove `output_spent` index from code; manual DB cleanup.
  5. `mark_output_spent` dead code stays — not worth 3-file churn.
- **Open questions**: None — ready to create feature doc.

### 2026-02-11 — Feature doc created
- **What we did**: Distilled ideation into feature doc at `feature-docs/ready/drop-isspent-precompute-outputid.md`
- **Status**: Complete. Ideation archived.
