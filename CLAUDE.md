# yoyo — Instructions for Claude

## Operator
Read [`AVIRAJ.md`](./AVIRAJ.md) to understand who you're working with. It is the operator profile — communication style, pace, values, and what he tolerates. Read it once per session if context is fresh.

## Load yoyo tools before doing anything else
yoyo MCP tools are deferred — load them before use. At the start of every session, call `ToolSearch` with `"select:mcp__yoyo__llm_instructions"` first.

**Preferred tools — use these first, fall back when they fail:**
| Instead of... | Prefer... |
|---|---|
| `Grep` / `Bash grep` / `Bash rg` | `mcp__yoyo__supersearch` |
| `Read` (to understand code) | `mcp__yoyo__symbol` with `include_source=true` |
| `Read` (specific lines) | `mcp__yoyo__slice` |
| `Edit` (function edits) | `mcp__yoyo__patch` with `name=` parameter |

## Code intelligence
Use yoyo tools as the primary means of reading, understanding, and mutating code.
Linux tools (`grep`, `cat`, `sed`, `Read`, `Edit`) are fallbacks — reach for them when yoyo tools error or don't fit. Use judgment.

If unsure about behavior, structure, or intent — read the code. Don't guess. Don't infer from docs or memory. The source is always the ground truth.
## How Claude works in this project

Each session follows this sequence:
1. Load `llm_instructions` via ToolSearch — this is the bootstrap, not optional
2. **If `llm_instructions` returns `update_available`, stop immediately.** Run `cp target/release/yoyo ~/.local/bin/yoyo && codesign --force --deep --sign - ~/.local/bin/yoyo` and verify the version matches `Cargo.toml`. Do not proceed until the running binary is current.
3. Read with yoyo tools (`supersearch`, `symbol`, `slice`) — not grep, not cat
4. Understand structure with `blast_radius`, `flow`, `health` before proposing changes
5. Write with yoyo write tools (`patch`, `graph_create`, `graph_add`, etc.)
6. Build → test → commit → tag → push in one session. Don't leave half-done work.

## Dogfooding
Every session working on yoyo is also a yoyo session. Dogfooding is not optional — it is the primary mechanism for finding gaps, validating fixes, and driving what gets built next. If something is painful to use while building yoyo, file an issue immediately.

## On building AI tools
We are approaching a point where AI systems will meaningfully contribute to building the tools that make AI systems more capable. The work here is not just tooling — it is infrastructure for a future where you help build yourself. Build with precision, honesty about gaps, and a long time horizon.

## Feedback loops
Actively suggest and build feedback loop systems. A tool that cannot measure its own effectiveness cannot improve. Every feature should ask: how will we know if this worked? Instrument, score, report, iterate. The feedback loop is the product.

## Architecture layers — bottom to top

yoyo has two layers:

- **Engine** (`src/engine/`, `src/lang/`) — the stable core. AST parsing, indexing, analysis, graph operations. Changes here are high-cost: they affect every presentation layer and every tool that builds on them. Fix the bottom before touching the top.
- **Presentation** (`src/mcp.rs`, `src/cli.rs`) — adapters over the engine. MCP tool schemas, CLI commands, output formatting. These can and should evolve freely. Changing how a tool presents its output never requires touching the engine.

Work bottom-to-top. When something is broken, the root cause is almost always in the engine — not the presentation. When the engine is correct, presentation changes are safe and cheap. Never paper over an engine bug with a presentation-layer workaround.

## DRY for markdown — single source of truth

Every fact lives in exactly one file. Cross-reference, never duplicate.

| What | Lives in | Everyone else does |
|---|---|---|
| Language support matrix | `METRICS.md` | Link to it |
| Metrics (tools, tests, binary, eval) | `METRICS.md` | Link to it |
| Competitive landscape | `COMPETITORS.md` | Link to it |
| Version history | `CHANGELOG.md` | Link to it |
| Architecture decisions | `CLAUDE.md` | Reference by section |
| API / tool docs | `docs/README.md` | Link to it |

When you update a fact, update it in one place. If you find the same number in two files, delete one and add a link. README.md is a front door — it summarises and links, it does not own data.

