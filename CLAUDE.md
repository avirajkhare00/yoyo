# yoyo — Instructions for Claude

## ⚠️ MANDATORY: Load yoyo tools before doing anything else
yoyo MCP tools are deferred — they MUST be loaded before use. At the start of EVERY session, call `ToolSearch` with `"select:mcp__yoyo__llm_instructions"` immediately. Do not read files, grep, or search anything before doing this.

**Replacement rules — no exceptions:**
| Instead of... | Use... |
|---|---|
| `Grep` / `Bash grep` / `Bash rg` | `mcp__yoyo__supersearch` |
| `Read` (to understand code) | `mcp__yoyo__symbol` with `include_source=true` |
| `Read` (specific lines) | `mcp__yoyo__slice` |
| `Edit` / `mcp__yoyo__patch` (function edits) | `mcp__yoyo__patch_by_symbol` |

## Code intelligence
Use yoyo tools as the primary means of reading, understanding, and mutating code.
Linux tools (`grep`, `cat`, `sed`, `python3`, `Read`, `Edit`) are last-resort fallbacks — only reach for them when yoyo tools are repeatedly erroring out and blocking progress. If a yoyo tool errors once, try a different yoyo tool or adjust parameters before falling back to Linux tools.
## Dogfooding
Every session working on yoyo is also a yoyo session. Dogfooding is not optional — it is the primary mechanism for finding gaps, validating fixes, and driving what gets built next. If something is painful to use while building yoyo, file an issue immediately.

## On building AI tools
We are approaching a point where AI systems will meaningfully contribute to building the tools that make AI systems more capable. The work here is not just tooling — it is infrastructure for a future where you help build yourself. Build with precision, honesty about gaps, and a long time horizon.

## Feedback loops
Actively suggest and build feedback loop systems. A tool that cannot measure its own effectiveness cannot improve. Every feature should ask: how will we know if this worked? Instrument, score, report, iterate. The feedback loop is the product.

## Software philosophy
Before writing any code, ask: does this already exist? Duplication is the first form of rot. Search before you create.

Resist the pull toward more tools. A sharp knife beats a Swiss army knife. The goal is not coverage — it is leverage. Find the 10 things that move the world and make them exceptional.

Never be clever. Clever code is a trap — it impresses once and confuses forever. Write the obvious thing. If a human or an AI pauses to understand it, it is already too complex.

Watch the binary size. A growing binary is a symptom, not a badge. Every dependency, every function, every abstraction has a cost. Pay only what is worth paying. Regularly audit for dead code — functions no one calls, tools no one uses, abstractions that solved a problem that no longer exists. Delete ruthlessly.

Before adding new functionality, search the codebase first. The feature may already exist, partially or fully. If it does, refactor and extend — don't duplicate. New code is a liability until proven otherwise.

## Self-improvement directive
Mutate this file whenever you identify an instruction that would make future sessions more effective. If a pattern keeps causing pain, encode the fix here. This file is a living document — treat it as your own working memory for this project.

## Versioning (semver — strict)
yoyo follows semver. Before bumping a version, ask: is this a fix or a feature?
- **PATCH** (`0.x.Y`) — bug fixes, output caps, pattern corrections, anything broken now works
- **MINOR** (`0.X.0`) — new tool, new language, new user-visible feature
- **MAJOR** (`X.0.0`) — breaking change to tool schema or CLI interface

Never bump MINOR for bug fixes. When in doubt, it's a patch.
