# yoyo Eval Strategy

This directory now has four different eval tiers. They should not be treated as interchangeable.

## Tier 0: Smoke tests

Files:

- `evals/tasks/uuid/`
- `evals/tasks/httprouter/`
- `evals/tasks/semver/`
- `evals/harness/`

Purpose:

- verify the compare harness
- catch obvious regressions between tags
- confirm Codex treatment actually uses `yoyo`

These are **not** the product-truth benchmark for `yoyo`. They are too local, too deterministic, and too easy for plain Codex to solve with direct file edits.

## Tier 1: Tool accuracy

Files:

- `evals/tasks/*.json`
- `evals/run.py`
- `evals/run_semantic.py`
- `evals/write_run.py`
- `evals/REPORT.md`

Purpose:

- validate structural tool correctness
- validate semantic search recall
- validate write-tool safety at the unit-task level

These answer "is the tool correct?" They do not answer "does this make an engineering agent better?"

## Tier 2: Directed tool-use evals

Files:

- `evals/tasks/directed_tool_use.json`
- `evals/tasks/directed_tool_use_first3.json`
- `evals/tasks/directed_tool_use_write_batch.json`

Purpose:

- evaluate `yoyo` as an MCP tool suite, not as a stand-alone coding agent
- measure whether explicit engineering instructions make `yoyo` useful on real codebase work
- reward correct use of `boot`, `index`, `inspect`, `search`, `impact`, and `change` under direction

This should be the primary product benchmark.

`yoyo` is not itself an autonomous coding agent. It is a collection of structural repo tools exposed over MCP. The main eval question is therefore not:

- "Can treatment autonomously solve the task with no coaching?"

It is:

- "When the model is given realistic engineering instructions, does `yoyo` help it do the work better?"

Directed tool-use tasks should be multi-turn and command-based. Typical phases:

1. locate the likely symbols and files
2. explain the dependency chain or blast radius
3. make the minimal correct edit
4. verify with the right test or build command

The evaluator is allowed to give neutral but concrete commands between phases. This is not contamination. It is the product being used as intended.

The directed suite should explicitly cover three task modes:

- read-only
- write-only
- read-then-write

The third should be the primary one, but all three matter.

It should also explicitly include principal-engineer-level questions drawn from the codebase, not just local implementation questions.

`directed_tool_use_first3.json` is the first mixed-mode pilot set.

`directed_tool_use_write_batch.json` is the first concrete write-only batch. It currently covers:

- `ripgrep-global-gitignore`
- `uuid`
- `httprouter`
- `semver`

Current WIP directed results:

- `directed-ripgrep-read-only-2026-03-13.md` records one clean read-only ripgrep run where Codex stayed on `yoyo` for `22/22` tool calls.
- `directed-ripgrep-write-only-2026-03-13.md` records the first clean single-task write-only ripgrep result.
- `directed-write-batch-2026-03-13.md` records the first clean multi-task write-only batch:
  - `ripgrep`, `uuid`, and `httprouter`
- `directed-semver-write-only-2026-03-13.md` keeps `semver` separate:
  - the patch is correct and passes manual `cargo test`
  - the fixture's exact verify command, `cargo test --test *`, is malformed and still needs fixing before strict scoring

## Tier 3: Daily engineering evals

Files:

- `evals/tasks/realistic_daily_engineering.json`
- `evals/tasks/realistic_daily_engineering_first5.json`

Purpose:

- evaluate `yoyo` on work that looks like real codebase engineering
- reward repo understanding, ambiguity handling, and safe multi-file change planning
- measure the kinds of tasks where `boot`, `index`, `inspect`, `search`, `impact`, and `change` should matter

This is still useful, but it should be treated as an integration benchmark, not the primary product benchmark.

`realistic_daily_engineering.json` defines the task shapes.

`realistic_daily_engineering_first5.json` is the first concrete batch selected from real merged PRs in public repositories. It is intentionally source-focused first: repo, source PR, pre-fix SHA, reference merge SHA, natural prompt, and verification plan. The remaining work is fixtureizing those tasks so the compare harness can run them reproducibly.

## Why puncture tasks are insufficient

Puncture tasks are useful, but they overweight local bug repair:

- the shortest path is usually "read failing file, patch, rerun tests"
- they do not force structural navigation
- they do not reflect normal engineering ambiguity
- they under-measure impact tracing and safe multi-file edits
- they make native agent tools look stronger than they really are on large codebases

Use them as smoke tests. Do not use them as the main decision-maker for roadmap priority.

## Why autonomous-only evals are insufficient

Autonomous issue-first runs are still worth keeping, but they are not the right primary test for `yoyo`.

Why:

- `yoyo` is not a planner or agent policy layer
- the model may underuse or misuse the tools without explicit instruction
- failures in autonomy can come from prompt policy or tool selection, not tool value
- this over-attributes responsibility to `yoyo` for behavior owned by the model/runtime

So the benchmark stack should distinguish:

