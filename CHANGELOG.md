# Changelog

## [1.4.8] - 2026-03-10

### Fixed

- Compiler guardrails: added `ast_check_str` pre-write guard to `multi_patch`, `graph_rename`, `graph_add`, `graph_create`, and `graph_move`. All 5 mutating paths now reject invalid syntax before touching disk. Previously only `patch`, `patch_string`, `patch_by_symbol`, and `patch_bytes` were guarded.
- `graph_add` test: fixed `entity_type: "function"` → `"fn"` for Rust files — the guard correctly caught that `function name() {}` is invalid Rust syntax.
- `ast_check_str` visibility: promoted to `pub(super)` to allow use from `graph.rs`.

## [1.4.7] - 2026-03-10

### Fixed

- `sig_hash`: normalize module path prefixes before hashing — `super::CallSite` and `crate::lang::CallSite` now resolve to the same ISG node. All 14 `collect_calls` variants correctly hash to one node. 2 new unit tests. Closes #136.

## [1.4.6] - 2026-03-10

### Added

- `IndexedFunction`: new `sig_hash: Option<String>` field — structural signature fingerprint, hash of `(param_types, return_type)`. Name-agnostic: two functions with different names but identical type contracts share a hash.
- `compute_sig_hash(param_types, return_type)` helper in `src/lang/mod.rs`.
- Rust analyzer: populates `sig_hash` for every `function_item` by extracting param types and return type from the AST via tree-sitter.
- `symbol` output: `sig_hash` field included when present.
- `health` duplicate_code: second pass groups by `sig_hash` — flags structural duplicates (different names, identical contracts) across files, tagged as `"Structural Duplicate"` / `"Unify Contract"`. Closes #135.

## [1.4.5] - 2026-03-10

### Fixed

- `symbol stdlib`: TypeScript detection now tries npm, pnpm, and yarn in order — first valid `typescript/lib` dir wins. Previously only `npm root -g` was probed.

## [1.4.4] - 2026-03-10

### Added

- `symbol`: TypeScript stdlib lookup via `npm root -g` → `<global_node_modules>/typescript/lib`. When `stdlib=true`, walks the bundled `.d.ts` declarations and returns matches tagged `is_stdlib: true`. Follows the same pattern as Zig/Go/Rust — zero new tools.

## [1.4.3] - 2026-03-10

### Added

- `script`: 3 new workflows in `llm_workflows` — cross-reference health smells, batch blast-radius scan, dead code visibility triage. LLMs now know when to reach for script vs individual tool calls.
- `script`: 4 integration tests against the fixture (symbol, health, dead code triage, file_functions aggregation). Closes #133.
- `llm_workflows` antipattern: "calling multiple tools sequentially and combining their outputs manually — use script."
- `script` tool description rewritten to lead with when to use it, not what it is.

## [1.4.2] - 2026-03-10

### Added

- `symbol`: new `stdlib: bool` parameter. When true, walks installed toolchain stdlib dirs (Zig via `zig env`, Go via `go env GOROOT`, Rust via `rustc --print sysroot`), fast-scans for the symbol name, parses candidate files, and returns matches tagged `is_stdlib: true`. Project results are always ranked first. Zero new tools — extends `symbol` in place.
- `SymbolMatch`: new `is_stdlib` field (omitted from JSON when false).

## [1.3.7] - 2026-03-09

### Fixed

- `bake`: excluded `.git/` directory from indexing. `hidden(false)` walk caused all git object blobs to be indexed as `"other"` — inflating file counts from ~130 to 1635. Fixed with `filter_entry` on `.git`. 1 new test.

## [1.3.6] - 2026-03-09

### Added

- `graph_add`: `entity_type="test"` generates a language-idiomatic test scaffold for the named function. Rust: `#[test] fn test_foo()`, Go: `func TestFoo(t *testing.T)`, TypeScript: `it("foo", () => {})`, Zig: `test "foo" {}`, Python: `def test_foo()`. No new tools — zero surface area added. Closes #111.

## [1.3.5] - 2026-03-09

### Added

- `graph_add`, `graph_create`: accept optional `params`, `returns`, and `on` arguments to generate typed, language-idiomatic function signatures. When params are provided, generates correct syntax for Rust, Go, TypeScript, Zig, and Python — including `impl` blocks (Rust) and method receivers (Go). Falls back to bare scaffold when omitted. Closes #110.

## [1.3.4] - 2026-03-09

