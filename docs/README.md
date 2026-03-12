# yoyo — full documentation

yoyo parses your codebase and gives Claude Code, Cursor, Codex CLI, Gemini CLI, or OpenCode 21 tools to read and edit it over MCP. Every answer comes from the AST — not model memory. No API keys, no SaaS, no telemetry.

**Eval:** 119/120 tasks correct (99%) across 7 real codebases vs 26% baseline (Claude Code without index).

---

## Contents

- [Philosophy](#philosophy)
- [Architecture](./architecture.md)
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
| `search` → `symbol` → `edit` | Find it, read it, change it |
| `callers` → `health` → `delete` | Who calls this? Is it dead? Remove it safely. |
| `flow` → `bulk_edit` | Trace the full request path, fix it end-to-end in one shot. |
| `index` → `ask` → `map` | Where does this new function belong? |
| `map` → `routes` → `create` | Understand the shape, find the gap, fill it. |

No single tool is the point. The orchestration is.

---

## How it works

```
index →  parse source files with tree-sitter  →  write bake.db
read  →  symbol / search / read / …           →  query bake.db
write →  edit / rename / …                     →  write file + reindex
```

**Read tools run in parallel. Write tools run sequentially.** After every write, the index resyncs automatically so the next read is always fresh.

The index is a SQLite database (`bakes/latest/bake.db`) in your project root. No server, no daemon.

---

## How Claude works with yoyo

Each session follows this sequence:

1. **Bootstrap** — Claude calls `boot` and `index` in parallel on first contact. `boot` returns tool names grouped by category and concurrency rules. `index` builds the AST index.
2. **Read** — `search`, `symbol`, `read` replace grep and cat. Structured data from the AST index, not line matches.
3. **Understand** — `callers`, `flow`, `health` answer structural questions no text tool can: who calls this? what does this touch? is this dead?
4. **Write** — `edit`, `rename`, `create`, `add`, `move`, `delete` mutate code and auto-reindex. Claude does not edit files directly when a yoyo write tool applies.
5. **Discover** — `help` returns params, output shape, example, and limitations for any tool on demand. No need to memorize schemas.

Result: Claude answers from facts, not memory. No hallucinated file paths. No stale function names.

---

## Installation

**macOS (Apple Silicon)**
```bash
brew tap avirajkhare00/yoyo
brew install yoyo
```
Homebrew handles signing and PATH. No `codesign`, no `sudo mv`.

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

For Gemini CLI, add yoyo from your terminal:
```bash
gemini mcp add yoyo /usr/local/bin/yoyo --mcp-server
```
If you installed to `~/.local/bin/yoyo`, use that path in the command.

For OpenCode, add yoyo from your terminal:
```bash
opencode mcp add
```
Then choose `Local (stdio)` and set: name `yoyo`, command `/usr/local/bin/yoyo`, args `--mcp-server`.

**Recommended — add a `UserPromptSubmit` hook** so Claude is reminded to prefer yoyo tools on every turn. Add to your project's `.claude/settings.local.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "echo '[yoyo] Use mcp__yoyo__search instead of Grep. Use mcp__yoyo__symbol+include_source instead of Read. Use mcp__yoyo__read for line ranges.'"
          }
        ]
      }
    ]
  }
}
```

---

## Tools reference (21 MCP tools)

### Bootstrap

| Tool | requires index | What it does |
|---|---|---|
| `boot` | No | Lean bootstrap: tool names grouped by category, concurrency rules (~500 tokens). Call first. |
| `index` | No | Parse the project, write the AST index (`bake.db`). Run before any read-indexed tool. |
| `help` | No | Progressive discovery: params, output shape, example, and limitations for any tool on demand. |

### Read

| Tool | requires index | What it does |
|---|---|---|
| `read` | No | Read any line range from any file. Use `start_line`/`end_line` from `symbol` output. |
| `symbol` | Yes | Find a function by name. Returns file, line range, visibility, calls, optionally full body. |
| `outline` | Yes | Every function in a file with line ranges and cyclomatic complexity. |
| `search` | Yes | AST-aware search. Finds call sites, assignments, identifiers. Replaces grep. |
| `ask` | Yes | Find functions by intent using local ONNX embeddings (fastembed). No API key. |

### Understand

| Tool | requires index | What it does |
|---|---|---|
| `map` | Yes | Directory tree with inferred roles (routes, services, models, etc.). |
| `callers` | Yes | All functions that transitively call a symbol. Affected file list included. |
| `flow` | Yes | Endpoint → handler → call chain to boundary in one call. |
| `routes` | Yes | All detected HTTP routes (Express, Actix, Rocket, gin, echo, net/http). |
| `health` | Yes | Dead code, large functions (high complexity + fan-out), duplicate name hints. |

### Write

| Tool | What it does |
|---|---|
| `edit` | Write changes by symbol name, line range, or exact string match. Compiles after write — rolls back on error (Rust, Go, Zig). Auto-reindexes. |
| `bulk_edit` | Apply N edits across M files in one call, bottom-up so offsets stay valid. |
| `rename` | Rename a symbol at its definition and every call site, atomically. |
| `create` | Create a new file with an initial function scaffold. Errors if file exists or parent dir missing. |
| `add` | Insert a function scaffold into an existing file, optionally after a named symbol. |
| `move` | Move a function from one file to another. Removes from source, appends to destination. |
| `delete` | Remove a function by name. Checks blast radius before deleting. |

### Orchestration

| Tool | What it does |
|---|---|
| `script` | Run a Rhai script with yoyo tools as functions. |

### CLI-only tools (not exposed via MCP)

These are available via `yoyo <command>` but removed from MCP to keep context cost low:

`shake`, `find_docs`, `suggest_placement`, `package_summary`, `trace_down`, `patch_bytes`, `llm_workflows`, `api_surface`, `api_trace`, `crud_operations`.

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
| Zig | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |

**Endpoints** — route detection via `routes`, `flow` (MCP) and `api_trace`, `crud_operations` (CLI).
**Import graph** — `callers` uses imports to expand affected files.
**flow** — BFS call chain to db/http/queue boundaries (Rust + Go today).

---

## Known limitations

- **Route detection is partial** — works for Express, Actix-web, Rocket, Flask, FastAPI, gin, echo, net/http. Axum, NestJS, Fastify, Django, and dynamic routers not yet supported.
- **`health` false positives for HTTP handlers** — functions registered via router (not direct calls) may be flagged as dead code. The static call graph can't see router registration.
- **`flow` call chain** — Rust + Go only. TypeScript and Python not yet supported.
- **Call graph is name-based** — `callers` matches callee names without module qualification. A function named `parse` in one package matches all callers of any `parse`.
- **C++ namespace false positives** — `namespace` blocks may appear as top-complexity entries.
- **`index` performance on large C codebases** — can time out on repos with 700+ files (tracked in [#65](https://github.com/avirajkhare00/yoyo/issues/65)).

Open issues: [github.com/avirajkhare00/yoyo/issues](https://github.com/avirajkhare00/yoyo/issues)

---

## Project layout

```
src/
  main.rs        binary entrypoint — CLI vs MCP switch
  cli.rs         CLI (clap) — exposes all engine capabilities
  mcp.rs         MCP JSON-RPC server over stdio — curated 21-tool surface
  engine/
    index.rs     boot (llm_instructions), index (bake), shake, help
    search.rs    symbol, search (supersearch), outline (file_functions), ask (semantic_search)
    edit.rs      edit (patch), bulk_edit (multi_patch), read (slice) + compiler guard
    graph.rs     rename, create, add, move, trace_chain
    analysis.rs  callers (blast_radius), health, delete (graph_delete), find_docs
    embed.rs     fastembed ONNX embeddings + SQLite store
    db.rs        SQLite bake index (bake.db) — read/write
    api.rs       routes (all_endpoints), flow, api_surface, api_trace, crud_operations
    nav.rs       map (architecture_map), package_summary, suggest_placement
    types.rs     shared payload structs
    util.rs      resolve_project_root, load_bake_index, reindex_files
  lang/
    mod.rs       IndexedFunction, IndexedEndpoint, LanguageAnalyzer trait
    rust.rs / go.rs / python.rs / typescript.rs / javascript.rs
    c.rs / cpp.rs / csharp.rs / java.rs / kotlin.rs / php.rs / ruby.rs / swift.rs / bash.rs / zig.rs
evals/
  harness/       real-repo puncture eval (Go) — clone → patch → run own tests → score
  tasks/         task.json + puncture.patch per codebase
  results/       timestamped JSON score records
```

---

Apache 2.0 — see [LICENSE](../LICENSE).
