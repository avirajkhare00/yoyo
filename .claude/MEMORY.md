# yoyo Project Memory

## MCP Binary Location
The MCP server uses `~/.local/bin/yoyo` (NOT `/tmp/yoyo-aarch64-apple-darwin`).

**Deploy workflow after any engine change:**
```
cargo build --release && cp target/release/yoyo ~/.local/bin/yoyo && xattr -c ~/.local/bin/yoyo && codesign --force --sign - ~/.local/bin/yoyo
```
Then restart Claude Code to reload the MCP server.

## Architecture (current — engine/ split as of v0.6.0)
- `src/engine/mod.rs` — public re-exports
- `src/engine/index.rs` — bake, shake, llm_instructions + tool/workflow catalog
- `src/engine/search.rs` — symbol, supersearch, file_functions
- `src/engine/edit.rs` — patch, patch_bytes, patch_by_symbol, multi_patch, slice
- `src/engine/graph.rs` — graph_rename, graph_add, graph_move, trace_down
- `src/engine/analysis.rs` — blast_radius, find_docs
- `src/engine/api.rs` — all_endpoints, api_surface, api_trace, crud_operations
- `src/engine/nav.rs` — architecture_map, package_summary, suggest_placement
- `src/engine/types.rs` — all payload structs
- `src/engine/util.rs` — resolve_project_root, load_bake_index, reindex_files
- `src/mcp.rs` — MCP stdio server
- `src/cli.rs` — human-facing CLI

## Languages Supported
TypeScript/JS, Rust, Python, Go (gin/echo/net-http route detection)

## Adding a New Language
1. Create `src/lang/<lang>.rs` implementing `LanguageAnalyzer`
2. Add `pub mod <lang>;` and `Box::new(<lang>::Analyzer)` to `src/lang/mod.rs`
3. Add lang to `supersearch()` filter in `src/engine/search.rs`
4. Deploy new binary

## Bake Index
- Field names: `functions` (Vec<IndexedFunction>), `endpoints` (Vec<IndexedEndpoint>), `types` (Vec<IndexedType>)
- `IndexedFunction.calls` is `Vec<CallSite>` with `{ callee, qualifier?, line }` — qualifier is the receiver/object (e.g. `db` from `db.Query()`)
- Bake skips: `.git`, `node_modules`, `target`, `dist`, `build`, `__pycache__`

## Version History
- v0.6.0 — auto-sync after patch (reindex_files), graph_rename, graph_add, graph_move
- v0.7.0 — symbol + supersearch get --file (scope) and --limit (cap) params; README updated to 21 tools
- v0.8.0 — trace_down: BFS call chain tracer (Go+Rust), CallSite schema (callee+qualifier+line), 22 tools
- v0.9.0 — api_surface module cap + truncated field; find_docs limit (default 50) + config pattern fix; architecture_map role inference expanded (30+ keywords, 10 categories), intent optional. NOTE: should have been v0.8.1 (all patch-level fixes); over-bumped by mistake.

## Semver Rules (strict)
- Bug fixes / output cap / pattern fixes → PATCH bump (0.x.Y)
- New tool or new user-visible feature → MINOR bump (0.X.0)
- Breaking API/schema change → MAJOR bump (X.0.0)
Never bump MINOR for bug fixes. Always ask: "is this a new feature or a fix?"

## Known Tool Weaknesses (backlog)
- `architecture_map` suggestions empty when dir uses generic names (e.g. `api/`) not `routes/`/`handlers/` — suggestions heuristic too narrow
- `architecture_map` role inference: "repo" substring matches `reports/` as repositories — needs word-boundary matching
- `graph_rename`, `graph_add`, `graph_move` untested in real-world tasks so far

## Tag/Release Workflow
Push tag only — CI pipeline handles release artifacts automatically:
```
git tag vX.Y.Z && git push origin main && git push origin vX.Y.Z
```
Do NOT use `gh release create`.