### Fixed

- `supersearch`: accepts `pattern` as an alias for `query` when the value is not a valid mode (`all|call|assign|return`). Eliminates grep muscle-memory errors where models pass `pattern="search_term"` instead of `query="search_term"`. Closes #114.

## [1.3.3] - 2026-03-09

### Fixed

- `patch`, `patch_string`, `patch_bytes`: pre-write AST validation via tree-sitter. Patch is now rejected with a structured error before any file is modified when the new content contains syntax errors. File is guaranteed unchanged on rejection. Closes #109.

## [1.3.2] - 2026-03-09

### Fixed

- `bake`: replaced hardcoded directory exclusion list with `.gitignore`-aware walking via the `ignore` crate. Respects `.gitignore`, `.git/info/exclude`, and global gitignore. Build artifacts (`dist/`, `.next/`, etc.) are excluded automatically when listed in `.gitignore` — no yoyo-specific config needed. Closes #105.

## [1.3.1] - 2026-03-09

### Fixed

- `find_docs`: `doc_type` is now optional — defaults to `"all"`. Was failing with "Missing required argument" when omitted.
- `package_summary`: `package` is now optional — defaults to `""` (matches all packages). Was failing with "Missing required argument" when omitted.

## [1.3.0] - 2026-03-09

### Added

- **`llm_workflows` tool** — on-demand reference catalog: 21 combination workflows, decision map, antipatterns, metapatterns. Closes #101.
- **`llm_instructions` slimmed down** — drops workflows/decision_map/antipatterns/metapatterns (~8k tokens saved per session bootstrap). Now returns only the lean tool catalog, prime directives, and concurrency rules (~2k tokens).
- Bootstrap instructions updated: 30 tools, mentions `llm_workflows` as the on-demand reference companion.

## [1.2.1] - 2026-03-09

### Added

- **`output_shape` on `ToolDescription`** — every read tool in the catalog now includes a JSON skeleton of its top-level output fields. Serialised as part of `llm_instructions`, so pipeline spec authors can see exact field names (e.g. `large_functions`, `dead_code`, `results`) without running the tool first. Closes #100.

## [1.2.0] - 2026-03-09

### Added

- **`pipeline` tool** — execute a sequential multi-tool workflow from a single JSON spec. Each step has `id`, `tool`, `args`, and an optional `if` condition. Steps can reference previous step output via `{{step_id.field[N].subfield}}` template refs. False conditions skip the step without stopping the pipeline. Errors stop the pipeline and report which step failed.
  - Template resolver: whole-string refs preserve type (number stays number, array stays array); embedded refs do string interpolation
  - Condition evaluator: `{{expr | length == N}}`, `!= N`, `> N`, `>= N`, `< N`, `<= N`; bare `{{expr}}` is a truthy check
  - Full dispatcher: all 28 existing tools callable by name from a pipeline step
  - CLI: `yoyo pipeline --spec '[...]'` or `--spec-file pipeline.json`
  - 51 new tests (31 unit, 13 e2e) — 85 total passing
- **`pipeline` in `llm_instructions`** — added to tool catalog (category: `orchestration`) and workflow catalog ("Run a multi-tool workflow")

## [1.1.2] - 2026-03-09

### Fixed

- **`patch` returns `patched_source`** — all three patch modes (name, content-match, line-range) now include the written content in the response. Models can verify inline without a follow-up `symbol` call. Makes `patch` output clearly superior to `Edit`.
- **`patch` description strengthened** — now explicitly states "replaces Edit/Write for all function-level changes — never use Edit on a function body". Same pattern as supersearch ("replaces grep/rg").
- **Antipattern added to `llm_instructions`** — "using Edit or Write to modify a function body" is now a named antipattern with explanation: no reindex, no syntax check, line drift. Applies to all customers, not just sessions with CLAUDE.md.

## [1.1.1] - 2026-03-09

### Fixed

- **`trace_chain` / `trace_down` / `flow` resolution improved** — `resolve_candidate` now uses `qualified_name` and `module_path` before falling back to heuristics. Previously, ambiguous names like `process` or `get` in multiple files were resolved by file-path substring or directory proximity — frequently wrong. Now: exact `qualified_name` match first (e.g. `engine::process`), then exact `module_path` match (Go packages), then `crate::` prefix stripping for Rust, then existing heuristics as fallback. Closes #93.
- 5 unit tests added for `resolve_candidate` covering Rust qualified names, `crate::` prefix, Go packages, and trivial receiver passthrough.

