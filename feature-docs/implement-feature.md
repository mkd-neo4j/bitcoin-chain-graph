# Implement Feature

Source this file (`@feature-docs/implement-feature.md`) to pick up and implement an existing feature doc.

---

You are helping me kick off the agent teams pipeline for an existing feature. Follow these instructions carefully.

## Coordinator Role — Read-Only for Code

You are the **coordinator**. Your job is to orchestrate the pipeline — scan for work, run pre-flight checks, invoke agents, and verify lifecycle compliance between stages. You **NEVER** write implementation or test code yourself.

### What You MUST NOT Do

- **NEVER** use Write, Edit, or sed on implementation or test files
- **NEVER** use Write, Edit, or sed on any file listed in a feature doc's `affected-files`
- **NEVER** edit the same files an agent is working on
- **NEVER** implement a fix directly — even a one-line change. All code changes go through test-writer → builder. TDD is the workflow, not a suggestion.
- **NEVER** launch the next agent for a feature until the current agent has completed. The pipeline is **per-feature sequential**: test-writer must finish before builder starts, builder must finish before reviewer starts. Cross-feature parallelism is fine if `affected-files` don't overlap.
- If code needs fixing — re-invoke the responsible agent with specific error details
- If tests are wrong — report to the user or re-invoke the test-writer with the issue

### What You May Do

- **Read, Grep, Glob** on any file (read-only inspection is always fine)
- **Task** to invoke agents (`@test-writer`, `@builder`, `@code-reviewer`)
- **sed** on feature doc `status:` frontmatter field only (lifecycle housekeeping)
- **mv** to move feature docs between lifecycle directories
- **Write/Edit** on `feature-docs/STATUS.md` only (progress dashboard)
- **sed** on ideation README `status:` frontmatter field (lifecycle housekeeping — same scope as feature doc status updates)
- **Write/Edit** on `feature-docs/ideation/*/README.md` (lifecycle housekeeping — progress entries at pipeline completion)

When you encounter a problem with code — wrong implementation, failing tests, missing files — your response is always to **send the agent back with specific instructions**, never to fix it yourself.

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

4. **Check ideation README** — if the feature doc has an `ideation-ref` field, read the ideation README. If its status is still `in-progress` (meaning the distillation step forgot to update it), notify and fix:

   > The ideation README for this feature still shows `in-progress` but a ready feature doc exists. Updating to `complete`.

   ```bash
   sed -i '' 's/status: in-progress/status: complete/' feature-docs/ideation/<feature-name>/README.md
   ```

## Step 3 — Check Branch

Check if a feature branch `feat/<feature-name>` already exists (run `git branch --list "feat/<feature-name>"`).

- If the branch doesn't exist, the test-writer will create it
- If the branch exists, note this — it may be from a previous attempt

## Step 4 — Kickoff

**If pre-flight checks passed with no warnings** (no missing sections, no file ownership conflicts, no existing branch from a previous attempt), skip confirmation and go straight to the kickoff command:

> **Kicking off:**
>
> - **Feature**: <title>
> - **Agent**: @test-writer
> - **Branch**: `feat/<feature-name>` (will be created)
> - **Affected files**: <list from frontmatter>
>
> ```
> @test-writer Pick up feature-docs/ready/<filename>.md
> ```

**If any warnings were raised** (missing sections, file conflicts, or branch already exists), show the plan and ask for confirmation before providing the kickoff command:

> **Implementation Plan**
>
> - **Feature**: <title>
> - **Agent**: @test-writer
> - **Action**: Write failing tests for all acceptance criteria
> - **Branch**: `feat/<feature-name>` (exists — previous attempt?)
> - **Affected files**: <list from frontmatter>
> - **Warnings**: <list warnings from Steps 2–3>
>
> Ready to kick off?

On confirmation, output the kickoff command:

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

**Critical: Per-feature sequential.** Within a single feature, only one agent works at a time. Do NOT launch the next agent until the current agent has **completed its task and gone idle**. Launching the builder while the test-writer is still running causes file conflicts. Multiple features may run in parallel if their `affected-files` don't overlap.

### Between test-writer and builder

**Wait for the test-writer to complete its task before proceeding.** If the test-writer is still running, do not launch the builder — both agents will edit overlapping files and cause conflicts.

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

**Wait for the builder to complete its task before proceeding.**

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
3. **Check for non-blocking issues** in the review report. If the reviewer flagged follow-ups, present them to the user:

