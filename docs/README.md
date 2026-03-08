# yoyo — full documentation

yoyo parses your codebase and gives Claude or Cursor 27 tools to read and edit it over MCP. Every answer comes from the AST — not model memory. No API keys, no SaaS, no telemetry.

**Eval:** 119/120 tasks correct (99%) across 7 real codebases vs 26% baseline (Claude Code without index).

---

## Contents

- [Philosophy](#philosophy)
- [How it works](#how-it-works)
- [How Claude works with yoyo](#how-claude-works-with-yoyo)
- [Installation](#installation)
- [MCP setup](#mcp-setup)
- [Tools reference](#tools-reference)
- [Language support matrix](#language-support-matrix)
- [Known limitations](#known-limitations)
- [Project layout](#project-layout)

---

## Philosophy

In yoyo tournaments, a yoyo is just a spinning disk on a string. The magic is in the combinations — string wraps, body movements, timing layered together. A single trick is fine. Fifty moves chained in sequence is something else entirely.

yoyo (the tool) works the same way. Each tool does one thing cleanly. The power is in how your agent orchestrates them:

| Combination | What it does |
|---|---|
| `supersearch` → `symbol` → `patch` | Find it, read it, change it |
| `blast_radius` → `health` → `graph_delete` | Who calls this? Is it dead? Remove it safely. |
| `flow` → `multi_patch` | Trace the full request path, fix it end-to-end in one shot. |
| `bake` → `semantic_search` → `suggest_placement` | Where does this new function belong? |
| `architecture_map` → `api_surface` → `graph_create` | Understand the shape, find the gap, fill it. |

No single tool is the point. The orchestration is.

---

## How it works

```
bake  →  parse source files with ast-grep  →  write bake.json
read  →  symbol / supersearch / slice / …  →  read from bake.json
write →  patch / graph_rename / …          →  write file + reindex
```

**Read tools run in parallel. Write tools run sequentially.** After every write, the index resyncs automatically so the next read is always fresh.

The index is a plain JSON file (`bakes/latest/bake.json`) in your project root. No server, no daemon.

---

## How Claude works with yoyo

Each session follows this sequence:

1. **Bootstrap** — Claude loads `llm_instructions` on first contact. Returns tool list, workflows, prime directives. No file reading, no grepping.
2. **Read** — `supersearch`, `symbol`, `slice` replace grep and cat. Structured data from the AST index, not line matches.
3. **Understand** — `blast_radius`, `flow`, `trace_down`, `health` answer structural questions no text tool can: who calls this? what does this touch? is this dead?
4. **Write** — `patch`, `graph_rename`, `graph_create`, `graph_add`, `graph_move`, `graph_delete` mutate code and auto-reindex. Claude does not edit files directly when a yoyo write tool applies.
5. **Dogfood** — every session building yoyo is a yoyo session. Gaps found while building are filed as issues immediately.

Result: Claude answers from facts, not memory. No hallucinated file paths. No stale function names.

---

## Installation

**macOS — Homebrew (recommended)**
```bash
brew tap avirajkhare00/yoyo
brew install yoyo
```
Homebrew handles signing and PATH. No `codesign`, no `sudo mv`.

**macOS — manual (Apple Silicon)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-aarch64-apple-darwin.tar.gz | tar xz
sudo mv yoyo-aarch64-apple-darwin /usr/local/bin/yoyo
# Required: sign the binary or Gatekeeper kills it silently (exit 137)
codesign --force --deep --sign - /usr/local/bin/yoyo
```

**Linux (x86_64)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv yoyo-x86_64-unknown-linux-gnu /usr/local/bin/yoyo
```

**Build from source** (requires [Rust stable](https://rustup.rs)):
```bash
git clone https://github.com/avirajkhare00/yoyo.git
cd yoyo && cargo build --release
sudo cp target/release/yoyo /usr/local/bin/yoyo
```

**Quick start:**
```bash
yoyo bake --path /path/to/your/project
yoyo shake --path /path/to/your/project
yoyo symbol --path /path/to/your/project --name myFunction
```

---

## MCP setup

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

For Codex CLI, add yoyo from your terminal:
```bash
codex mcp add yoyo -- /usr/local/bin/yoyo --mcp-server
```
If you installed to `~/.local/bin/yoyo`, use that path in the command.

**Recommended — add a `UserPromptSubmit` hook** so Claude is reminded to prefer yoyo tools on every turn. Add to your project's `.claude/settings.local.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "echo '[yoyo] Use mcp__yoyo__supersearch instead of Grep/Bash grep. Use mcp__yoyo__symbol+include_source instead of Read. Use mcp__yoyo__slice for line ranges. yoyo tools must be loaded via ToolSearch first if not yet loaded.'"
          }
        ]
      }
    ]
  }
}
```

---

## Tools reference

### Bootstrap

| Tool | requires bake | What it does |
|---|---|---|
| `llm_instructions` | No | Prime directive: tool list, workflows, prime directives. Claude calls this first. |
| `bake` | No | Parse the project, write the index. Run before any indexed tool. |
| `shake` | No | Language breakdown, file count, top-complexity functions. |

### Read (no bake required)

| Tool | What it does |
|---|---|
| `slice` | Read any line range from any file. Use `start_line`/`end_line` from `symbol` output. |
| `find_docs` | Locate README, .env, Dockerfile, and config files. |

### Read (bake required)

| Tool | What it does |
|---|---|
| `symbol` | Find a function by name. Returns file, line range, visibility, calls, optionally full body. |
| `file_functions` | Every function in a file with line ranges and cyclomatic complexity. |
| `supersearch` | AST-aware search. Finds call sites, assignments, identifiers. Prefer over grep. |
| `semantic_search` | Find functions by intent using local ONNX embeddings (fastembed). No API key. |
| `blast_radius` | All functions that transitively call a symbol. Affected file list included. |
| `trace_down` | BFS call chain from a function to db/http/queue boundaries. Rust + Go only. |
| `flow` | **One-call vertical slice:** endpoint → handler → call chain to boundary. Replaces `api_trace` + `trace_down` + `symbol`. |
| `health` | Dead code, god functions (high complexity + fan-out), duplicate name hints. |
| `package_summary` | All functions, endpoints, and complexity for a module path substring. |
| `architecture_map` | Directory tree with inferred roles (routes, services, models, etc.). |
| `api_surface` | Exported functions grouped by module. |
| `suggest_placement` | Ranked files to add a new function to, based on related symbols. |
| `all_endpoints` | All detected HTTP routes (Express, Actix, Flask, FastAPI, gin, echo). |
| `api_trace` | Trace a route path + HTTP method to its handler function. |
| `crud_operations` | Create/read/update/delete matrix inferred from routes. |

### Write

| Tool | What it does |
|---|---|
| `patch` | Write changes by symbol name, line range, or exact string match. Auto-reindexes. |
| `patch_bytes` | Splice at exact byte offsets from the index. |
| `multi_patch` | Apply N edits across M files in one call, bottom-up so offsets stay valid. |
| `graph_rename` | Rename a symbol at its definition and every call site, atomically. |
| `graph_create` | Create a new file with an initial function scaffold. Errors if file exists or parent dir missing. |
| `graph_add` | Insert a function scaffold into an existing file, optionally after a named symbol. |
| `graph_move` | Move a function from one file to another. Removes from source, appends to destination. |
| `graph_delete` | Remove a function by name. Checks blast radius before deleting. |

---

## Language support matrix

| Language | Functions | Types | Endpoints | Import graph | AST search | trace_down |
|---|---|---|---|---|---|---|
| Rust | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Go | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Python | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| TypeScript | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| JavaScript | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| C | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| C++ | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| C# | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| Java | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| Kotlin | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| PHP | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| Ruby | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| Swift | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| Bash | ✅ | ❌ | ❌ | ❌ | ✅ | ❌ |

**Endpoints** — route detection via `all_endpoints`, `api_trace`, `crud_operations`, `flow`.
**Import graph** — `blast_radius` uses imports to expand affected files.
**trace_down / flow** — BFS call chain to db/http/queue boundaries (Rust + Go today).

---

## Known limitations

- **Route detection is partial** — works for Express, Actix-web, Rocket, Flask, FastAPI, gin, echo, net/http. Axum, NestJS, Fastify, Django, and dynamic routers not yet supported.
- **`health` false positives for HTTP handlers** — functions registered via router (not direct calls) may be flagged as dead code. The static call graph can't see router registration.
- **`trace_down` / `flow` call chain** — Rust + Go only. TypeScript and Python not yet supported.
- **Call graph is name-based** — `blast_radius` matches callee names without module qualification. A function named `parse` in one package matches all callers of any `parse`.
- **C++ namespace false positives** — `namespace` blocks may appear as top-complexity entries.
- **bake performance on large C codebases** — can time out on repos with 700+ files (tracked in [#65](https://github.com/avirajkhare00/yoyo/issues/65)).

Open issues: [github.com/avirajkhare00/yoyo/issues](https://github.com/avirajkhare00/yoyo/issues)

---

## Project layout

```
src/
  main.rs        binary entrypoint — CLI vs MCP switch
  cli.rs         CLI (clap)
  mcp.rs         MCP JSON-RPC server over stdio
  engine/
    index.rs     bake, shake, llm_instructions
    search.rs    symbol, supersearch, file_functions, semantic_search
    edit.rs      patch, patch_bytes, multi_patch, slice
    graph.rs     graph_rename, graph_create, graph_add, graph_move, trace_chain
    analysis.rs  blast_radius, find_docs, health, graph_delete
    embed.rs     fastembed ONNX embeddings + SQLite store
    api.rs       all_endpoints, api_surface, api_trace, crud_operations, flow
    nav.rs       architecture_map, package_summary, suggest_placement
    types.rs     shared payload structs
    util.rs      resolve_project_root, load_bake_index, reindex_files
  lang/
    mod.rs       IndexedFunction, IndexedEndpoint, LanguageAnalyzer trait
    rust.rs / go.rs / python.rs / typescript.rs / javascript.rs
    c.rs / cpp.rs / csharp.rs / java.rs / kotlin.rs / php.rs / ruby.rs / swift.rs / bash.rs
```

---

MIT — see [LICENSE](../LICENSE).
