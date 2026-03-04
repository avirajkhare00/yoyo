# Changelog

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
