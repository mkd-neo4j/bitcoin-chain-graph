---
feature: bip30-duplicate-coinbase
status: complete
created: 2026-02-12
---

# Ideation: BIP30 Duplicate Coinbase Handling

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
- **Summary**: BIP30 defines 2 duplicate coinbase txids in Bitcoin history. The fast output CREATE query hits a unique constraint violation at block 91,880. Rather than switching to MERGE (heavy for a one-time edge case), the plan is to catch the constraint violation error in Rust code for these specific blocks and skip/continue, since the existing data is already valid.
- **Key constraint**: The user explicitly does NOT want MERGE queries. The fix should be a try-catch style error handler scoped to these known blocks.
- **Open questions**: All resolved — see analysis files and feature doc.

### 2026-02-12 — Team validation complete
- **What we did**: Two-agent team analysed rollback logic, query paths, and neo4rs error types
- **Decisions made**: Fall back to existing MERGE queries for BIP30 chunks (not try-catch). Add ConstraintViolation error variant. UTXO cache is fine (HashMap overwrite).
- **Feature doc**: `feature-docs/ready/bip30-duplicate-coinbase.md`
