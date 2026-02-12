# New Feature

Source this file (`@feature-docs/new-feature.md`) to start or resume feature work.

---

You are guiding me through the feature ideation and creation process. Follow these instructions carefully.

## Step 1 — Check for In-Progress Features

Scan `feature-docs/ideation/` for any subfolders. For each subfolder, read its `README.md` and check the YAML frontmatter for `status: in-progress`.

**If in-progress features exist**, list them with their feature name and the last entry from the `## Progress` section, then ask:

> I found these in-progress features:
> - **feature-name**: last progress entry summary
>
> Would you like to resume one of these, or start a new feature?

**If no in-progress features exist** (or ideation/ is empty), go straight to Step 2.

## Step 2 — Understand the Feature

Ask me:

> What feature do you want to build? Give me a brief description — what does it do and why does it need to exist?

Listen to my answer. Ask follow-up questions if my description is vague. You need enough context to understand:
- What problem this solves
- Who benefits from it
- How it fits into the existing application

Once you understand, summarise the feature back to me in 2-3 sentences for confirmation.

Then ask:

> Do you want to explore this first (ideation), or do you already know exactly what you want and want to go straight to creating a feature doc?

- **If they choose ideation** → continue to Step 3
- **If they choose to skip** → jump to Step 8 (Direct Feature Doc Creation)

## Step 3 — Create the Ideation Folder

Create the ideation folder and seed the README using the template from `feature-docs/ideation/CLAUDE.md` (see the "README Template" section):

```
feature-docs/ideation/<feature-name>/README.md
```

Set the frontmatter with the feature name, `status: in-progress`, and today's date. Under the `## Progress` section, add the first entry:

```markdown
### <today's date> — Initial exploration
- **Summary**: <1-2 sentence description of what we discussed>
- **Open questions**: <list any unknowns identified so far>
```

Tell me the folder was created.

## Step 4 — Validate Against Existing Code

Now do due diligence on the codebase:

1. **Read the project's CLAUDE.md** to understand the stack, conventions, and architecture
2. **Search for related code** — look for files, functions, components, or modules that this feature will interact with
3. **Report what you found**: which files exist, what patterns they follow, what will need to change

Save your findings as `feature-docs/ideation/<feature-name>/code-review.md`.

Ask me: "Based on what I found, does this align with how you see this feature working? Anything I should look at more closely?"

## Step 5 — Iterative Exploration

This is the open-ended phase. Before showing the menu, **check what has already been done**:

1. List files in the `feature-docs/ideation/<feature-name>/` folder
2. Read the `## Progress` section of the README.md

Use the artifacts present to determine which activities are completed:

| Activity | Completed when |
|---|---|
| Review code | `code-review.md` exists |
| Research approaches | `research.md` or `spike-results.md` exists |
| Design | Design artifacts exist (e.g., `api-design.md`, `component-analysis.md`, `state-management-notes.md`) |

Then present a **status-aware menu**. Show completed activities as a summary, and only offer the remaining options:

> **Completed so far:**
> - <for each completed activity, show a one-line summary from the most recent relevant progress entry>
>
> **What's next?**
> - <only list uncompleted activities from the table above>
> - **Write notes**: I'll save anything important to the ideation folder
> - **Create the feature**: When you're ready, say "create the feature" and I'll distill everything into a ready file
>
> What would you like to explore?

If **all three activities** (review code, research, design) are completed, skip the uncompleted list and just show:

> **Completed so far:**
> - <summaries>
>
> **What's next?**
> - **Write notes**: I'll save anything important to the ideation folder
> - **Create the feature**: When you're ready, say "create the feature" and I'll distill everything into a ready file
> - Or ask me to dive deeper into any of the completed areas
>
> What would you like to do?

During this phase:

- **Save important artifacts** to the ideation folder with descriptive filenames (e.g., `api-design.md`, `state-management-notes.md`, `component-analysis.md`)
- **After each significant piece of work**, append a progress entry to the README.md's `## Progress` section:
  ```markdown
  ### <date> — <brief title>
  - **What we did**: <summary>
  - **Decisions made**: <any conclusions reached>
  - **Open questions**: <what's still unresolved>
  ```
- **Re-check artifacts** each time you show this menu — the completed list should always reflect the current state
- **Stay in this phase** until I say "create the feature" or indicate I'm ready to proceed

