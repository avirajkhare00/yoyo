<p align="center">
  <img src="logo.svg" width="80" height="96" alt="yoyo logo"/>
</p>

# yoyo

**Claude Code on steroids. Codex on steroids. Any AI coding agent — on steroids.**

yoyo is an MCP server that gives your agent 30 AST-grounded tools to read, understand, and edit code. No hallucinated file paths. No guessing. Facts from the source.

**99% eval accuracy** across 4 languages, 8 real codebases — vs 26% baseline (Claude Code alone).

---

## Why

Your AI agent reads code like a human with no IDE: grep, cat, hope. It hallucinates function names. It misses callers. It patches the wrong file.

yoyo gives it what it was missing: a structured interface to the codebase. The agent calls `symbol` instead of `cat`. It calls `blast_radius` before deleting. It calls `flow` to trace a request end to end. It patches by function name, not line number.

The eval gap is the proof: **99% vs 26%**. Same model. Same tasks. Different tools.

---

## Language focus

> **Rust · Go · Zig · TypeScript — four languages, done deep.**

| Language | bake | symbol | trace_down | endpoints | write tools |
|---|---|---|---|---|---|
| Rust | ✅ | ✅ | ✅ | ✅ actix/rocket | ✅ |
| Go | ✅ | ✅ | ✅ | ✅ gin/echo/net-http | ✅ |
| Zig | ✅ | ✅ | — | — | ✅ |
| TypeScript | ✅ | ✅ | — | ✅ express | ✅ |

Not every language. The four where systems-level code intelligence matters most.

---

## The combinations are the point

One trick is fine. Fifty moves chained is transcendent.

| Combination | What it does |
|---|---|
| `supersearch` → `symbol` → `patch` | find it, read it, change it ⚡ |
| `blast_radius` → `health` → `graph_delete` | who calls this? is it dead? remove it safely |
| `flow` → `multi_patch` | trace the full request path, fix it end-to-end in one shot |
| `bake` → `semantic_search` → `suggest_placement` | where does this new function belong? |
| `architecture_map` → `all_endpoints` → `graph_create` | understand the shape, find the gap, fill it |

No single tool is the point. The orchestration is.

---

## Setup (4 steps)

### 1. Install

**macOS (Apple Silicon)**
```bash
brew tap avirajkhare00/yoyo
brew install yoyo
```

**Linux (x86_64)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv yoyo-x86_64-unknown-linux-gnu /usr/local/bin/yoyo
```

```bash
yoyo --version
```

---

### 2. Add to your agent's MCP config

**Claude Code** — add to `~/.claude/settings.json`:
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

**Codex CLI**
```bash
codex mcp add yoyo -- /usr/local/bin/yoyo --mcp-server
```

**Gemini CLI**
```bash
gemini mcp add yoyo /usr/local/bin/yoyo --mcp-server
```

**OpenCode** — run `opencode mcp add` → Local (stdio) → name `yoyo` → command `/usr/local/bin/yoyo` → args `--mcp-server`.

**Cursor** — same JSON block as Claude Code, in your Cursor MCP config.

---

### 3. Index your project

```bash
yoyo bake --path /path/to/your/project
```

Run once per project, again after large changes.

---

### 4. Teach your agent to prefer yoyo

**Claude Code** — add to `.claude/settings.local.json`:
```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "echo '[yoyo] Use mcp__yoyo__supersearch instead of Grep. Use mcp__yoyo__symbol+include_source instead of Read. Use mcp__yoyo__slice for line ranges.'"
          }
        ]
      }
    ]
  }
}
```

**Codex** — add to `AGENTS.md`:
```md
## yoyo
Load `mcp__yoyo__llm_instructions` first.
Prefer `supersearch` over grep, `symbol` over file reads, `patch` for edits.
```

Without this, your agent sees yoyo but won't reach for it first.

---

## Tools

### Bootstrap
| Tool | What it does |
|---|---|
| `bake` | Parse the project, write the AST index. Run first. |
| `shake` | Language breakdown, file count, top-complexity functions. |
| `llm_instructions` | Lean bootstrap: tool catalog, prime directives, concurrency rules. |
| `llm_workflows` | On-demand reference: combination workflows, decision map, antipatterns. |

### Read
| Tool | What it does |
|---|---|
| `symbol` | Find a function by name — file, line range, optionally full body. |
| `slice` | Read any line range from any file. |
| `supersearch` | AST-aware search across all files. Replaces grep. |
| `semantic_search` | Find functions by intent. Local ONNX embeddings, no API key. |
| `file_functions` | Every function in a file with complexity scores. |
| `find_docs` | Locate README, .env, Dockerfile, config files. |

### Understand
| Tool | What it does |
|---|---|
| `blast_radius` | All transitive callers of a symbol + affected files. |
| `flow` | Endpoint → handler → call chain in one call. |
| `trace_down` | BFS call chain to db/http/queue boundary. Rust + Go. |
| `health` | Dead code, large functions, duplicate names. |
| `architecture_map` | Directory tree with inferred roles. |
| `package_summary` | Functions, endpoints, complexity for a module path. |
| `api_surface` | Exported functions grouped by module. |
| `suggest_placement` | Ranked files to place a new function. |
| `all_endpoints` | All detected HTTP routes. |
| `api_trace` | Route path + method → handler function. |
| `crud_operations` | CRUD matrix inferred from routes. |

### Write
| Tool | What it does |
|---|---|
| `patch` | Write by symbol name, line range, or string match. Compiles after write — rolls back on error. Auto-reindexes. |
| `patch_bytes` | Write at exact byte offsets. |
| `multi_patch` | N edits across M files in one call. |
| `graph_rename` | Rename a symbol at definition + every call site, atomically. |
| `graph_create` | Create a new file with an initial function scaffold. |
| `graph_add` | Insert a function scaffold into an existing file. |
| `graph_move` | Move a function between files. |
| `graph_delete` | Remove a function by name. Checks blast radius first. |

---

## Why not just LSP?

LSP is for humans in an editor. yoyo is for AI agents understanding codebases.

| | LSP | yoyo |
|---|---|---|
| Consumer | Editor (VS Code, Neovim…) | AI agent (Claude, Codex, Cursor…) |
| Protocol | JSON-RPC to editor buffers | MCP stdio — agent calls tools directly |
| Scope | Per-file, cursor-aware | Whole codebase in one call |
| Setup | One server per language | One binary for all languages |
| "Where should new code go?" | No equivalent | `suggest_placement` |
| Edit by function name | No equivalent | `patch` |

Use both. LSP while you write. yoyo when your agent needs to understand or change code it has never seen.

---

Full docs: [`docs/README.md`](./docs/README.md) · [Eval report](./evals/REPORT.md) · [Metrics](./METRICS.md) · [Changelog](./CHANGELOG.md) · Apache 2.0
