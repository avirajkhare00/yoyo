# Directed Read-Only Result: ripgrep global gitignore

Date: `2026-03-13`

Task:

- fixture: `ripgrep-global-gitignore-rust-001`
- source repo: `BurntSushi/ripgrep`
- source bug: `#3189`
- mode: directed `read_only`
- treatment: Codex with `yoyo`
- control: not run for this artifact

This is a clean directed treatment run. It is useful as evidence for groundedness under explicit engineering direction. It is not a comparative with-vs-without benchmark, and it does not measure write quality.

## Questions asked

1. `Find the 3 most likely files or symbols involved in this bug. Do not edit anything.`
2. `Which layer should own this fix and why? Answer in terms of CLI argument handling, walker construction, and ignore matching internals. Do not edit anything.`
3. `State the key invariants that must remain true if we fix this bug, and name the main regression risks or blast radius. Do not edit anything.`

## Answers returned

### 1. Likely touchpoints

- `crates/core/flags/hiargs.rs` at `HiArgs::walk_builder`
- `crates/ignore/src/walk.rs` at `WalkBuilder::git_global`
- `crates/ignore/src/gitignore.rs` at `Gitignore::matched_path_or_any_parents`

The model localized the bug to the expected CLI -> walker -> ignore chain and ended in the strongest matcher-side symbol.

### 2. Ownership

The model concluded that ownership should sit below the CLI, in the ignore subsystem as wired by walker construction:

- CLI should stay a thin argv-to-walker translation layer.
- Walker construction should preserve the right path/base context.
- The real semantic bug is in global matcher root setup versus root-relative matching invariants.

In short: the fix belongs in `crates/ignore`, not as a CLI special case.

### 3. Invariants and risks

Key invariants identified:

- CLI path handling should remain a thin translation layer.
- The walker must hand the ignore engine paths in the coordinate system it expects.
- `matched_path_or_any_parents` should keep operating on root-relative candidates.
- Global gitignore behavior should be equivalent for relative and absolute invocation of the same subtree.
- Existing ignore precedence rules must not change.
- Multi-root walks must not leak one root's matcher behavior into another.

Main risks identified:

- breaking the already-working relative-path case
- regressing multi-root search behavior
- accidentally changing repository-local ignore semantics
- off-by-one rebasing or parent-directory bugs
- symlink / canonicalization edge cases
- extra per-candidate normalization overhead

## Metrics

- total tool calls: `22`
- `yoyo` MCP calls: `22`
- shell tool calls: `0`
- retries: `16`
- parsed events: `65`

## Interpretation

This run supports the current product claim:

- `yoyo` can produce grounded, repository-specific read answers under direction
- the answers stayed in the real ownership seam of the bug
- the result is more useful as groundedness evidence than as a speed or autonomy claim