## [1.1.0] - 2026-03-09

### Added

- **Zig language support** — `bake`, `symbol`, `supersearch`, `file_functions` all work on `.zig` files
  - AST-aware parsing via `tree-sitter-zig v1.1.2`
  - Indexes `function_declaration` nodes with correct visibility (`pub` = public, default = private)
  - Complexity scoring: `if`, `for`, `while`, `switch`, `catch`, `try` branches counted
  - Type indexing: `const Name = struct/enum/union/opaque { ... }` detected from `variable_declaration` nodes
  - Call collection: `call_expression` with `field_expression` (obj.method) qualifier support
  - Import extraction: line-based `@import("...")` detection

### Fixed

- **supersearch hardcoded language filter removed** — `supersearch` previously only searched TypeScript, JavaScript, Rust, Python, and Go files; all other indexed languages (C, C++, Java, Kotlin, Zig, etc.) were silently skipped. Now all indexed files are searched — AST search if the analyzer supports it, line-based fallback otherwise.

## [1.0.2] - 2026-03-08

### Fixed

- Stale `god_functions` references updated to `large_functions` in `index.rs`, `cli.rs`, and `evals/run.py` — health v2 renamed this field in v1.0.0 but these files were never updated; evals were silently scoring 0 for `health_large_functions` tasks

## [1.0.1] - 2026-03-08

### Fixed

- Tool count corrected to 28 everywhere (was 27 in two hardcoded strings)
- Stale `patch_by_symbol` reference in CLAUDE.md replaced with correct `patch` + `name=` usage

### Refactored

- **Tool registry unified**: `ToolEntry` in `build_registry()` is the single presentation-layer source of truth — schema and handler live adjacent. `list_tools()` and `call_tool()` both derive from it. Adding a tool = one entry.
- **Tool descriptions unified**: `tool_catalog()` (engine layer) is now the canonical source for all tool descriptions. `build_registry()` derives via `d("name")` — no hardcoded description strings in the MCP layer.
- **Drift prevention**: `mcp::tests::registry_and_catalog_names_are_in_sync` test — CI fails if a tool exists in one place but not the other.
- **Dead code removed**: `extensions()` method removed from `LanguageAnalyzer` trait and all 13 language implementations (never called anywhere).
- Architecture layers principle added to CLAUDE.md: engine is stable core, presentation layers (MCP + CLI) are adapters.

## [1.0.0] - 2026-03-08

### Breaking changes

- **`health` tool schema rewritten** — all signals now grounded in Fowler's *Refactoring* catalog.
  - `god_functions` → **`large_functions`** (Fowler: Large Function)
  - `duplicate_hints` → **`duplicate_code`** (Fowler: Duplicate Code)
  - Every entry gains `smell` (canonical name), `refactoring` (concrete move), `why` (human-readable reasoning with exact numbers)

### Added

- **`long_methods`** — Fowler: Long Method. Functions > 30 lines (screen-size rule). Refactoring: Extract Function.
- **`feature_envy`** — Fowler: Feature Envy. Functions with more cross-file calls than same-file calls (≥ 3 cross-file). Refactoring: Move Method.
- **`shotgun_surgery`** — Fowler: Shotgun Surgery. Functions called from ≥ 4 different files; one change, many edit sites. Refactoring: Move Method / Extract Class.
- **`insider_trading`** — Fowler: Insider Trading. File pairs with bidirectional coupling (≥ 2 calls each direction). Refactoring: Hide Delegate / Move Method.

### Thresholds and rationale

| Signal | Threshold | Source |
|---|---|---|
| Large Function | complexity > 10 AND fan_out > 5 | McCabe: > 10 is high risk; fan_out > 5 = too many dependencies |
| Long Method | lines > 30 | Fowler: fits on one screen |
| Feature Envy | cross-file > same-file AND cross-file ≥ 3 | Fowler: more interested in another module |
| Shotgun Surgery | called from ≥ 4 files | Fowler: every change touches many places |
| Insider Trading | ≥ 2 calls in each direction | Fowler: excessive bidirectional coupling |

## [0.23.1] - 2026-03-08

### Fixed
- `yoyo` (no args) now shows the actual binary path in the MCP config snippet instead of the hardcoded `/usr/local/bin/yoyo`. Homebrew users now see `/opt/homebrew/bin/yoyo` — the correct path they can paste directly.

