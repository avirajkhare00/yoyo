# yoyo

yoyo parses your codebase and gives Claude or Cursor 25 tools to read and edit it over MCP.

Without a tool to read your code, AI assistants guess — wrong file paths, stale function names, invented module structures. yoyo stops the guessing. Every answer comes from the AST.

No API keys. No SaaS. No telemetry. Your code stays on your machine.

---

## What you get

- **Speed** — 8 parallel tool calls replace 15–20 minutes of manual file reading. A 400-file codebase in ~30 seconds.
- **Accuracy** — file paths, function names, line numbers, and byte offsets come directly from the parsed AST, not from model memory.
- **Semantic search** — find functions by intent ("semaphore acquisition", "spawn blocking task") using local ONNX embeddings. No API key required.

Evaluated on 3 real Rust codebases (9,954 functions): **81/81 tasks — 100%** structural + semantic accuracy. Full report: [`evals/REPORT.md`](./evals/REPORT.md)

---

## How it works

```
bake  →  parse source files with Tree-sitter  →  write bake.json
read  →  symbol / supersearch / slice / ...   →  read from bake.json
write →  patch / graph_rename / ...           →  write file + reindex
```

**Read tools run in parallel. Write tools run sequentially.** After every write, the index resyncs automatically.

---

## Installation

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

**Build from source** (requires [Rust stable](https://rustup.rs)):
```bash
git clone https://github.com/avirajkhare00/yoyo.git
cd yoyo
cargo build --release
sudo cp target/release/yoyo /usr/local/bin/yoyo
```

**Quick start:**
```bash
yoyo bake --path /path/to/your/project
yoyo shake --path /path/to/your/project
yoyo symbol --path /path/to/your/project --name myFunction
yoyo supersearch --path /path/to/your/project --query myFunction
```

---

## Use with Claude or Cursor (MCP)

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

Claude calls the tools automatically. You don't manage it.

---

## Tools

| Tool | What it does |
|---|---|
| `bake` | Parse the project and write the index. Run this first. |
| `shake` | Language breakdown, file count, top-complexity functions. |
| `symbol` | Find a function by name. Returns file, line range, optionally the full body. |
| `slice` | Read any line range from any file. |
| `file_functions` | Every function in a file with line ranges and complexity scores. |
| `supersearch` | AST-aware search. Finds call sites, assignments, identifiers across all files. |
| `blast_radius` | All functions that transitively call a symbol. Affected file list included. |
| `trace_down` | BFS call chain from a function to db/http/queue boundaries. Rust + Go. |
| `health` | Dead code, high-complexity functions, and duplicate function name hints. |
| `package_summary` | All functions, endpoints, and complexity in a module path. |
| `architecture_map` | Directory tree with inferred roles (routes, services, models, etc.). |
| `api_surface` | Exported functions grouped by module. |
| `suggest_placement` | Ranked list of files to add a new function to. |
| `find_docs` | Locate README, .env, Dockerfile, and config files. |
| `all_endpoints` | All detected HTTP routes (Express, Actix, Flask, FastAPI, gin, echo). |
| `api_trace` | Trace a route path + HTTP method to its handler function. |
| `crud_operations` | Create/read/update/delete matrix inferred from routes. |
| `patch` | Write changes by symbol name, line range, or exact string match. Auto-reindexes. |
| `patch_bytes` | Write at exact byte offsets from the index. |
| `multi_patch` | Apply N edits across M files in one call. |
| `graph_rename` | Rename a symbol at its definition and every call site atomically. |
| `graph_add` | Insert a new function scaffold into a file. |
| `graph_move` | Move a function from one file to another. |
| `semantic_search` | Find functions by intent using local ONNX embeddings (fastembed). No API key. |
| `graph_delete` | Remove a function by name. |

**Languages:** TypeScript, JavaScript, Rust, Python, Go

**Route detection:** Express, Actix-web, Rocket, Flask, FastAPI, gin, echo, net/http

---

## Known limitations

- **Route detection is partial** — `api_trace` and `crud_operations` work with the frameworks listed above. Axum, NestJS, Fastify, Django, and dynamic routers are not detected. See [#19](https://github.com/avirajkhare00/yoyo/issues/19).
- **`health` false positives for HTTP handlers** — functions registered via a router (not via direct calls) are flagged as dead code because the static call graph can't see the registration. Tracked in [#39](https://github.com/avirajkhare00/yoyo/issues/39).
- **`trace_down` is Rust + Go only** — call chain tracing doesn't work in TypeScript or Python yet.
- **Call graph is name-based** — `blast_radius` matches callee names without module qualification. A function named `parse` in one package matches all callers of any `parse`.
- **`graph_move` doesn't update imports** — it relocates the function body but doesn't add or remove `use`/`import` statements.
- **Common search terms explode** — `symbol` and `supersearch` return many matches for generic names like `parse` or `connect`. Use `--file` to scope to a directory and `--limit` to cap results.

Open issues: [github.com/avirajkhare00/yoyo/issues](https://github.com/avirajkhare00/yoyo/issues)

---

## Project layout

```
src/
  main.rs        binary entrypoint, CLI vs MCP switch
  cli.rs         CLI (clap)
  mcp.rs         MCP JSON-RPC server over stdio
  engine/
    index.rs     bake, shake, llm_instructions
    search.rs    symbol, supersearch, file_functions
    edit.rs      patch, patch_bytes, multi_patch, slice
    graph.rs     graph_rename, graph_add, graph_move
    analysis.rs  blast_radius, find_docs, health, graph_delete
    embed.rs     semantic_search — fastembed ONNX embeddings + SQLite store
    api.rs       all_endpoints, api_surface, api_trace, crud_operations
    nav.rs       architecture_map, package_summary, suggest_placement
    types.rs     shared payload structs
    util.rs      resolve_project_root, load_bake_index, reindex_files
  lang/
    typescript.rs
    rust.rs
    python.rs
    go.rs
```

---

## License

MIT — see [LICENSE](./LICENSE).