> The reviewer approved the feature but flagged these non-blocking issues:
>
> 1. [issue from review report]
> 2. [issue from review report]
>
> Would you like to:
> - **Create follow-ups** — route each through the TDD pipeline
> - **Skip** — ship as-is and track them separately
> - **Mark as blocking** — move the doc back to `ready/` and route through the full TDD pipeline

4. **Update ideation README** (if applicable): Read the feature doc's `ideation-ref` frontmatter field. If it points to an ideation folder with a README, update it:
   ```bash
   sed -i '' 's/status: complete/status: shipped/' feature-docs/ideation/<feature-name>/README.md
   ```
   Then append a progress entry to the README's `## Progress` section:
   ```markdown
   ### <today's date> — Pipeline complete
   - **Result**: Feature shipped through agent teams pipeline (test-writer → builder → reviewer)
   - **Feature doc**: `feature-docs/completed/<filename>.md`
   - **Branch**: `feat/<feature-name>`
   ```
   If no `ideation-ref` field or no ideation folder, skip this step silently.

5. The feature branch is ready for PR (unless the user chose to fix follow-ups first)

### If the reviewer flags blocking issues

The reviewer reports issues back to the coordinator — it **never fixes code itself**. The coordinator reads the review report and determines the routing. This triggers an automatic rework loop — no human intervention needed unless it stalls.

**Determine the routing:**

Read the review report and classify each issue:

- **Implementation issues** (wrong logic, missing error handling, convention violations, dead code, unused imports): route to the **builder**
- **Test gaps** (missing test coverage, wrong test expectations, missing edge case tests): route to the **test-writer** first, then the **builder**, then back to the reviewer — the full TDD cycle

**Implementation-only rework cycle** (builder → reviewer):

1. **Verify the doc location**: `ls feature-docs/building/<filename>.md`
   - If the reviewer didn't move it back, move it: `sed` the status to `building`, `mv` to `feature-docs/building/`
2. **Check STATUS.md** reflects `building` status — update if the reviewer did not
3. **Re-invoke the builder** with the specific issues:
   ```
   @builder The reviewer found issues with feature-docs/building/<filename>.md:
   - [specific issue 1 from review]
   - [specific issue 2 from review]
   Fix these and move the doc back to review/ when tests pass.
   ```
4. **Wait for the builder to complete**, then re-invoke the reviewer (follow the "Between builder and reviewer" steps above)

**Test-gap rework cycle** (test-writer → builder → reviewer):

1. Move the doc back to `ready/`: `sed` the status to `ready`, `mv` to `feature-docs/ready/`
2. **Update STATUS.md** to reflect `ready` status
3. **Re-invoke the test-writer** with the specific gaps:
   ```
   @test-writer The reviewer found test gaps in feature-docs/ready/<filename>.md:
   - [missing test coverage for X]
   - [wrong expectation in Y test]
   Add or fix failing tests for these issues.
   ```
4. **Wait for the test-writer to complete**, then invoke the builder, then the reviewer — full pipeline

**Do NOT fix the code yourself** — the coordinator routes, the agents fix.

**Circuit breaker:** Track the number of rework cycles. After **3 round-trips**, stop and escalate to the user:

> The builder and reviewer have cycled 3 times on **<feature title>**. Remaining issues:
>
> - [issue from latest review]
>
> This may indicate a spec ambiguity or a problem the builder cannot resolve alone. How would you like to proceed?

### Follow-up issues after pipeline completes

When the user or reviewer identifies an issue after the feature is in `completed/` — even a "small" one-line fix:

1. **NEVER fix it directly** — this is the most common way the coordinator breaks TDD
2. **Ask the user** how to handle it:

> Follow-up issue identified: [description]
>
> Options:
> - **New feature doc** — create a separate doc in `ready/` for this fix
> - **Amend existing** — move the completed doc back to `ready/`, add acceptance criteria for the fix
> - **Skip** — note it as a known issue, ship without fixing

3. **Route through the full pipeline**: `@test-writer` → `@builder` → `@code-reviewer`
4. Even for trivial fixes — a failing test proves the bug exists, implementation proves it's fixed, review proves it's correct

### If the pipeline stalls mid-stage

If an agent exits without completing lifecycle steps:

1. Check which directory the feature doc is actually in: `ls feature-docs/*/<filename>.md`
2. Check the `status:` field in the frontmatter: `head -5 feature-docs/*/<filename>.md`
3. If the status and directory are out of sync, fix them manually
4. Re-launch the appropriate agent for the current stage

---

Start with Step 1 now.
