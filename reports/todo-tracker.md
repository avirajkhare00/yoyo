# TODO Tracker

Issues are tracked on GitHub: https://github.com/avirajkhare00/yoyo/issues

---

## ✅ Resolved

| Item | Version |
|------|---------|
| No language support beyond TypeScript/JavaScript | v0.2.0 |
| License not set | v0.2.0 |
| `symbol` requires follow-up `slice` for source — added `--include-source` | v0.2.4 |
| `walk_ts` missed `method_definition`, `arrow_function`, `function_expression` | v0.2.0 |
| `supersearch` bypassed AST walk on default `context=all, pattern=all` | v0.2.5 |
| Bake index lacked call graph — no `calls`/`called_by` data | v0.2.6 |
| No blast radius analysis tool | v0.2.6 |
| Go language support missing | v0.2.6 |
| `engine.rs` monolith (1,756 lines) — split into 9 submodules under `src/engine/` | v0.3.1 |
| Stale bake index after yoyo upgrade — auto-reindex on version change | v0.3.1 |
| Stale bake index after source edits — auto-reindex when source newer than bake | v0.3.1 |
| `llm_instructions` returned flat string — replaced with structured tool catalog + workflows | v0.3.2 |
| `symbol`/`search` missed structs, classes, interfaces, enums — added `IndexedType` | v0.3.3 |
| `search` only matched function names — now also searches `types` array | v0.3.3 |
| Bake index stale after patch — added `reindex_files()` auto-sync | v0.6.0 |
| No graph-level mutation tools — added `graph_rename`, `graph_add`, `graph_move` | v0.6.0 |
| Result explosion on `symbol`/`supersearch` — added `--file` and `--limit` | v0.7.0 |
| No downward call chain tracing — added `trace_down` (Go + Rust) | v0.8.0 |
| Patch tool silently corrupted files — added post-patch syntax validation | v0.8.0 |
