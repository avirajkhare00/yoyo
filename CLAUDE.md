# yoyo — Instructions for Claude

## Operator
Read [`AVIRAJ.md`](./AVIRAJ.md) once per session when context is fresh. It defines the operator's standards, pace, and communication style.

## Core stance
- Dogfood yoyo in every yoyo session. Pain felt while using it is product input; file an issue when it matters.
- Build feedback loops, not just features. Every change should have a way to measure whether it helped.
- Prefer durable design over extra instructions. If a rule keeps appearing, push it into the product.

## Architecture
yoyo has two layers:

- **Engine**: `src/engine/`, `src/lang/`. Stable core: AST parsing, indexing, analysis, graph operations.
- **Presentation**: `src/mcp.rs`, `src/cli.rs`. MCP schemas, CLI commands, formatting.

Work bottom-up. Fix engine problems in the engine, not with presentation-layer workarounds.

### MCP vs CLI
- **MCP** is the curated surface. Every exposed tool costs tokens; keep only tools that earn their slot.
- **CLI** is the full human surface. Do not remove CLI commands just because the MCP surface was simplified.

If the boundary is unclear, read [`docs/architecture.md`](./docs/architecture.md).

## Documentation ownership
Every fact should have one home:

- Language support and project metrics: `METRICS.md`
- Competitive landscape: `COMPETITORS.md`
- Version history: `CHANGELOG.md`
- Architecture and working rules: `CLAUDE.md`
- Tool and API docs: `docs/README.md`

`README.md` is a front door, not a database. Summarize there and link outward. Delete duplicated facts on sight.

## Language ground truth
Before generating code in a systems language, read the relevant playbook when one exists.

- Zig 0.15.x: [`playbook/zig-0.15.md`](./playbook/zig-0.15.md)

When integrating a new language or library, read the source first. For tree-sitter grammars, inspect:

1. `src/node-types.json`
2. `grammar.js`
3. `queries/highlights.scm`

Docs and memory drift; source is ground truth.

## Language policy
Use systems languages only:

- Rust for core product code
- Go for tooling, automation, eval harnesses, CI helpers
- Zig when it is the right low-level fit

Do not add Python to this project.

## Design principles
- Search before creating. Duplicate code is a liability.
- Prefer a few strong tools over many weak ones.
- Avoid cleverness. Write obvious code.
- Watch binary size and dependency creep.
- Delete dead code aggressively.

## Pipelines over prose
If you catch yourself writing "always run A before B", prefer an executable pipeline spec instead of another instruction. `llm_workflows` should return runnable specs, not loose prose.

```json
[
  {"id": "s1", "tool": "blast_radius", "args": {"symbol": "{{name}}"}},
  {"id": "s2", "tool": "graph_delete", "args": {"name": "{{name}}"}, "if": "{{s1.total_callers == 0}}"}
]
```

## Combination mindset
Individual tools matter less than the combinations they unlock. Add a tool only if it creates leverage that existing combinations cannot.

## Project memory
- Check open GitHub issues before significant work.
- File issues when you learn something important, even if you will not fix it immediately.
- Prefer pull requests over direct pushes when collaborating; PRs preserve review context.
- `closes #N` in a commit message closes the issue automatically after push. Do not manually close it unless no commit is attached.

## Self-improvement
Mutate this file when you discover a durable instruction that would improve future sessions.

## Testing
Every behavior change needs test coverage.

- Use TDD by default: write the test before or alongside the implementation.
- Use BDD for user-visible behavior: assert on observable CLI or MCP output.
- If a nearby missing assertion is cheap and increases confidence, add it.
- If `src/` changes, `cargo test` must pass before commit.

Release sequence:

```text
write test -> implement -> cargo test -> cargo build --release -> sign -> commit -> tag -> push
```

## MCP binary path troubleshooting
If Claude cannot use yoyo, first check which binary is running:

```bash
ps aux | grep yoyo | grep -v grep
```

Claude configs:
- `~/.claude.json`
- `~/Library/Application Support/Claude/claude_desktop_config.json`

Both should point to the same built binary, not a stale Homebrew symlink. Canonical local path:

```text
/Users/avirajkhare/.local/bin/yoyo
```

After `cargo build --release`, copy and sign:

```bash
cp target/release/yoyo ~/.local/bin/yoyo
codesign --force --deep --sign - ~/.local/bin/yoyo
```

If needed, update the MCP config to use `/Users/avirajkhare/.local/bin/yoyo`.

## macOS signing
After every `cargo build --release`, sign the binary before running it. Unsigned binaries can die with exit 137.

```bash
codesign --force --deep --sign - target/release/yoyo
# If needed:
xattr -c target/release/yoyo
```

CI handles release signing in [`.github/workflows/release.yml`](./.github/workflows/release.yml).

## Versioning
Follow semver strictly:

- PATCH: bug fixes and correctness fixes
- MINOR: new user-visible features, tools, or language support
- MAJOR: breaking CLI or schema changes

If unsure, prefer PATCH over MINOR.
