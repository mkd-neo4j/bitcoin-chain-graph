# Feature Docs

This directory manages the Agent Teams workflow — a parallel multi-agent development pattern where a test-writer writes failing tests, a builder implements until green, and a reviewer validates quality.

## Directory Structure

```
feature-docs/
  CLAUDE.md             This file — auto-discovered by Claude
  STATUS.md             Progress dashboard — updated by agents after each stage
  new-feature.md        Source this to create a new feature (ideation or direct)
  implement-feature.md  Source this to implement an existing feature doc
  ideation/             Explore and shape feature ideas (one subfolder per feature)
    CLAUDE.md           Ideation process guide + README template
  ready/                Distilled feature docs waiting for a test-writer
  testing/              Test-writer is writing failing tests
  building/             Builder is implementing until tests pass
  review/               All tests pass, awaiting reviewer
  completed/            Reviewer approved, PR ready
```

## Getting Started

**Source `feature-docs/new-feature.md`** to create a new feature. It handles both paths:
- **Ideation first**: Walk through exploring, validating, and designing the feature with artifacts saved to `ideation/<feature-name>/`
- **Skip to ready**: If you already know what you want, go straight to creating a feature doc

**Source `feature-docs/implement-feature.md`** to implement an existing feature. It scans `ready/` for available work, runs pre-flight checks (completeness, file ownership conflicts), and kicks off the test-writer to start the pipeline.

## Lifecycle

1. **Ideation** — Human sources `new-feature.md`, creates `ideation/<feature-name>/`, adds research, code reviews, design notes. No format rules. Status tracked in README.md frontmatter.
2. **Ready** — Human distills ideation into a single feature doc in `ready/<feature-name>.md` with YAML frontmatter and GIVEN/WHEN/THEN acceptance criteria.
3. **Testing** — Test-writer reads the feature doc, writes failing tests, commits them on a feature branch, moves the doc here.
4. **Building** — Builder reads the failing tests, implements code until all pass, runs verify, moves the doc here.
5. **Review** — Reviewer checks code quality, conventions, and completeness. Moves to `completed/` if approved, back to `building/` if not.
6. **Completed** — Feature is done. Branch is ready for PR.

All agents update `feature-docs/STATUS.md` after each stage transition.

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

<!-- Use exact names: function names, field names, error types, status codes.
     The test-writer turns each criterion into an assertion — if it has to
     guess what "works" means, the test will be wrong. -->

1. GIVEN [exact precondition] WHEN [exact action with specific inputs] THEN [exact observable result with named fields/values]
2. GIVEN [exact precondition] WHEN [exact action with specific inputs] THEN [exact observable result with named fields/values]

## Edge Cases

<!-- Tie each case to a specific data pattern or input, not generic error categories. -->

- [Specific input or data condition] — [exact expected behavior with error type/code]
- [Specific input or data condition] — [exact expected behavior with error type/code]

## Out of Scope

<!-- Name the specific code the builder will be tempted to touch, and explain
     why touching it is risky. "Separate feature" alone is not enough. -->

- What this feature does NOT include — [reason + what breaks if agent ignores this]
- What this feature does NOT include — [reason + what breaks if agent ignores this]

## Technical Notes

<!-- Include rejected approaches with the reason they were rejected.
     This prevents agents from rediscovering a "clever" optimization
     that was already considered and ruled out during ideation. -->

- Implementation hints, constraints, or architectural decisions
- Reference existing patterns: "Follow the pattern in src/existing/file"
- **Rejected**: [what was considered] — [specific failure mode or risk]

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

### Writing Effective Acceptance Criteria

Name the functions, fields, error types, and return shapes. The test-writer turns each criterion directly into an assertion.

| Vague (agent has to guess) | Precise (agent can write a test) |
|---|---|
| THEN the login works | THEN `authenticate()` returns a `Session` with non-null `token` |
| THEN an error is shown | THEN it throws `AuthenticationError` with code `"INVALID_CREDENTIALS"` |
| THEN the data is saved | THEN `authStore.getState().session` contains the new `Session` |
| THEN the field is removed | THEN the returned object does NOT include a `legacyField` key |

### Writing Actionable Out of Scope

For each exclusion, name the specific temptation and the risk:

> Do NOT remove the deprecated `validateLegacy()` in `src/auth/validators.ts` — it is still called by the admin module and removing it breaks `AdminAuthProvider`.

"Separate feature" is not enough. Name the file, the code, and the consequence.

### Writing Technical Notes That Prevent Wrong Paths

Include at least one rejected approach when a non-obvious design decision was made:

> **Rejected**: localStorage with encryption wrapper. **Why**: XSS-accessible, no real protection — httpOnly cookies are invisible to JS entirely.

This prevents a builder from independently arriving at the same "obvious" optimization and reintroducing a known problem.

## Rules for Agents

- **File ownership**: Each feature doc lists `affected-files` in its frontmatter. Do not modify files owned by another in-progress feature. Check `testing/` and `building/` for conflicts before starting.
- **Test-writer**: Never writes implementation code. Only creates test files.
- **Builder**: Never modifies test files. If tests are wrong, stop and report.
- **Reviewer**: Maps to the `code-reviewer` agent.
- **Moving files IS the status transition** — the `status` field in frontmatter and the directory must stay in sync. This is not optional. The `task-completed.sh` hook blocks task completion if a feature doc's `status:` field does not match its directory.
- **Progress dashboard**: Update `feature-docs/STATUS.md` after every stage transition. This is the only way the next agent (or the orchestrator) can orient without reading every directory.
- **Ideation reference**: Feature docs may include `ideation-ref` in frontmatter pointing to the ideation folder for additional context.

### Lifecycle Compliance Checklist

Every agent must complete ALL of these before finishing a task. The `task-completed.sh` hook enforces items 1-2 deterministically — your task WILL be rejected if they are not done.

1. **Feature doc is in the correct directory** for the current stage (e.g., `testing/` after test-writer, `review/` after builder)
2. **`status:` frontmatter matches the directory name** (e.g., `status: testing` for a doc in `testing/`)
3. **`feature-docs/STATUS.md` has a current entry** reflecting the new stage
4. **Feature doc is included in the git commit** (not just source/test files)

### Deterministic Enforcement

The `task-completed.sh` hook scans all feature docs in `ready/`, `testing/`, `building/`, `review/`, and `completed/`. For each doc with a `status:` field, it verifies the value matches the directory name. If any mismatch is found, task completion is blocked with exit code 2. This is not a prompt convention — it is a shell script that runs automatically and cannot be skipped.

## Kickoff Commands

```
@test-writer Pick up feature-docs/ready/<filename>.md
@builder Pick up feature-docs/testing/<filename>.md
@code-reviewer Review feature-docs/review/<filename>.md
```
