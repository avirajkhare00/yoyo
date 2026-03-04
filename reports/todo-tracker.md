# TODO Tracker
**Last updated:** 2026-03-04 (updated file paths after engine refactor v0.3.1)
**Sources:** README.md roadmap + live code analysis + benchmark on face-api.js + blast-radius session report

Items are tagged with source:
- `[README]` — listed in README TODO/Roadmap section
- `[CODE]` — discovered by reading source files
- `[BENCH]` — confirmed broken by live benchmark run
- `[SESSION]` — discovered during a live implementation session

---

## 🔴 P0 — Breaks real usage today

### 1. `api_surface` has no output cap — overflows LLM context on large projects
**Source:** `[README]` `[BENCH]`
**File:** `src/engine/api.rs:40` — `api_surface()`

Returns all functions for all modules in one shot. Produced **50.7 KB** on face-api.js (386 files), exceeding inline display limits.

**Fix:** Honour existing `--limit` and `--package` flags to cap output. Add `"truncated": true` and `total_count` when results are capped.

---

### 2. `find_docs` has no output cap
**Source:** `[README]` `[BENCH]`

Produced 298 K characters on face-api.js. Completely unusable on any mid-size project.

**Fix:** Limit results to 50 items max. Add filtering at the query level, not just post-collection.

---

### 3. `architecture_map` role inference is too narrow
**Source:** `[README]` `[CODE]` `[BENCH]`
**File:** `src/engine/nav.rs:73` — `architecture_map()`

Only 3 path-pattern keywords (`routes`, `controllers`, `services`, `models`, `entities`). All 45 directories in face-api.js got `roles: []`. Also, the `intent` parameter is required but not documented as such — calling without it returns an MCP error, which is a bad experience.

**Missing keywords:** `handlers`, `repositories`, `resolvers`, `middleware`, `hooks`, `components`, `store`, `reducers`, `actions`, `selectors`, `utils`, `helpers`, `lib`, `api`, `net`, `network`, `dom`, `draw`, `ops`, `factories`.

**Fix:** Expand keyword table. Make `intent` optional with a sensible default (use empty string).

---

## 🟡 P1 — Significantly limits usefulness

### 4. `symbol` does not index structs, classes, or types
**Source:** `[CODE]` `[BENCH]` `[SESSION]`

Searching `symbol(BakeIndex)` returns nothing — it's a struct, not a function. Searching `symbol(SsdMobilenetv1)` returns call-site matches but not the class definition itself. During the blast-radius implementation session, finding `BakeIndex`'s definition required falling back to `supersearch` with `context: identifiers`.

**Fix:** Add `struct_item`, `type_item`, `class_declaration`, `interface_declaration` to each language analyzer. Store in a separate `types` array in `BakeIndex`. Extend the `symbol` tool to search both `functions` and `types`.

---

### 5. `blast_radius` output has no deduplication — same caller appears N times via N paths
**Source:** `[SESSION]`
**File:** `src/engine/analysis.rs:11` — `blast_radius()`

`blast_radius(load_bake_index, depth=3)` returns `call_tool` 15 times at depth 2 (once per engine function it calls). `run` appears 14 times at depth 3. The BFS correctly avoids re-enqueueing visited nodes but does not deduplicate the output `callers` vec.

**Fix:** Add a `--unique` flag (default true) that deduplicates callers by name+file before returning. Keep the current "show all paths" behaviour accessible via `--unique false`.

---

### 6. `supersearch` context/pattern flags unreliable — "best-effort" caveat
**Source:** `[README]` `[BENCH]`

CLI help strings and MCP schema both say "currently best-effort". Benchmark confirmed identical results across three filter combinations.

**Fix:** Verify AST filter wiring for all languages; remove "currently best-effort" wording once confirmed reliable.

---

### 7. `api_trace` and `crud_operations` limited to static route patterns
**Source:** `[README]`

Return zero results on NestJS, Fastify, Hono, CLI tools, ML libraries. Now that we have a call graph, `api_trace` could follow chains deeper than the route handler.

**Fix (incremental):**
- Phase 1: Detect NestJS `@Get()`, `@Post()`, `@Controller()` decorators via Tree-sitter.
- Phase 2: Detect Fastify/Hono route patterns.
- Phase 3: Use `calls` graph to follow handler call chains (now possible with blast_radius data).

