# Implement Feature

Source this file (`@feature-docs/implement-feature.md`) to pick up and implement an existing feature doc.

---

You are helping me kick off the agent teams pipeline for an existing feature. Follow these instructions carefully.

## Step 1 — Scan for Ready Features

Scan `feature-docs/ready/` for `.md` files (excluding example-feature.md). For each feature doc found, read its YAML frontmatter to get the `title`, `priority`, and `affected-files`.

**If features are found**, present them in a table:

> Here are features ready for implementation:
>
> | # | Feature | Priority | Affected Files |
> |---|---------|----------|----------------|
> | 1 | Feature Title | high | 4 files |
> | 2 | Feature Title | medium | 2 files |
>
> Which feature do you want to implement? (number or name)

**If no features are found**, tell me:

> No feature docs found in `feature-docs/ready/`.
>
> To create a new feature, source `feature-docs/new-feature.md`.

Then stop.

## Step 2 — Pre-flight Checks

After I select a feature:

1. **Read the full feature doc** — understand what the feature does, its acceptance criteria, and affected files

2. **Verify completeness** — confirm the feature doc has all required sections: Summary, Acceptance Criteria, Edge Cases, Out of Scope, and `affected-files` in frontmatter. If anything is missing, flag it:

> This feature doc is missing: **<section>**. The test-writer agent needs this to work effectively. Would you like to add it now, or proceed anyway?

3. **Check file ownership** — scan `feature-docs/testing/` and `feature-docs/building/` for other in-progress features. Compare their `affected-files` with the selected feature's `affected-files`. If any files overlap, warn me:

> **File ownership conflict detected.**
>
> `<filename>` is also claimed by **<other feature title>** (status: <status>).
>
> Running both features in parallel risks conflicting edits. Options:
> - Wait for the other feature to complete first
> - Proceed anyway (only if you're sure the files won't conflict)
>
> What would you like to do?

## Step 3 — Check Branch

Check if a feature branch `feat/<feature-name>` already exists (run `git branch --list "feat/<feature-name>"`).

- If the branch doesn't exist, the test-writer will create it
- If the branch exists, note this — it may be from a previous attempt

## Step 4 — Kickoff

Show me what will happen:

> **Implementation Plan**
>
> - **Feature**: <title>
> - **Agent**: @test-writer
> - **Action**: Write failing tests for all acceptance criteria
> - **Branch**: `feat/<feature-name>` (exists / will be created)
> - **Affected files**: <list from frontmatter>
>
> Ready to kick off?

On confirmation, output the exact kickoff command for me to run:

```
@test-writer Pick up feature-docs/ready/<filename>.md
```

## Step 5 — What Happens Next

After I kick off the agent, explain:

> The **@test-writer** is now working on **<feature title>**.
>
> **What happens automatically:**
> - The `Stop` hook runs `scripts/verify.sh` after each agent response (if code changed)
> - The `TaskCompleted` hook runs the full verify pipeline before any task can be marked done
> - The `TeammateIdle` hook will auto-assign the next agent when the current one finishes:
>   test-writer → builder → code-reviewer
>
> **Manual handoff** (if you prefer to control each step):
> - After test-writer finishes: `@builder Pick up feature-docs/testing/<filename>.md`
> - After builder finishes: `@code-reviewer Review feature-docs/review/<filename>.md`
>
> **If the pipeline stalls** (agent stops mid-feature):
> - Features in `testing/` or `building/` are locked to the current agent
> - To unlock, move the doc back to `ready/` and source this file again

## Step 6 — Pipeline Orchestration (Between-Stage Verification)

Whether the pipeline runs via TeammateIdle hooks or manual orchestration, **verify lifecycle compliance between every stage**. Agents sometimes skip the doc-move and STATUS.md update steps. The `task-completed.sh` hook enforces this deterministically, but if you are orchestrating manually, check before launching the next agent.

### Between test-writer and builder

After the test-writer finishes, verify before invoking the builder:

1. **Check the feature doc location**: `ls feature-docs/testing/<filename>.md`
   - If the file is still in `feature-docs/ready/`, the test-writer skipped the move step. Fix it:
     ```bash
     sed -i '' 's/status: ready/status: testing/' feature-docs/ready/<filename>.md
     mv feature-docs/ready/<filename>.md feature-docs/testing/
     ```
2. **Check STATUS.md**: `grep '<feature-name>' feature-docs/STATUS.md`
   - If no entry exists, add one showing `testing` status
3. **Then launch**: `@builder Pick up feature-docs/testing/<filename>.md`

### Between builder and reviewer

After the builder finishes, verify before invoking the reviewer:

1. **Check the feature doc location**: `ls feature-docs/review/<filename>.md`
   - If the file is still in `feature-docs/building/`, the builder skipped the move step. Fix it:
     ```bash
     sed -i '' 's/status: building/status: review/' feature-docs/building/<filename>.md
     mv feature-docs/building/<filename>.md feature-docs/review/
     ```
2. **Check STATUS.md**: `grep '<feature-name>' feature-docs/STATUS.md`
   - If the entry still says `building`, update it to `review`
3. **Then launch**: `@code-reviewer Review feature-docs/review/<filename>.md`

### After reviewer approves

1. **Verify doc moved to completed**: `ls feature-docs/completed/<filename>.md`
2. **Update STATUS.md** if the reviewer did not
3. The feature branch is now ready for PR

### If the pipeline stalls mid-stage

If an agent exits without completing lifecycle steps:

1. Check which directory the feature doc is actually in: `ls feature-docs/*/<filename>.md`
2. Check the `status:` field in the frontmatter: `head -5 feature-docs/*/<filename>.md`
3. If the status and directory are out of sync, fix them manually
4. Re-launch the appropriate agent for the current stage

---

Start with Step 1 now.
