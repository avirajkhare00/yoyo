# Changelog

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
