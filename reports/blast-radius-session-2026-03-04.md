# Blast Radius Feature — Session Report
**Date:** 2026-03-04
**Feature:** Call graph extraction + blast radius analysis (MCP tool + CLI)
**Codebase size:** 4,466 LoC across 9 Rust source files (was ~3,331 on 2026-03-03, +1,135 LoC)

---

## Did yoyo help build this?

Honest answer: **yes, meaningfully — but unevenly.**

### Where yoyo genuinely saved time

**`symbol` with `include_source: true`** was the single most useful tool. Before touching any file, I used it to read `build_bake_index`, `search`, `api_trace`, `list_tools`, and all four `ast_search` implementations in one pass — getting source + line numbers without hunting through files. That replaced about 8-10 manual Read calls.

**`slice`** was useful for targeted reads: finding where `BakeIndex` struct was defined (via `supersearch` → line 1155), then slicing exactly those lines. Also used to read the tail of `mcp.rs` and `cli.rs` to find insertion points without reading entire files.

**`supersearch`** found `BakeIndex` usage across files cleanly. Confirmed it appears in `engine.rs` at lines 1155, 1520, 1528, 1533, 1593 — told me exactly what I needed to update.

**`file_functions`** gave me the full function list for `engine.rs` with line numbers in one call. This let me find the right insertion point for `blast_radius` (after `api_trace` at line 913) without reading 1,700 lines.

**`shake`** gave a quick sanity check after the build — confirmed the new `blast_radius` function appeared in the top-10 complexity list (complexity 8), and that `collect_calls_inner` in rust.rs was also indexed.

### Where yoyo fell short

**`search` is too shallow for structural work.** It's fuzzy-name-only — searching for "BakeIndex" returned 0 hits because `BakeIndex` is a struct, not a function. Had to fall back to `supersearch`. The `search` tool should handle struct/type names.

**`architecture_map` requires an `intent` parameter** — easy to forget, returned an error on the first call. Minor but slightly disruptive when trying to orient quickly.

**No "where is this type defined?" query.** There's no equivalent of `symbol` for structs/types. I had to use `supersearch` with `context: identifiers` to find `struct BakeIndex {`. A `type_symbol` tool or extending `symbol` to handle non-function declarations would fill this gap.

**The bake index was stale** at the start of the session — it had been built before the `calls` field was added, so `shake` showed functions without calls during development. This is expected behavior but worth noting: the index is a snapshot, not a live view.

**`blast_radius` on itself** — the tool can't query data about itself until after bake is re-run, which is a bootstrapping quirk. Not a flaw, just reality.

---

## What was built

### Architecture
```
IndexedFunction.calls: Vec<String>   ← new field (serde default for backward compat)

Lang analyzers (4 files):
  collect_calls(node) → Vec<String>  ← recursive AST walk per language

engine.rs:
  blast_radius(symbol, depth) → JSON
    builds called_by: HashMap<callee → [(caller, file)]]>
    BFS with visited dedup + depth cap

mcp.rs:   blast_radius tool definition + handler
cli.rs:   blast-radius --symbol <name> [--depth N]
```

### LoC added this session: ~200 net
- `lang/mod.rs`: +2 lines (field + serde attr)
- `lang/typescript.rs`: +35 lines (collect_calls + 2 inline updates)
- `lang/rust.rs`: +50 lines (collect_calls handles call_expression + method_call_expression)
- `lang/python.rs`: +35 lines (collect_calls for call + attribute)
- `lang/go.rs`: +35 lines (collect_calls for call_expression + selector_expression)
- `engine.rs`: +65 lines (blast_radius function)
- `mcp.rs`: +28 lines (tool definition + handler)
- `cli.rs`: +20 lines (struct + dispatch + handler)

### Sample output
```json
{
  "tool": "blast_radius",
  "symbol": "build_bake_index",
  "depth": 2,
  "callers": [
    { "caller": "bake",      "file": "src/engine.rs", "depth": 1 },
    { "caller": "call_tool", "file": "src/mcp.rs",    "depth": 2 },
    { "caller": "run_bake",  "file": "src/cli.rs",    "depth": 2 }
  ],
  "affected_files": ["src/cli.rs", "src/engine.rs", "src/mcp.rs"],
  "total_callers": 3
}
```

---

## Design decisions

**No graph DB (CozoDB or otherwise):** BFS over a flat `HashMap<callee, Vec<(caller, file)>>` reconstructed at query time from `bake.functions` is sufficient. Zero new dependencies.

**`calls` stores callee names only (not fully-qualified paths).** This means `foo` matches any `foo` regardless of module. Good enough for most blast radius use cases; precise cross-module resolution would require import graph tracking.

**Duplicate callers in output are intentional.** If `call_tool` calls both `shake` and `search`, and both call `load_bake_index`, then `call_tool` appears twice at depth 2 — once per path. This shows cardinality, not just reachability.

**Backward-compatible bake schema:** `#[serde(default)]` on `calls` means old bake indexes (without the field) still load fine — calls will be empty, blast_radius will return no results until re-baked.

---

## What yoyo is missing (discovered during this session)

1. **Type/struct symbol lookup** — `symbol` only indexes functions
2. **Import/dependency graph** — would enable file-level blast radius without call-graph noise
3. **"Who defines this field?"** query — useful when changing struct layouts
4. **Cross-file deduplication in blast_radius output** — caller appears N times if it's reachable via N paths; should offer a `--unique` mode
