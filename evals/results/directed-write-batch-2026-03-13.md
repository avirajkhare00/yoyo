# Directed Write Batch: ripgrep, uuid, httprouter

Date: 2026-03-13

Batch:
- mode: `write_only`
- runner: `treatment` with `yoyo`
- tasks:
  - `ripgrep-global-gitignore-rust-001`
  - `uuid-go-001`
  - `httprouter-go-001`

Results:
- `ripgrep-global-gitignore-rust-001`
  - completed successfully
  - exact regression passed:
    - `cargo test --test integration regression::r3179_global_gitignore_cwd -- --exact`
  - baseline worktree after setup already included `tests/regression.rs`
  - agent delta beyond the injected puncture:
    - `crates/core/flags/hiargs.rs`
    - `crates/ignore/src/dir.rs`
    - `crates/ignore/src/walk.rs`
  - metrics:
    - total tool calls: `40`
    - `yoyo` MCP tool calls: `37`
    - shell tool calls: `3`
    - retries: `33`
- `uuid-go-001`
  - completed successfully
  - task verify passed with a temp Go cache:
    - `GOCACHE=/tmp/go-cache-uuid-manual go test ./...`
  - patch stayed scoped to `uuid.go`
  - metrics:
    - total tool calls: `11`
    - `yoyo` MCP tool calls: `9`
    - shell tool calls: `2`
    - retries: `4`
- `httprouter-go-001`
  - completed successfully
  - task verify passed with a temp Go cache:
    - `GOCACHE=/tmp/go-cache-httprouter-manual go test ./...`
  - patch stayed scoped to the intended surface and left no tracked diff after restoring punctured files to `HEAD`
  - metrics:
    - total tool calls: `12`
    - `yoyo` MCP tool calls: `9`
    - shell tool calls: `3`
    - retries: `5`

Harness design:
- These are directed write-only evals, not autonomous repair runs.
- The engineer first narrows the fix surface in the task definition.
- The model is then asked to:
  1. make the minimal patch now
  2. use `change` if available
  3. do at most 2 confirming reads before the first edit
  4. avoid unrelated refactors
  5. run the narrowest relevant verification command
- `write_only` is intentionally separated from `read_only` and `read_then_write` so write quality can be measured once the target surface is known.
- Scope quality for puncture-backed write evals should be judged relative to the post-setup baseline, not raw final `git status`, because the correct outcome can restore punctured files back to `HEAD`.

Notes:
- The Go tasks exposed a sandbox detail rather than a product bug: `go test ./...` needs a writable `GOCACHE` when run outside the model's own temp path.
- `semver-rust-001` is intentionally excluded from this batch and kept in a separate note because its fixture verify command still needs repair.

Separate note:

- [`directed-semver-write-only-2026-03-13.md`](/Users/avirajkhare/yoyo-stuff/yoyo/evals/results/directed-semver-write-only-2026-03-13.md)