---

### 8. No import/dependency graph
**Source:** `[SESSION]`

`blast_radius` traces call-graph edges (function → function) but has no concept of file-level imports. A file that imports a changed module but never calls its functions is not captured. Call-name matching is also unqualified — `foo` matches any `foo` regardless of module.

**Fix:** Extract `import`/`use`/`require` statements in each language analyzer. Store as `imports: Vec<String>` on `IndexedFile`. Build a file-level reverse dependency graph alongside the call graph.

---

## 🟢 P2 — Polish & completeness

### 9. `suggest_placement` recommends test files
**Source:** `[BENCH]`

Returns `test/tests/globalApi/detectAllFaces.test.ts` as top candidate for a service function. Test files should be excluded by default.

**Fix:** Exclude `test/`, `spec/`, `__tests__/` from placement candidates. Add `--include-tests` to opt in.

---

### 10. `shake` returns no function data if run before bake completes
**Source:** `[BENCH]`

When `shake` and `bake` run in parallel, `shake` fires before `bake.json` is written and falls back to a lightweight scan with no function data.

**Fix:** `shake` could note in the response that baking is in progress, or attempt a short retry.

---

### 11. `blast_radius` callers list is unsorted and unranked
**Source:** `[SESSION]`

Callers are returned in BFS traversal order. For large codebases, the most impactful callers (high-complexity functions, entry points) should surface first.

**Fix:** Sort callers by depth (ascending), then by function complexity (descending from bake index). Optionally add a `risk_score` per caller.

---

### 12. No incremental baking
**Source:** `[README]`

Full re-bake on every invocation. Fast enough for small projects, will bottleneck on monorepos (1000+ files).

**Fix:** Hash file contents; skip re-parsing files whose hash hasn't changed since last bake.

---

### 13. No `yoyo.yaml` config file support
**Source:** `[README]`

No per-project excludes beyond the hardcoded list (`.git`, `node_modules`, `target`, `dist`, `build`, `__pycache__`).

**Fix:** Support `yoyo.yaml` with `exclude`, `include_only`, and `depth` settings.

---

### 14. No tests or CI
**Source:** `[README]`

Zero unit tests in `src/`. No CI pipeline.

**Fix:** Unit test each engine submodule function against fixture files. Integration test `bake` on a known project and assert specific functions are found.

---

### 15. `search` only matches function names — misses struct/type names
**Source:** `[SESSION]`

Searching for `BakeIndex` returns 0 hits because it's a struct. `search` only indexes `functions`. Should also search the `types` array once #4 is resolved.

---

## ✅ Resolved

| # | Item | Version |
|---|---|---|
| ✅ | No language support beyond TypeScript/JavaScript | v0.2.0 |
| ✅ | License not set | v0.2.0 |
| ✅ | `symbol` requires a follow-up `slice` for source — added `--include-source` | v0.2.4 |
| ✅ | `walk_ts` missed `method_definition`, `arrow_function`, `function_expression` | v0.2.0 |
| ✅ | `supersearch` bypassed AST walk on default `context=all, pattern=all` | v0.2.5 |
| ✅ | Bake index lacked call graph — no `calls`/`called_by` data | v0.2.6 |
| ✅ | No blast radius analysis tool | v0.2.6 |
| ✅ | Go language support missing | v0.2.6 |
| ✅ | `engine.rs` monolith (1,756 lines) — split into 9 submodules under `src/engine/` | v0.3.1 |
| ✅ | Stale bake index after yoyo upgrade — auto-reindex when binary version > index version | v0.3.1 |
| ✅ | Stale bake index after source edits — auto-reindex when any source file newer than `bake.json` | v0.3.1 |
| ✅ | `llm_instructions` returned flat guidance string — replaced with structured tool catalog + 10 workflow chains | v0.3.2 |

---

## Priority summary

| Priority | Count |
|---|---|
| 🔴 P0 (breaks usage) | 3 |
| 🟡 P1 (significant gaps) | 5 |
| 🟢 P2 (polish) | 7 |
| ✅ Resolved | 12 |
| **Total tracked** | **27** |