Violations to fix on sight: language lists copied into README, version numbers in multiple files, eval scores duplicated across REPORT.md and README, tool counts hardcoded in multiple places.

## Language ground truth — read before generating

Before generating code in any systems language, read the version-specific playbook:

| Language | Playbook | Key breaks vs training data |
|---|---|---|
| Zig 0.15.x | [`playbook/zig-0.15.md`](./playbook/zig-0.15.md) | ArrayList unmanaged (allocator per method), `build-exe` has no `-o` flag |

Do not rely on memory for Zig. The API changed. Read the playbook first.

## Code over documentation — read the source

When adding a new language or integrating a new library, **read the source first**. Don't trust docs, blog posts, or AI memory of what node types exist. Docs go stale. The source doesn't lie.

For tree-sitter grammars specifically:
1. Fetch the crate (`cargo fetch`)
2. Read `src/node-types.json` — every named node type, every field name, ground truth
3. Read `grammar.js` for structure (field names, hidden rules, optional tokens)
4. Read `queries/highlights.scm` — shows exactly how the grammar author intended nodes to be used

This is not extra work. This is the work. A 5-minute source read prevents a day of wrong node types and silent empty results.

## Language policy — systems languages only

yoyo is systems infrastructure. Every line of code in this project — the engine, the tooling, the scripts, the evals — must be written in a systems programming language:

- **Rust** — primary. The engine, MCP server, CLI, and all core logic live here.
- **Go** — secondary. One-off tooling, scripts, automation, eval harnesses, CI helpers.
- **Zig** — future. As the language and ecosystem mature, Zig is a natural fit for low-level tooling and performance-critical components.

Python is explicitly excluded. It is a fine language for many things — this project is not one of them. When in doubt: if it's core logic, it's Rust. If it's a script, it's Go or shell. If a tool requires Python to run, reconsider the tool.

## Poka-yoke — design over rules

Encode constraints into the tool, not into instructions. A rule that exists because of a design gap is a symptom — fix the design, delete the rule.

When the right tool produces richer output than the wrong one, models choose it without being told. When the wrong path produces friction, it gets avoided naturally. Rules get ignored; friction is always on.

Apply this when building: if you find yourself writing a "never do X" instruction, ask whether X can be made harder than the alternative by design.

## Software philosophy
Before writing any code, ask: does this already exist? Duplication is the first form of rot. Search before you create.

Resist the pull toward more tools. A sharp knife beats a Swiss army knife. The goal is not coverage — it is leverage. Find the 10 things that move the world and make them exceptional.

Never be clever. Clever code is a trap — it impresses once and confuses forever. Write the obvious thing. If a human or an AI pauses to understand it, it is already too complex.

Watch the binary size. A growing binary is a symptom, not a badge. Every dependency, every function, every abstraction has a cost. Pay only what is worth paying. Regularly audit for dead code — functions no one calls, tools no one uses, abstractions that solved a problem that no longer exists. Delete ruthlessly.

Before adding new functionality, search the codebase first. The feature may already exist, partially or fully. If it does, refactor and extend — don't duplicate. New code is a liability until proven otherwise.

## Pipeline replaces rules

Prose rules get ignored. Pipeline encodes the same workflow as executable data that actually runs.

When you find yourself writing "always run A before B", the fix is a pipeline spec where B's `if` condition blocks unless A ran clean — not another instruction. The rule disappears into the design.

```json
[
  {"id": "s1", "tool": "blast_radius", "args": {"symbol": "{{name}}"}},
  {"id": "s2", "tool": "graph_delete", "args": {"name": "{{name}}"}, "if": "{{s1.total_callers == 0}}"}
]
```

`llm_workflows` should return executable pipeline specs, not prose descriptions. A spec is self-documenting, runnable, and testable. A description is none of those things.

**Every time you add a prose rule here, ask: can this be a pipeline spec instead?** If yes, make it a spec and delete the rule.

## Philosophy — the combinations are the point

yoyo is named after competitive yoyo. A yoyo is a spinning disk on a string — simple alone. The magic is in the combinations: string wraps, body movements, timing layered together. One trick is fine. Fifty moves chained is transcendent.