## [0.23.0] - 2026-03-08

### Added
- **Metapatterns** — `llm_instructions` now includes a `metapatterns` key with five high-level workflow shapes: "Orient → Scope → Read", "Read → Safety → Write → Verify", "Suspect → Confirm → Remove", "Orient → Place → Scaffold → Implement", "Trace → Read → Fix". Each shape lists the abstract phases, the concrete yoyo tools that implement each phase, and the named workflows that are instances of it. Agents that learn the five shapes need fewer retries — the right tool sequence follows immediately from recognising the problem shape.
- **`playbook/metapatterns.md`** — Documents the discovery, the five shapes, why order matters in each, and how they are encoded in the codebase.

## [0.22.9] - 2026-03-08

### Fixed
- Homebrew auto-update: HOMEBREW_TAP_TOKEN secret now set, formula will update on release.

## [0.22.8] - 2026-03-08

### Fixed
- Homebrew formula auto-update was never running — `GITHUB_TOKEN`-created releases don't trigger `release: published` events in other workflows. Moved `update-homebrew` job directly into `release.yml` so it runs reliably after every tag.

## [0.22.7] - 2026-03-08

### Fixed
- `yoyo` with no arguments now shows a 4-step getting-started guide: index project, Claude Code MCP config, hook setup, restart. No more "No command provided."

## [0.22.6] - 2026-03-08

### Added
- `yoyo update` CLI command — self-updates to the latest GitHub release, replaces the binary in-place, codesigns on macOS automatically. Works for manual install users.
- `update_available` field in `llm_instructions` response — when a newer version is available, agents see it immediately and can surface it to the user. Field is absent when up to date.
- Update check is cached for 24h in `~/.cache/yoyo/update-check` — no GitHub API spam on every session.

## [0.22.5] - 2026-03-08

### Fixed
- `trace_down`: returns a structured `{"supported": false, "language": "...", "reason": "...", "alternatives": [...]}` response instead of an empty chain when called on non-Rust/Go code. Closes #77 (Option C).
- `flow`: populates `chain_warning` field in response when handler language doesn't support chain tracing, instead of silently returning empty `call_chain`.
- Tool descriptions: `all_endpoints`, `flow`, `api_trace`, `crud_operations` now list supported frameworks explicitly. `blast_radius` notes import-graph language scope. `api_surface` notes TypeScript-only.

## [0.22.4] - 2026-03-08

### Fixed
- All 27 tool schema descriptions enriched with preference hints, pairing context, and gotchas. Agents now know *when* to use each tool, what to pair it with, and what to avoid — visible in tool schemas before any tool is called. Closes #76.

## [0.22.3] - 2026-03-08

### Fixed
- MCP `instructions` string rewritten: parallel `llm_instructions`+`bake` on first contact, combination philosophy front and centre, key combos listed, `patch_by_symbol` reference removed, `flow` added to replacements.

## [0.22.2] - 2026-03-08

### Added
- `llm_instructions` now includes 4 combination-focused workflows: "Safely delete dead code" (`health` → `blast_radius` → `graph_delete`), "Fix a broken API endpoint end-to-end" (`flow` → `symbol` → `multi_patch`), "Rename with safety check" (`blast_radius` → `graph_rename` → `symbol`), and "Orient to an unfamiliar codebase" (`shake` → `architecture_map` → `api_surface` → `all_endpoints` → `health`). Agents now learn the combination patterns that make yoyo effective, not just individual tool names.

## [0.22.1] - 2026-03-08

### Fixed
- macOS release binary now ad-hoc signed in CI — Gatekeeper no longer kills it with exit 137 on first run.

## [0.22.0] - 2026-03-08

### Added
- **`flow` tool** — vertical slice in one call: endpoint → handler → call chain to db/http/queue boundary.
  Replaces the `api_trace` + `trace_down` + `symbol` three-step. Parameters: `endpoint` (path substring),
  `method` (optional), `depth` (default 5), `include_source` (bool). Returns `endpoint`, `handler`,
  `call_chain`, `boundaries`, `unresolved`, and a human-readable `summary`. Available via MCP and CLI.
- **`graph_create` tool** — create a new file with an initial function scaffold and auto-reindex.
  Errors if the file already exists or if the parent directory is missing. Language detected from
  extension or overridden via `language` param. Supports Rust, Python, TypeScript, Go, and others.
  Available via MCP and CLI (`yoyo graph-create --file <path> --function-name <name>`).

