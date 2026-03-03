# TODO Tracker
**Last updated:** 2026-03-04
**Sources:** README.md roadmap + live code analysis of `src/`

> Items marked ✅ were resolved in v0.2.0.

Items are tagged with source:
- `[README]` — listed in README TODO/Roadmap section
- `[CODE]` — discovered by reading source files
- `[BENCH]` — confirmed broken by live benchmark run

---

## 🔴 P0 — Breaks real usage today

### 1. ts_index.rs misses non-`function_declaration` nodes
**Source:** `[CODE]` `[BENCH]`
**File:** `src/ts_index.rs:70`
```rust
"function_declaration" => { ... }
// NOT captured:
// - method_definition (class methods)
// - arrow_function (const fn = () => ...)
// - function_expression (const fn = function() {})
// - public_method_definition
```
`file_functions` returns 0 results for any TypeScript file that uses classes or arrow functions — which is the majority of modern TypeScript. `SsdMobilenetv1.ts` (133 lines, 5 methods) returned zero functions.

**Fix:** Add cases for `method_definition`, `arrow_function`, `function_expression` in `walk_ts()`. For arrow/expression functions, use `variable_declarator` parent to infer the name.

---

### 2. `api_surface` has no pagination — overflows LLM context on large projects
**Source:** `[README]` `[BENCH]`
**File:** `src/engine.rs` — `api_surface()`

Returns all functions for all modules in one shot. Produced **50.7 KB** on face-api.js (386 files), exceeding the inline display limit.

**Fix:** Honour existing `--limit` and `--package` flags to cap output. Add a `"truncated": true` field and `total_count` when results are capped.

---

### 3. `find_docs` has no output cap
**Source:** `[README]`
**Previous benchmark result:** 298 K characters on face-api.js.

**Fix:** Limit results to 50 items max. Add `--doc-type` and `--limit` filtering at the query level, not just post-collection.

---

### 4. `architecture_map` role inference is too narrow
**Source:** `[README]` `[CODE]` `[BENCH]`
**File:** `src/engine.rs:762–776`
```rust
if path_str.contains("routes") || path_str.contains("controllers") { ... }
if path_str.contains("services") { ... }
if path_str.contains("models") || path_str.contains("entities") { ... }
```
Only 3 patterns. All 45 directories in face-api.js got `roles: []`.

**Missing keywords:** `handlers`, `repositories`, `resolvers`, `middleware`, `hooks`, `components`, `store`, `reducers`, `actions`, `selectors`, `utils`, `helpers`, `lib`, `api`, `net`, `network`, `dom`, `draw`, `ops`, `env`, `factories`, `classes`.

**Fix:** Expand keyword table. Consider using `intent` parameter to boost matching for the stated project type.

---

## 🟡 P1 — Significantly limits usefulness

### 5. `supersearch` default path bypasses AST filtering
**Source:** `[CODE]`
**File:** `src/engine.rs:512`
```rust
if lang == "typescript" && (context_norm != "all" || pattern_norm != "all") {
    ast_supersearch_typescript(...)
} else {
    // plain line-oriented text search ← default path (context=all, pattern=all)
}
```
When the user passes the defaults (`context=all, pattern=all`), the code falls through to a plain `str.contains()` loop — the AST walk is never entered. Users passing context/pattern always get AST filtering, but anyone relying on the default gets dumb line search.

Additionally, the `walk_ts_supersearch` function may report duplicate line numbers when multiple AST nodes on the same line match the query (one `push` per identifier leaf, not per line).

**Fix:** Always use AST walk for TypeScript files; apply context/pattern filtering on the same pass. Deduplicate by `(file, line)` before pushing to `matches`.

---

### 6. `supersearch` context/pattern flags described as "best-effort" but expected to work
**Source:** `[README]` `[BENCH]`
The CLI help strings and MCP schema descriptions both say "currently best-effort" — this signals to LLM consumers that the flags may be ignored. The README benchmark confirmed identical results across three filter combinations.

**Fix:** Once #5 is resolved, remove "currently best-effort" from help strings and mark the flags as reliable.

