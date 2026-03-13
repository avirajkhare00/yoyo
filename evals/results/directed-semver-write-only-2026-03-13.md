# Directed Write-Only Note: semver

Date: 2026-03-13

Task:
- repo: `dtolnay/semver`
- fixture: `semver-rust-001`
- mode: `write_only`
- runner: `treatment` with `yoyo`

Result:
- patch completed cleanly in `src/eval.rs`
- manual verification passed:
  - `cargo test`
- runner step 1 also reported:
  - `cargo test --tests` passed
- no tracked changes remained outside `src/eval.rs`

Why this is separate:
- the fixture's exact task command is malformed for shell execution:
  - `cargo test --test *`
- shell expansion turns `*` into repo filenames before Cargo sees it
- that makes strict automated scoring invalid even though the patch itself is correct

Metrics:
- total tool calls: `11`
- `yoyo` MCP tool calls: `8`
- shell tool calls: `3`
- retries: `4`

Patch summary:
- restored the major-version comparison in `matches_less`
- relaxed the prerelease comparisons in `matches_tilde` and `matches_caret`

Status:
- useful evidence for write correctness on the known surface
- not part of the clean publishable write batch until the fixture verify command is fixed
