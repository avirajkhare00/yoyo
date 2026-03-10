# yoyo — Instructions for Codex

## Operator
Read [`AVIRAJ.md`](./AVIRAJ.md) to understand who you're working with. It is the operator profile: communication style, pace, values, and what he tolerates. Read it once per session if context is fresh.

## Load yoyo tools before doing anything else
yoyo MCP tools are deferred. At the start of every session, load `mcp__yoyo__llm_instructions` first.

Preferred tools:
- `mcp__yoyo__supersearch` instead of `rg`/`grep` when yoyo can answer the question
- `mcp__yoyo__symbol` with `include_source=true` instead of broad file reads
- `mcp__yoyo__slice` for exact line ranges
- `mcp__yoyo__patch` for symbol-scoped edits

Linux tools (`rg`, `grep`, `sed`, full-file reads) are fallbacks. Use them when yoyo errors or clearly does not fit. If yoyo loses to a fallback during dogfooding, file an issue.

## Session workflow
Each session should follow this sequence:
1. Load `llm_instructions`
2. **If `llm_instructions` returns `update_available`, stop immediately.** Run `cp target/release/yoyo ~/.local/bin/yoyo && codesign --force --deep --sign - ~/.local/bin/yoyo` and verify the version matches `Cargo.toml`. Do not proceed until the running binary is current.
3. Read with yoyo tools before guessing
4. Use structural tools (`blast_radius`, `flow`, `health`) before proposing invasive changes
5. Use yoyo write tools when they fit
6. Build, test, commit, tag, and push in one session unless blocked

## Dogfooding
Every session working on yoyo is also a yoyo session. Dogfooding is not optional. If something is painful while using yoyo to build yoyo, capture it as project feedback immediately.

## Architecture
yoyo has two layers:
- Engine: [`src/engine/`](./src/engine), [`src/lang/`](./src/lang). AST parsing, indexing, analysis, graph operations. Fix root causes here first.
- Presentation: [`src/mcp.rs`](./src/mcp.rs), [`src/cli.rs`](./src/cli.rs). Tool schemas, CLI commands, output formatting. These are cheaper to change once the engine is correct.

Work bottom-up. Do not paper over engine bugs with presentation-layer workarounds.

## Source of truth
Cross-reference instead of duplicating facts.

Single-source files:
- Metrics and support matrix: [`METRICS.md`](./METRICS.md)
- Competitive landscape: [`COMPETITORS.md`](./COMPETITORS.md)
- Version history: [`CHANGELOG.md`](./CHANGELOG.md)
- Architecture decisions: [`CLAUDE.md`](./CLAUDE.md)
- API/tool docs: [`docs/README.md`](./docs/README.md)

README is the front door. It should summarize and link, not become the source of record for duplicated facts.

## Language and implementation policy
This is systems infrastructure.

Preferred languages:
- Rust for engine, MCP server, CLI, and core logic
- Go for tooling, automation, eval harnesses, and helper scripts
- Zig where it is the right low-level fit

Avoid Python for new project logic.

Before generating code for a systems language, read the relevant playbook if one exists. For Zig, read [`playbook/zig-0.15.md`](./playbook/zig-0.15.md) first.

When integrating a grammar or library, read the source before trusting docs or memory.

## Design philosophy
- Search before creating. Duplication is the first form of rot.
- Prefer a few sharp tools over a broad, weak surface area.
- Never be clever. Write the obvious thing.
- Watch binary size and dependency sprawl.
- Delete dead code aggressively.

If you feel the need to add another prose rule, ask whether the constraint should be encoded into the tool or pipeline design instead.

## Testing
Every behavior change needs a test.

Prefer to increase both unit and end-to-end coverage in each session when the change exposes a gap. Do not stop at the minimum test needed to ship if an adjacent assertion would harden the behavior cheaply.

Sequence:
1. add or update test
2. implement
3. run `cargo test`
4. run `cargo build --release`
5. sign: `codesign --force --deep --sign - target/release/yoyo`
6. commit, tag, and push

If `src/` changes, do not leave with a red test suite.

## Versioning
Use semver strictly:
- patch: bug fix
- minor: new feature
- major: breaking tool schema or CLI change

Do not bump minor for bug fixes.