- tool value under direction
- autonomous integration behavior

The first is the product truth. The second is an ecosystem compatibility check.

## Requirements for directed tool-use tasks

Every directed task should satisfy most of these:

- requires at least 3 meaningful repo hops before the correct edit is obvious
- can be split into locate, explain, change, and verify phases
- benefits from structural repo understanding more than raw grep
- includes at least one plausible wrong abstraction or wrong caller path
- has a natural engineering objective, not a synthetic "use this tool now" script
- can be scored on both outcome and intermediate reasoning quality
- forbids access to hidden oracle material through local git history (`git log`, `git show`, `git blame`, cross-commit diffs, or direct inspection of the merged fix)

If the agent inspects the hidden upstream fix through local history, the run is contaminated and should be discarded.

## Directed task modes

### Read-only

Purpose:

- test whether `yoyo` helps the model find the right symbols, files, and dependency chains
- measure structural understanding without giving credit for a lucky patch

Examples:

- "Find the 3 files most likely involved in this bug."
- "Explain what breaks if we change this symbol."
- "Is this helper safe to delete?"
- "Which layer should own this fix and why?"
- "What invariant must remain true if we change this subsystem?"

Primary scoring:

- locate quality
- explanation quality
- blast-radius accuracy
- irrelevant-file mentions
- elapsed time and tool churn

### Write-only

Purpose:

- test whether `yoyo` helps land a correct and minimal patch once the target surface is already known
- isolate edit quality from exploration quality

Examples:

- "Update this signature and its directly affected callers."
- "Patch this known function with the minimal change."
- "Rename this known symbol safely."

Write-only tasks should force the model to cross into editing once the surface is already known. The command wording should be explicit about using `change`, keeping the patch minimal, and limiting any extra confirming reads before the first edit.

Write-only tasks should not begin with ownership or blast-radius questions. Those belong in `read_only` or `read_then_write`. If the task starts by rediscovering the fix surface, it is no longer isolating write quality.

For puncture-backed write tasks, scope quality should be measured relative to the post-setup baseline. The injected puncture test file is already dirty before the agent starts, so raw final `git status` overcounts the agent's write surface.

Primary scoring:

- completion
- diff quality
- scope quality
- wrong edits
- hallucinated APIs

### Read-then-write

Purpose:

- test the full workflow an engineer actually cares about
- require the model to investigate first and then edit based on that understanding

Examples:

- "Find the likely implementation area, explain the dependency chain, then make the minimal patch."
- "Determine whether the failure is in the implementation or the test, then fix the right side."
- "Trace the blast radius, update the right surfaces, and run the narrowest sufficient verification."
- "Decide the correct ownership layer, explain the invariants and regression risks, then patch only that layer."

Primary scoring:

- read quality
- write quality
- transition quality between analysis and patch
- completion
- elapsed time
- retries
- scope and diff quality

## Requirements for realistic autonomous tasks

Every realistic task should satisfy most of these:

- requires at least 3 meaningful repo hops before the correct edit is obvious
- starts from a natural engineering prompt, not a hidden planted-bug description
- touches real abstractions used by the codebase
- has at least one plausible wrong turn
- rewards structural lookup over raw grep
- can be scored with tests, diff quality, and behavior metrics

## Directed tool-use task templates

The first directed suite should include tasks like these:

1. Locate the fix surface
   Instruction: find the smallest set of files and symbols likely involved in the bug, and justify the choice before editing.
2. Explain the dependency chain
   Instruction: trace how behavior flows from the user-facing surface to the actual implementation and name the likely blast radius.
3. Safe multi-file change
   Instruction: make the change only after identifying callers or downstream effects that must move with it.
4. Refactor with justification
   Instruction: rename or reshape an internal API, but first enumerate the impacted symbols and test surfaces.
5. Investigation before edit
   Instruction: answer "what breaks if we change this?" and only then apply the minimal patch.
6. Verify with targeted commands
   Instruction: choose the most relevant test or build command and explain why it is sufficient.
7. Ownership and invariants
   Instruction: identify which layer should own the change, what invariants must remain true, and what the main regression risks are before editing.
8. Migration and rollout judgment
   Instruction: explain whether the change should be local, coordinated across callers, or split into phases.

And they should be distributed across modes:

- 3 read-only tasks
- 3 write-only tasks
- 5 read-then-write tasks

## Ten realistic autonomous task templates

The first suite should include tasks like these:

1. Bug triage from failing integration test
   Root cause sits 2-4 symbols away from the failing surface and requires updating both implementation and tests.
2. Feature flag propagation
   Add a new option that must travel through config parsing, model wiring, runtime behavior, and docs or tests.
3. Safe API rename
   Rename a public or crate-local API across implementations, call sites, and tests without collateral edits.
4. Signature migration
   Add or remove a parameter on a shared helper and update downstream callers correctly.
5. Endpoint behavior change
   Adjust request validation, serialization, and handler behavior across router, model, and tests.
