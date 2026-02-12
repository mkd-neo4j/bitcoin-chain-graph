---
feature: snapshot-resilience
status: complete
created: 2026-02-12
---

# Ideation: Snapshot Resilience

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
- **Summary**: UTXO cache snapshot stays stale (block 13,999 vs Neo4j checkpoint 91,799) when app crashes repeatedly. Periodic saves only trigger at 2,000-block boundaries and clean-exit saves never run during crashes. Crash-restart loops never complete enough blocks to hit the next save interval.
- **Open questions**:
  - Should we save more frequently (every batch)?
  - Should we detect stale snapshots and rebuild from DB?
  - What's the current snapshot save/load code path?
  - How does the resume logic interact with snapshot loading?
  - Are there atomicity concerns with snapshot writes (partial writes on crash)?

### 2026-02-12 — Feature doc created
- **What we did**: Deep research into snapshot save/load/resume code paths, designed fix to save after every committed batch chunk inside `ingest_blocks_batch`, identified dead code to remove
- **Decisions made**: Save after commit (not before), add `cache_snapshot_path` to orchestrator via setter (not constructor), remove `utxo_cache_snapshot_interval` config, keep completion/shutdown saves as safety nets
- **Result**: Feature doc saved to `feature-docs/ready/snapshot-resilience.md`