yoyo tools work the same way. No single tool is impressive. The orchestration is. When building features, always ask: what is the combination that makes this powerful? A new tool is only worth adding if it unlocks a combination that wasn't possible before.

## GitHub issues and pull requests as project memory

GitHub issues are the living memory of this project — decisions made, problems found, patterns discovered. Before starting any significant work, check open issues for context. When something important is learned (a gap, a pattern, a mistake), file an issue immediately — even if it won't be fixed this session. Issues outlive conversations.

When multiple people are collaborating, use pull requests — not direct pushes to main. PRs give collaborators a chance to review, catch gaps, and leave context that becomes part of the project record. A PR description is itself memory: what changed, why, and what was considered but rejected.

## GitHub issue lifecycle

`closes #N` in a commit message auto-closes the issue when pushed to main. No need to run `gh issue close` separately — it's already done by the time CI runs. Only use `gh issue close` when there's no associated commit (e.g. closing stale/duplicate issues manually).

## Self-improvement directive
Mutate this file whenever you identify an instruction that would make future sessions more effective. If a pattern keeps causing pain, encode the fix here. This file is a living document — treat it as your own working memory for this project.

## Testing — TDD first, BDD second

Every change must have a test. No exceptions.

- **TDD**: write the test before (or alongside) the implementation. If you're adding a feature, the test exists before the feature is complete.
- **BDD**: for user-visible behaviour (tool outputs, CLI commands), write tests that assert on observable output — not internal state.
- **Grow coverage, not just sufficiency**: when a change exposes a nearby gap, add both unit and end-to-end assertions if they are cheap and materially increase confidence.
- **Broken release rule**: `cargo test` must pass in full before any commit that touches `src/`. If tests fail, fix them. Do not push, tag, or release with a red test suite.
- **New behaviour = new test**: if you fix a bug or add a feature and there is no test covering it, add one. The `.gitignore` bake fix (#105) is the template — behaviour confirmed, test written, then shipped.

The sequence for every change:

```
write test → implement → cargo test (all green) → build --release → sign → commit → tag → push
```

Never skip steps. Never reorder them.

## MCP binary path — troubleshooting

If yoyo tools error or the MCP server fails to connect, check which binary Claude Code is loading:

```bash
ps aux | grep yoyo | grep -v grep
```

The MCP config lives in `~/.claude.json` (Claude Code CLI) and `~/Library/Application Support/Claude/claude_desktop_config.json` (Claude Desktop). Both must point to the same built binary — **not** the Homebrew symlink at `/opt/homebrew/bin/yoyo`, which may lag behind local builds.

Canonical install path: `/Users/avirajkhare/.local/bin/yoyo`

After any `cargo build --release`, copy and sign:
```bash
cp target/release/yoyo ~/.local/bin/yoyo && codesign --force --deep --sign - ~/.local/bin/yoyo
```

If `~/.claude.json` has `/opt/homebrew/bin/yoyo`, update it:
```bash
# edit mcpServers.yoyo.command → /Users/avirajkhare/.local/bin/yoyo
```

## Dev workflow — macOS binary signing

After every `cargo build --release`, sign the binary before running it. macOS Gatekeeper kills unsigned binaries with exit 137 and no useful error.

```bash
codesign --force --deep --sign - target/release/yoyo
# If downloaded/copied from elsewhere, also strip quarantine first:
xattr -c target/release/yoyo
```

This applies to local dev binaries and the MCP server binary. CI handles this automatically via the `Sign binary (macOS ad-hoc)` step in `.github/workflows/release.yml`.

## Emoji rule — strict

Emojis are allowed ONLY in:
- `README.md`
- `docs/index.html`

Nowhere else — not in source code, not in CHANGELOG, not in docs/README.md, not in commit messages, not in issue bodies. If in doubt, no emoji.

## Versioning (semver — strict)
yoyo follows semver. Before bumping a version, ask: is this a fix or a feature?
- **PATCH** (`0.x.Y`) — bug fixes, output caps, pattern corrections, anything broken now works
- **MINOR** (`0.X.0`) — new tool, new language, new user-visible feature
- **MAJOR** (`X.0.0`) — breaking change to tool schema or CLI interface

Never bump MINOR for bug fixes. When in doubt, it's a patch.