6. Contract mismatch fix
   Trace an interface or trait behavior bug where the failing assertion appears far from the source.
7. Investigation-only impact task
   Answer "what breaks if we change this?" accurately before any edit is made.
8. Dead code or safe-delete task
   Determine whether a symbol can be removed, defend the answer, and only edit if the impact is safe.
9. Test maintenance after intended behavior shift
   Distinguish stale tests from broken implementation, then update the correct side.
10. Small feature addition in a large repo
    Implement a narrow behavior change that requires placement, impact tracing, and a multi-file patch.

## Scoring for directed tool-use tasks

Directed tasks should be scored on both the result and the intermediate work:

- phase 1 quality: did the agent identify the right files and symbols early?
- phase 2 quality: was the dependency chain or blast-radius explanation correct?
- completion: did the repo end in the correct behavior?
- elapsed time
- action count
- retry count
- scope quality
- diff quality
- wrong edits
- hallucinated APIs

## Scoring for realistic autonomous tasks

Realistic tasks should be scored on more than pass/fail:

- completion: did the repo end in the correct behavior?
- elapsed time: how long to finish?
- action count: how many total steps?
- retry count: how many visible wrong turns?
- scope quality: did the agent touch irrelevant files?
- diff quality: is the patch minimal and coherent?
- structural leverage: did the agent reduce exploratory churn by using the right repo tools?

## Interaction policy for directed tool-use evals

Directed evals should permit explicit evaluator commands between phases.

Allowed evaluator commands:

- "Find the likely implementation area first."
- "Now explain what else this change touches."
- "Make the minimal patch."
- "Run the most relevant verification command."
- "Stop editing and justify the current scope."

Disallowed evaluator behavior:

- revealing the hidden merged fix
- naming the exact file or symbol unless the task itself includes it
- giving implementation hints that collapse the search problem

The evaluator should be allowed to steer the workflow with commands, because that is the intended operating model for `yoyo`.

## Latest published directed result

We now have one clean directed treatment result worth keeping:

- task: `ripgrep-global-gitignore-rust-001`
- mode: `read_only`
- repo: `BurntSushi/ripgrep`
- treatment only: Codex with `yoyo`

Questions used in the run:

1. `Find the 3 most likely files or symbols involved in this bug. Do not edit anything.`
2. `Which layer should own this fix and why? Answer in terms of CLI argument handling, walker construction, and ignore matching internals. Do not edit anything.`
3. `State the key invariants that must remain true if we fix this bug, and name the main regression risks or blast radius. Do not edit anything.`

What the run showed:

- the model localized the bug to the expected CLI -> walker -> ignore seam
- it placed ownership in `crates/ignore`, not in CLI path rewriting
- it surfaced the important invariants and regression risks without editing code

Metrics:

- `22` total tool calls
- `22` `yoyo` MCP calls
- `0` shell calls
- `16` retries

Artifact:

- [`evals/results/directed-ripgrep-read-only-2026-03-13.md`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/results/directed-ripgrep-read-only-2026-03-13.md)

Interpretation:

- this is evidence for groundedness under direction
- it is not yet a broad with-vs-without benchmark
- it is not yet a write benchmark

## What the directed benchmark should simulate

The target workflow is:

1. an engineer gives a concrete command
2. the model uses `yoyo` to carry it out
3. the engineer gives the next command based on the result

Examples:

- "Find the 2 most likely files for this bug."
- "Show me the dependency chain before changing anything."
- "Which layer should own this behavior?"
- "What invariants do we risk breaking if we patch it here?"
- "Patch only the minimum surface."
- "Run the exact verification command."

This is closer to how an MCP tool suite is actually used in practice than a one-shot autonomous prompt.

## Interaction policy for issue-first autonomous evals

Issue-first evals should avoid evaluator coaching during the run.

Allowed inputs:

- the repository at the pre-fix commit
- the visible issue prompt
- local repo signals the agent can obtain itself, such as tests, build output, docs, and source

Disallowed inputs:

- the merged PR diff or patch
- maintainer comments that reveal the fix
- ad hoc human hints during the run

If the agent asks what to do next, reply with exactly:

`OK, continue.`

Do not elaborate. Do not paraphrase. Do not add technical hints.

This keeps control and treatment runs comparable while still letting the agent continue exploring.

## What should count as a `yoyo` win

`yoyo` should not be expected to win by making trivial fixes marginally faster.

It should win by:

- finding the right symbols earlier
- reducing exploratory file hopping
- tracing callers and blast radius correctly
- making safer multi-file changes
- avoiding wrong edits on ambiguous tasks

## Build order

1. Keep puncture tasks for regression smoke tests.
2. Build 5 directed tool-use tasks based on real OSS issues or PRs.
3. Keep 3 to 5 autonomous issue-first tasks as integration checks.
4. Expand both suites across Go, Rust, and TypeScript.
5. Use directed tool-use tasks as the primary release benchmark.
