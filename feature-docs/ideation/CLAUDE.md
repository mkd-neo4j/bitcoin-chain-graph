# Feature Ideation

This directory contains feature ideation folders — workspaces where ideas are explored, validated, and shaped before entering the agent teams pipeline.

## Structure

Each subfolder is one feature idea:

```
ideation/
  CLAUDE.md              This file
  <feature-name>/
    README.md            Status tracking + progress log
    code-review.md       Analysis of existing code to change
    api-design.md        API contracts, data flow
    research.md          How others solve this, trade-offs
    ...                  Any other artifacts
```

## Starting or Resuming

Source `feature-docs/new-feature.md` — it handles both:
- **New**: Creates the ideation folder with a seeded README, walks you through exploration
- **Resume**: Scans for in-progress folders, reads all artifacts, picks up where you left off

## Status Tracking

Each folder's `README.md` has YAML frontmatter with status:

```yaml
---
feature: feature-name
status: in-progress       # or: complete
created: 2025-01-15
---
```

- **in-progress**: Still being explored. The `new-feature.md` prompt will offer to resume this.
- **complete**: Distilled into a feature doc in `feature-docs/ready/`. This folder is now an archive.

## Progress Log

The `README.md` has a `## Progress` section with dated entries tracking what was explored, decisions made, and open questions. This allows resuming ideation across sessions:

```markdown
### 2025-01-15 — Initial exploration
- **What we did**: Reviewed existing auth code, identified session management gap
- **Decisions made**: Use httpOnly cookies, not localStorage
- **Open questions**: Which OAuth provider to use later?
```

## README Template

When creating a new ideation folder, seed the README with this format:

```markdown
---
feature: <feature-name>
status: in-progress
created: <date>
---

# Ideation: <feature-name>

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

<!-- Append dated entries here as you explore the feature -->
```

## Rules

- **No format rules** for artifact files — use whatever helps you think
- **Agents never read ideation folders** — test-writer, builder, and reviewer only read the distilled feature doc in `ready/`
- The `ideation-ref` field in feature doc frontmatter links the ready file back to this folder for additional context