## Step 6 — Resume (If Picking Up an In-Progress Feature)

If we're resuming from Step 1:

1. Read **every file** in the ideation folder
2. Read the `## Progress` section of README.md to understand the history
3. Summarise: what has been explored, what decisions were made, and what open questions remain
4. Ask me: "Here's where we left off. What would you like to focus on next?"
5. Continue from Step 5 (iterative exploration)

## Step 7 — Create the Feature Doc (From Ideation)

When I say "create the feature" (or similar):

1. **Read all files** in the ideation folder
2. **Draft a feature doc** using the format from `feature-docs/CLAUDE.md` (see the "Feature Doc Format" section):
   - **Title**: Derived from our discussions
   - **Priority**: Ask me to choose (high / medium / low)
   - **Ideation ref**: Set to `feature-docs/ideation/<feature-name>/`
   - **Summary**: Synthesised from all artifacts
   - **Acceptance Criteria**: Extract testable behaviours in GIVEN/WHEN/THEN format. Push back on anything vague — each criterion becomes at least one automated test
   - **Edge Cases**: Pull from failure modes discussed during ideation
   - **Affected Files**: Identified from code reviews and design notes
   - **Out of Scope**: Anything discussed but deferred or rejected
   - **Technical Notes**: Implementation constraints, patterns to follow, dependencies
   - **Style Requirements**: Only if this is a frontend feature with visual specs
3. **Flag gaps**: Tell me what's missing or unclear before we finalise
4. **Show me the full draft** and ask if I want changes
5. **Check for conflicts**: Read feature docs in `feature-docs/testing/` and `feature-docs/building/` — warn if any `affected-files` overlap
6. **Save** to `feature-docs/ready/<feature-name>.md`
7. **Update the ideation README**: Set `status: complete` in frontmatter, add a final progress entry linking to the ready file
8. **Tell me the kickoff command**:

```
@test-writer Pick up feature-docs/ready/<feature-name>.md
```

Or source `feature-docs/implement-feature.md` to run the pipeline with pre-flight checks (file ownership validation, section completeness).

---

## Step 8 — Direct Feature Doc Creation (Skip Ideation)

If the user chose to skip ideation at Step 2, generate the feature doc autonomously using all available context — prior conversation, code reviews, codebase exploration, error reports, etc. **Do not interrogate the user section by section.** Draft first, review once.

**8a — Draft the Complete Feature Doc**

Using the format from `feature-docs/CLAUDE.md` (see the "Feature Doc Format" section), fill in every section:

- **Priority**: Infer from conversation context. If genuinely unclear, ask once before drafting.
- **Summary**: Synthesise from everything discussed so far.
- **Acceptance Criteria**: Write testable GIVEN/WHEN/THEN criteria based on the problem and solution discussed. Each criterion becomes at least one automated test — be precise with function names, field names, error types, and return shapes.
- **Edge Cases**: Infer from the problem domain (failures, timeouts, boundary conditions, concurrent access, invalid inputs).
- **Affected Files**: Infer from codebase exploration. If you haven't explored yet, search the codebase now to identify the relevant files.
- **Out of Scope**: Identify likely scope creep items and adjacent functionality that should not be touched.
- **Technical Notes**: Include implementation constraints, patterns to follow, and any rejected approaches with reasons.
- **Style Requirements**: Only for frontend features with visual specs. Skip otherwise.

For any section where you genuinely lack information, flag it in the draft with a note (e.g., `<!-- REVIEW: I wasn't sure about X — please check -->`). Do not stop to ask.

**8b — Show Draft and Ask for Changes**

Show the complete feature doc and ask: "Here's the feature doc. Want to change anything?"

Apply any requested changes. Do not re-ask about sections the user didn't flag.

**8c — File Ownership Check**

Before saving, check `feature-docs/testing/` and `feature-docs/building/` for any existing feature docs whose `affected-files` overlap with this one. Warn if there are conflicts.

**8d — Save and Next Steps**

Write the file to `feature-docs/ready/<feature-name>.md` where the filename is derived from the title (lowercase, hyphens). Then tell the exact command to kick off the test-writer:

```
@test-writer Pick up feature-docs/ready/<filename>.md
```

Or source `feature-docs/implement-feature.md` to run the pipeline with pre-flight checks (file ownership validation, section completeness).

---

Start with Step 1 now.
