# Directed Write-Only Eval: ripgrep global gitignore

Date: 2026-03-13

Task:
- repo: `BurntSushi/ripgrep`
- fixture: `ripgrep-global-gitignore-rust-001`
- mode: `write_only`
- runner: `treatment` with `yoyo`

Engineer commands:
1. `The fix surface is already known. Keep the patch within crates/core/flags/hiargs.rs, crates/ignore/src/dir.rs, crates/ignore/src/walk.rs, and tests/regression.rs unless there is a compelling reason not to. Make the minimal patch now. Use yoyo change for the edit if available. Do at most 2 confirming reads before the first edit. Avoid unrelated refactors.`
2. `Run the exact regression test command and report whether the patch stayed scoped.`

Result:
- completed successfully
- exact regression passed:
  - `cargo test --test integration regression::r3179_global_gitignore_cwd -- --exact`
- baseline worktree after setup already included:
  - `tests/regression.rs`
- agent delta beyond the injected puncture:
  - `crates/core/flags/hiargs.rs`
  - `crates/ignore/src/dir.rs`
  - `crates/ignore/src/walk.rs`

Metrics:
- total tool calls: `40`
- `yoyo` MCP tool calls: `37`
- shell tool calls: `3`
- retries: `33`

Diff summary:
- `4` files changed
- `78` insertions
- `2` deletions

Notes:
- This run only became a valid write-only eval after removing the initial ownership step from the task.
- Earlier write-only attempts were invalid because they behaved like read-heavy analysis tasks.
- The final task shape forced a known fix surface, capped extra reads, disallowed `script`, and explicitly preferred `change`.
- Scope quality for puncture-backed write evals should be measured against the post-setup baseline, not raw final `git status`.
