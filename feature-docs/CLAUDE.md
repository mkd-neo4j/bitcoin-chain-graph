# Feature Docs

This directory manages the Agent Teams workflow — a parallel multi-agent development pattern where a test-writer writes failing tests, a builder implements until green, and a reviewer validates quality.

## Directory Structure

```
feature-docs/
  CLAUDE.md             This file — auto-discovered by Claude
  new-feature.md        Source this to start or resume feature ideation
  ideation/             Explore and shape feature ideas (one subfolder per feature)
    CLAUDE.md           Ideation process guide + README template
  ready/                Distilled feature docs waiting for a test-writer
  testing/              Test-writer is writing failing tests
  building/             Builder is implementing until tests pass
  review/               All tests pass, awaiting reviewer
  completed/            Reviewer approved, PR ready
```

## Getting Started

**Source `feature-docs/new-feature.md`** to start the feature workflow. It handles both paths:
- **Ideation first**: Walk through exploring, validating, and designing the feature with artifacts saved to `ideation/<feature-name>/`
- **Skip to ready**: If you already know what you want, go straight to creating a feature doc

## Lifecycle

1. **Ideation** — Human sources `new-feature.md`, creates `ideation/<feature-name>/`, adds research, code reviews, design notes. No format rules. Status tracked in README.md frontmatter.
2. **Ready** — Human distills ideation into a single feature doc in `ready/<feature-name>.md` with YAML frontmatter and GIVEN/WHEN/THEN acceptance criteria.
3. **Testing** — Test-writer reads the feature doc, writes failing tests, commits them on a feature branch, moves the doc here.
4. **Building** — Builder reads the failing tests, implements code until all pass, runs verify, moves the doc here.
5. **Review** — Reviewer checks code quality, conventions, and completeness. Moves to `completed/` if approved, back to `building/` if not.
6. **Completed** — Feature is done. Branch is ready for PR.

## Feature Doc Format

Feature docs in `ready/` through `completed/` use this format. See `ready/example-feature.md` for a filled-out example.

```markdown
---
title: Feature Title
status: ready
priority: high | medium | low
ideation-ref: feature-docs/ideation/feature-name/
affected-files:
  - src/path/to/file1
  - src/path/to/file2
---

# Feature Title

## Summary

One paragraph describing what this feature does and why it exists. Include enough
context for agents making judgment calls during implementation.

## Acceptance Criteria

1. GIVEN [context/precondition] WHEN [action] THEN [expected result]
2. GIVEN [context/precondition] WHEN [action] THEN [expected result]
3. GIVEN [context/precondition] WHEN [action] THEN [expected result]

## Edge Cases

- Description of edge case — expected behavior
- Description of edge case — expected behavior

## Out of Scope

- What this feature explicitly does NOT include
- Prevents agents from scope-creeping into adjacent features

## Technical Notes

- Implementation hints, constraints, or architectural decisions
- Reference existing patterns: "Follow the pattern in src/existing/file"
- Dependencies on other features or external services

## Style Requirements (frontend only)

- Visual specifications, if applicable
- Reference design system components
- Screenshot baselines needed: yes/no
```

**Frontmatter fields**:
- `title` (required): Short descriptive name
- `status` (required): Matches the directory the file is in
- `priority` (required): `high` (blocking), `medium` (important), `low` (nice to have)
- `ideation-ref` (optional): Path to the ideation folder that produced this doc
- `affected-files` (required): Files this feature creates or modifies — defines ownership

## Rules for Agents

- **File ownership**: Each feature doc lists `affected-files` in its frontmatter. Do not modify files owned by another in-progress feature. Check `testing/` and `building/` for conflicts before starting.
- **Test-writer**: Never writes implementation code. Only creates test files.
- **Builder**: Never modifies test files. If tests are wrong, stop and report.
- **Reviewer**: Maps to the `code-reviewer` agent.
- **Moving files IS the status transition** — the `status` field in frontmatter and the directory must stay in sync.
- **Ideation reference**: Feature docs may include `ideation-ref` in frontmatter pointing to the ideation folder for additional context.

## Kickoff Commands

```
@test-writer Pick up feature-docs/ready/<filename>.md
@builder Pick up feature-docs/testing/<filename>.md
@code-reviewer Review feature-docs/review/<filename>.md
```
