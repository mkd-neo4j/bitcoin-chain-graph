---
feature: utxo-cache-shutdown-height
status: shipped
created: 2026-02-12
---

# Ideation: UTXO Cache Shutdown Save Uses Wrong Height

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
- **Summary**: SIGTERM during catchup causes the shutdown UTXO cache save to use a stale height (`current_height - 1`) instead of the actual last committed block height. On restart, the cache is missing UTXOs from the gap, causing crashes.
- **Open questions**: Validate line numbers and code paths against the current codebase. Confirm the fix approach (separate `last_committed_height` variable).

### 2026-02-12 — Code review complete
- **What we did**: Validated all three `save_to_file` call sites, traced `current_height` semantics, confirmed the race condition in both RPC catchup (lines 835→844) and ZMQ real-time (lines 964→1043) paths. Findings saved to `code-review.md`.
- **Decisions made**: The `last_committed_height: Option<u32>` approach is sound. Set it after successful `ingest_blocks_batch` in both paths, use it for shutdown save, skip save if `None`.
- **Open questions**: Reorg + shutdown interaction (cache may be ahead of rollback point) — separate issue, out of scope for this fix.
