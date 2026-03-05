# yoyo – Local Code Intelligence Engine

**yoyo** is a pure-Rust code-intelligence engine and MCP server that indexes your TypeScript, JavaScript, Rust, Python, and Go projects with Tree-sitter and exposes 22 LLM-ready tools over CLI and stdio.

No API keys. No SaaS. No telemetry. Your code stays on your machine.

---

## Why yoyo?

| | Without yoyo | With yoyo |
|---|---|---|
| Onboard to an unfamiliar codebase | `ls`, grep, file-by-file reading | A few parallel tool calls |
| Find where a function is defined | Search IDE, scroll, guess filenames | `yoyo symbol --name myFn` → file + line range |
| Trace usages across a large codebase | Grep hunt across dozens of files | `yoyo supersearch --query myFn` → instant |
| Understand a module's public API | Open every file in the module | `yoyo package_summary --package services` → done |
| Know where to add new code | Team discussion, guesswork | `yoyo suggest_placement --function-name sendEmail` |

---

## Installation

### Option 1 — Pre-built binary (recommended)

Download from the [latest GitHub release](https://github.com/avirajkhare00/yoyo/releases/latest):

**macOS (Apple Silicon)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-aarch64-apple-darwin.tar.gz \
  | tar xz
sudo mv yoyo-aarch64-apple-darwin /usr/local/bin/yoyo
```

**Linux (x86_64)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-x86_64-unknown-linux-gnu.tar.gz \
  | tar xz
sudo mv yoyo-x86_64-unknown-linux-gnu /usr/local/bin/yoyo
```

### Option 2 — Build from source

Requires [Rust stable](https://rustup.rs):

```bash
git clone https://github.com/avirajkhare00/yoyo.git
cd yoyo
cargo build --release
sudo cp target/release/yoyo /usr/local/bin/yoyo
```

### Quick start

```bash
# 1. index your project
yoyo bake --path /path/to/your/project

# 2. instant overview
yoyo shake --path /path/to/your/project

# 3. find a function by name (add --include-source to get body inline)
yoyo symbol --path /path/to/your/project --name myFunction

# 4. search across all files
yoyo supersearch --path /path/to/your/project --query prisma
```

---

## Use with AI assistants (MCP)

Add to `~/.claude/settings.json` (Claude Code) or your Cursor MCP config:

```json
{
  "mcpServers": {
    "yoyo": {
      "type": "stdio",
      "command": "/usr/local/bin/yoyo",
      "args": ["--mcp-server"]
    }
  }
}
```

Once configured, your AI assistant can call **all 22 yoyo tools as MCP methods**. Each method takes a `path` to the project (defaults to the current workspace in most editors) plus a few tool-specific arguments:

| MCP tool | Typical call | What it does |
|---|---|---|
| `bake` | `bake(path)` | Build the Tree-sitter index for the project. Must run before most other tools. |
| `shake` | `shake(path)` | High-level repo overview: languages, file counts, top-complex functions, endpoints. |
| `symbol` | `symbol(path, name, include_source, file?, limit?)` | Find a function by name. Use `file` to scope to a module and `limit` to cap result count. |
| `slice` | `slice(path, file, start, end)` | Read an exact line range from a file for precise code inspection. |
| `supersearch` | `supersearch(path, query, file?, limit?)` | AST-aware search across TypeScript, Rust, Python, and Go source files. |
| `file_functions` | `file_functions(path, file)` | List all functions in a file with complexity scores. |
| `api_surface` | `api_surface(path, package)` | Exported/public API grouped by module; omit `package` for a project-wide view. |
| `package_summary` | `package_summary(path, package)` | Deep-dive a module: files, functions, endpoints, and complexity. |
| `architecture_map` | `architecture_map(path, intent)` | Directory tree with role hints based on file names and an optional intent string. |
| `suggest_placement` | `suggest_placement(path, function_name, function_type, related_to)` | Scored candidates for where new code should live. |
| `all_endpoints` | `all_endpoints(path)` | List all detected HTTP endpoints (Express/Actix/Flask/FastAPI/gin/echo). |
| `api_trace` | `api_trace(path, endpoint, method)` | Trace a single route through its handler for a given path + HTTP method. |
| `crud_operations` | `crud_operations(path, entity)` | Inferred CRUD matrix from routes, optionally filtered to a specific entity. |
| `find_docs` | `find_docs(path, doc_type)` | Locate READMEs, `.env`, Dockerfiles, and other config/docs. |
| `patch` | `patch(path, ...)` | Apply structured edits by symbol or by file + line range. Auto-syncs the bake index after every write. |
| `blast_radius` | `blast_radius(path, symbol, depth)` | Find all functions that transitively call a symbol, and the affected files. |
| `graph_rename` | `graph_rename(path, name, new_name)` | Rename a symbol at its definition and every call site atomically. Word-boundary matching prevents partial renames. |
| `graph_add` | `graph_add(path, entity_type, name, file, after_symbol?)` | Insert a new function scaffold at the right location; fill the body with `patch`. |
| `graph_move` | `graph_move(path, name, to_file)` | Move a function from one file to another; removes from source, appends to destination. |
| `trace_down` | `trace_down(path, name, depth?, file?)` | Trace a function's call chain downward to external boundaries (db, http, queue). Go + Rust. |
| `llm_instructions` | `llm_instructions(path)` | Return a compact JSON "prime directive" with guidance on how assistants should use yoyo. |

In Claude, Cursor, and other MCP-aware tools, you typically don't call these methods manually — the assistant selects and calls them as needed to ground its answers in your actual code.

---

## Benchmark

Tested on two real TypeScript projects from scratch:

| Project | Files | Type |
|---|---|---|
| `schema-generator-prisma` | 24 | CLI tool |
| `face-api.js` | 386 | ML library |

**With yoyo (8 tool calls, 3 parallel batches):**
```
Batch 1:  bake + shake
Batch 2:  architecture_map + find_docs + api_surface
Batch 3:  package_summary(globalApi) + package_summary(mtcnn) + supersearch(detectAllFaces)
```
Total wall-clock time: **~30 seconds**

**Without yoyo (manual):** ~15–20 minutes of directory browsing, grepping, and file reading.

Full benchmark report: [`reports/benchmark-face-api-js-2026-03-03.md`](./reports/benchmark-face-api-js-2026-03-03.md)

---

## Why not just use LSP?

LSP is for humans navigating code in an editor. yoyo is for AI agents understanding code. Different consumer, different job.

| | LSP | yoyo |
|---|---|---|
| Consumer | Editor (VS Code, Neovim…) | AI assistant (Claude, Cursor…) |
| Protocol | JSON-RPC to editor buffers | MCP stdio — AI calls tools directly |
| Scope | Per-file, cursor-aware | Whole codebase in one call |
| Setup | One server per language (gopls, rust-analyzer, pyright…) | One binary for all languages |
| "Where should new code go?" | No equivalent | `suggest_placement` |
| Project-wide complexity overview | No equivalent | `shake` |
| Edit by symbol name | No equivalent | `patch` |

LSP tells you what exists at your cursor. yoyo tells an AI what the codebase looks like and lets it act on it. Use both — LSP while writing, yoyo when asking Claude to understand or modify a codebase it has never seen.

---

## Known limitations (current version)

- **Limited route detection** — `api_trace` and `crud_operations` detect Express (TS), Actix/Rocket (Rust), Flask/FastAPI (Python), and gin/echo/net-http (Go) routes. NestJS decorators, Fastify, and dynamic routers are not supported.
- **Result explosion on common terms** — `symbol` and `supersearch` can return hundreds of matches for generic names like `connect` or `parse`. Use `--file` to scope to a directory/file and `--limit` to cap the result count. Other tools (`api_surface`, `find_docs`, `package_summary`) have no cap yet.
- **Inline closure handlers not named** — In Go codebases where route handlers are inline closures (e.g. `r.GET("/path", func(c *gin.Context){...})`), `all_endpoints` returns `handler_name: null` and `file_functions` does not index the closure body. Name your handlers or extract them to named functions.
- **Chained method calls partially resolved** — `trace_down` extracts the first qualifier in a chain (`db` from `db.Query()`). Chained GORM-style calls like `db.DB.Preload("Nodes").First(...)` are listed as unresolved rather than classified as `database` boundary.
- **Rust macro syntax not caught by tree-sitter** — Post-patch syntax validation uses tree-sitter (fast) plus `cargo check` (thorough). For non-Rust languages, only tree-sitter runs; `go build`, `python3 -m py_compile`, and `tsc --noEmit` are used for Go, Python, and TypeScript respectively.
- **Call graph is name-based** — `blast_radius` matches callee names without module qualification. A function named `parse` in one package will match all callers of any `parse`. Re-run `bake` after code changes to refresh the graph.
- **Import updates not automated** — `graph_move` relocates a function body between files but does not add or remove `use`/`import` statements. Update those manually after moving.

Open issues: [github.com/avirajkhare00/yoyo/issues](https://github.com/avirajkhare00/yoyo/issues)
---

## Project layout

```
src/
  main.rs           binary entrypoint, CLI vs MCP switch
  cli.rs            human-facing CLI (clap)
  mcp.rs            MCP JSON-RPC server over stdio
  engine/
    mod.rs          public re-exports for all engine modules
    index.rs        bake, shake, llm_instructions
    search.rs       symbol, supersearch, file_functions
    edit.rs         patch, patch_bytes, patch_by_symbol, multi_patch, slice
    graph.rs        graph_rename, graph_add, graph_move
    analysis.rs     blast_radius, find_docs
    api.rs          all_endpoints, api_surface, api_trace, crud_operations
    nav.rs          architecture_map, package_summary, suggest_placement
    types.rs        shared payload structs
    util.rs         resolve_project_root, load_bake_index, reindex_files
  lang/
    mod.rs          LanguageAnalyzer trait, IndexedFunction (incl. calls graph), shared AST helpers
    typescript.rs   TypeScript/JS — functions, arrow fns, Express routes, call extraction
    rust.rs         Rust — functions, Actix/Rocket routes, call + method_call extraction
    python.rs       Python — functions, Flask/FastAPI decorators, call extraction
    go.rs           Go — functions, methods, gin/echo/net-http routes, call extraction
```

---

## License

MIT — see [LICENSE](./LICENSE).