---

### 7. `symbol` does not index class definitions
**Source:** `[CODE]` `[BENCH]`
Searching `symbol(SsdMobilenetv1)` returns `allFacesSsdMobilenetv1` and `createSsdMobilenetv1` — not the class itself. Class declarations are not captured in `ts_index.rs`.

**Fix:** Add `"class_declaration"` to `walk_ts()`. Store class name + file + line range in a separate `ts_classes` array in `BakeIndex`.

---

### 8. `api_trace` and `crud_operations` limited to static Express routes
**Source:** `[README]`
Both tools return zero results on non-Express projects (CLI tools, ML libraries, NestJS apps, etc.).

**Fix (incremental):**
- Phase 1: Detect NestJS `@Get()`, `@Post()`, `@Controller()` decorators via Tree-sitter.
- Phase 2: Detect Fastify/Hono route patterns.
- Phase 3: Add DB-layer signal (Prisma `findMany/create/update/delete`) as CRUD proxy.

---

### ✅ 9. No language support beyond TypeScript/JavaScript *(resolved in v0.2.0)*
Rust and Python support added via `LanguageAnalyzer` trait. `symbol`, `file_functions`, `supersearch` now work for `.rs` and `.py` files.

---

### 10. `symbol` requires a follow-up `slice` to read the function body
**Source:** `[README]`
Every `symbol` call forces a second round-trip (`slice`) to get the actual source. For LLM use cases this doubles latency.

**Fix:** Add optional `--include-source` flag to `symbol`. When set, append a `source` field containing the full function body text.

---

## 🟢 P2 — Polish & completeness

### 11. `suggest_placement` often recommends test files
**Source:** `[BENCH]`
`suggest_placement(detectFacesFromStream, service, related_to=detectAllFaces)` returned `test/tests/globalApi/detectAllFaces.test.ts` as top result. Test files should be excluded from placement candidates by default (or heavily down-ranked).

**Fix:** Exclude `test/`, `spec/`, `__tests__/` directories from placement suggestions by default. Add `--include-tests` flag to opt in.

---

### 12. `shake` shows no top functions when run before `bake` in parallel
**Source:** `[BENCH]`
When `shake` and `bake` run in parallel (Batch 1), `shake` fires before `bake` writes `bake.json` and falls back to a lightweight filesystem scan with no function data.

**Fix:** `shake` could check for the bake index with a short retry or note in the response that baking is in progress.

---

### 13. Missing PRD tools: `related_to` and `frontend`
**Source:** `[README]`
- `related_to` — find symbols related to a given one by call graph proximity.
- `frontend` — components, hooks, props, client-side routes.

---

### 14. Bake index lacks call graph
**Source:** `[README]`
No `calls`/`called_by` data persisted. `api_trace` can't follow call chains.

**Fix:** Add `callee` extraction in `walk_ts()` — when inside a `function_declaration`, record `call_expression` callee names as outgoing edges.

---

### 15. No incremental baking
**Source:** `[README]`
Full re-bake on every invocation. For a 386-file project this is fast enough, but will become slow on monorepos (1000+ files).

**Fix:** Hash file contents; skip re-parsing files whose hash hasn't changed since last bake.

---

### 16. No `yoyo.yaml` config file support
**Source:** `[README]`
No per-project excludes (e.g. `node_modules/`, `dist/`, `vendor/`). face-api.js inflated file counts due to nested paths.

---

### 17. No tests or CI
**Source:** `[README]`
Zero unit tests in `src/`. No CI pipeline.

**Fix:** Unit test each `engine.rs` function against fixture TypeScript files. Integration test `bake` on a known project and assert specific functions are found.

---

### ✅ 18. License not set *(resolved in v0.2.0)*
MIT license added (`LICENSE` file, README updated).

---

## Counts by priority

| Priority | Count |
|---|---|
| 🔴 P0 (breaks usage) | 4 |
| 🟡 P1 (significant gaps) | 6 |
| 🟢 P2 (polish) | 8 |
| **Total** | **18** |
