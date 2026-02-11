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

This is the open-ended phase. Tell me:

> We're now in the exploration phase. Here's what we can do:
>
> - **Review code**: Ask me to analyse specific files or patterns
> - **Research approaches**: I can look at how something could be implemented
> - **Design**: We can sketch out data flows, component structures, or API contracts
> - **Write notes**: I'll save anything important to the ideation folder
> - **Create the feature**: When you're ready, say "create the feature" and I'll distill everything into a ready file
>
> What would you like to explore?

During this phase:

- **Save important artifacts** to the ideation folder with descriptive filenames (e.g., `api-design.md`, `state-management-notes.md`, `component-analysis.md`)
- **After each significant piece of work**, append a progress entry to the README.md's `## Progress` section:
  ```markdown
  ### <date> — <brief title>
  - **What we did**: <summary>
  - **Decisions made**: <any conclusions reached>
  - **Open questions**: <what's still unresolved>
  ```
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

If the user chose to skip ideation at Step 2, guide them through creating a feature doc directly. Ask one question at a time, moving to the next after each answer.

**8a — Priority**
Ask me to choose: high (blocking other work), medium (important), or low (nice to have).

**8b — Summary**
Ask me to describe in one paragraph what this feature does and why it exists. Probe for context — ask a follow-up if my answer is vague. Agents need enough context to make judgment calls during implementation.

**8c — Acceptance Criteria**
Guide me through writing testable acceptance criteria in GIVEN/WHEN/THEN format. After each one I provide, ask if there are more. If my criteria are vague, push back — remind me that each criterion becomes at least one automated test. The test-writer agent cannot work with "the login should work." It needs "GIVEN valid credentials WHEN the user submits THEN a session token is stored."

**8d — Edge Cases**
Ask me what could go wrong or what unusual inputs might occur. Suggest common ones I might be missing based on the feature (empty inputs, network failures, concurrent access, boundary values, missing permissions). Format as: description — expected behavior.

**8e — Affected Files**
Ask me which files this feature will create or modify. Explain that these define file ownership — no other feature's agents should touch these files while this one is in progress. If I'm unsure, help me think through it based on the acceptance criteria (what stores, components, routes, or modules would need to change).

**8f — Out of Scope**
Ask me what this feature explicitly does NOT include. Suggest likely scope creep items based on what I've described. This prevents agents from building adjacent functionality.

**8g — Technical Notes**
Ask if there are implementation hints, constraints, existing patterns to follow, or dependencies on other features. This section is optional but helps the builder agent make better decisions.

**8h — Style Requirements (frontend only)**
If this is a frontend feature, ask about visual specifications, design system components to use, and whether screenshot baselines are needed. Skip this for Python or Rust projects.

**8i — Generate and Review**
Generate the complete feature doc using the format from `feature-docs/CLAUDE.md` (see the "Feature Doc Format" section). Before writing the file, show me the full doc and ask if I want to change anything.

**8j — File Ownership Check**
Before saving, check `feature-docs/testing/` and `feature-docs/building/` for any existing feature docs whose `affected-files` overlap with this one. Warn me if there are conflicts.

**8k — Save and Next Steps**
Write the file to `feature-docs/ready/<feature-name>.md` where the filename is derived from the title (lowercase, hyphens). Then tell me the exact command to kick off the test-writer:

```
@test-writer Pick up feature-docs/ready/<filename>.md
```

Or source `feature-docs/implement-feature.md` to run the pipeline with pre-flight checks (file ownership validation, section completeness).

---

Start with Step 1 now.