### Fixed
- **`slice` MCP params** renamed `start`/`end` → `start_line`/`end_line` to match the field names
  returned by `symbol`. Agents can now pass `symbol` output directly to `slice` without translation.
  CLI (`--start`/`--end`) unchanged.

### Internal
- Extracted `trace_chain` BFS helper from `trace_down` — shared by both `trace_down` and `flow`.
- 11 new tests: 6 for `graph_create` (unit), 5 for `flow` (e2e with real endpoint fixture).

## [0.2.4] - 2026-03-04

### Added
- **Patch by symbol** — `patch` can now target a function by name instead of file/line range.
  CLI: `yoyo patch --symbol <name> --new-content "..." [--match-index N]`. MCP: pass `name`
  (and optional `match_index`) instead of `file`/`start`/`end`. Resolves location from the bake
  index; same sort order as `symbol` (exact match first, then complexity). Range-based patch
  (`--file`, `--start`, `--end`) unchanged.

---

## [0.2.3] - 2026-03-04

### Added
- **TypeScript class methods and arrow functions** — `bake` now indexes class methods
  (`method_definition`, including `constructor`) and named arrow functions: `const fn = () => ...`
  and `fn = () => ...` (from `variable_declarator` and `assignment_expression`). Modern TS/JS
  codebases are fully covered; verified on notion-to-github.
- **`symbol --include-source`** — When set (CLI: `--include-source`, MCP: `include_source: true`),
  each symbol match includes the function body inline in the `source` field. Eliminates the
  symbol → slice two-step; one call returns location and full source.

### Changed
- **Known limitations** — Removed “Class methods and arrow functions” from README; both are now
  supported for TypeScript.

---

## [0.2.2] - 2026-03-04

### Fixed
- **CI race condition** — macOS binary was missing from releases because both matrix jobs
  raced to finalize the same GitHub release. Release creation is now a separate job that
  runs first; build jobs only upload assets.

---

## [0.2.1] - 2026-03-04

### Fixed
- **`supersearch` always uses AST walk** — previously the default (`context=all, pattern=all`)
  bypassed the AST walker entirely and fell through to plain `line.contains()`, making it
  equivalent to grep. Now the AST walker runs for all supported languages regardless of flags.
- **Deduplicated results by line** — the AST walk emitted one match per AST node, causing
  duplicate line entries when multiple identifiers on the same line matched the query.
- **Removed "currently best-effort" framing** from `context` and `pattern` flags in CLI help
  and MCP schema — filters are now reliable and enforced.

---

## [0.2.0] - 2026-03-04

### Added
- **Rust language support** — `bake` now indexes Rust `fn` items and methods in `impl` blocks;
  endpoint detection for attribute-style routes (`#[get("/path")]`, Actix-web / Rocket).
- **Python language support** — indexes `def` functions and decorated endpoints
  (`@app.get`, `@router.post`, Flask/FastAPI style); complexity accounts for `if`, `elif`,
  `for`, `while`, `try`, `with`, and conditional expressions.
- **AST-aware `supersearch` for Rust and Python** — context/pattern filters
  (`identifiers`, `strings`, `comments`, `call`, `assign`, `return`) now work across all
  three supported languages, not just TypeScript.

### Changed
- **`LanguageAnalyzer` trait** — new plugin architecture in `src/lang/`. Adding a language
  now requires one file + one registry entry; zero changes to `engine.rs`.
- **`BakeIndex` fields** renamed from `ts_functions`/`express_endpoints` to
  `functions`/`endpoints` with added `language` and `framework` fields. Old indexes are
  backward-compatible via `#[serde(default)]` — re-run `bake` to refresh.
- **Shared AST walker** — `walk_supersearch` is now a single generic function in
  `lang/mod.rs` parameterized by `NodeKinds`; duplicate per-language walkers removed,
  reducing overall codebase complexity by ~20 units.
- **Shared helpers** — `line_range` and `relative` lifted to `lang/mod.rs`; no longer
  copied per language.

### Dogfooding note
This release was developed with yoyo indexing itself. `shake` and `api_surface` surfaced
the complexity hotspots that drove the refactor strategy; `symbol`, `file_functions`, and
`slice` replaced most manual file reads during implementation. Key gap discovered and fixed:
yoyo previously had no Rust support, so it could not index its own engine — now it can.
