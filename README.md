# yoyo

yoyo is a code intelligence MCP server. It gives your AI agent 27 tools to read and edit any codebase — grounded in the AST, not model memory.

**Built for agents.** Drop it into Claude Code, Cursor, or any MCP-compatible agent. The agent calls the tools. You get better answers.

**99% eval accuracy** across 7 real codebases (120 tasks) vs 26% baseline (Claude Code without yoyo). No API keys. No SaaS. No telemetry.

---

## How it works for your agent

```
you run:   yoyo bake --path /your/project
agent gets: 27 tools — search, read, write, rename, trace, analyze
agent uses: supersearch / symbol / flow / patch — not grep, not cat
result:     answers from facts, not memory. no hallucinated file paths.
```

Every read comes from the AST index. Every write auto-reindexes. The agent always sees a fresh, accurate view of your code.

---

## 1. Install

**macOS (Apple Silicon)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-aarch64-apple-darwin.tar.gz | tar xz
sudo mv yoyo-aarch64-apple-darwin /usr/local/bin/yoyo
# macOS: sign the binary or Gatekeeper will kill it silently (exit 137)
codesign --force --deep --sign - /usr/local/bin/yoyo
```

**Linux (x86_64)**
```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv yoyo-x86_64-unknown-linux-gnu /usr/local/bin/yoyo
```

---

## 2. Connect your agent

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

**Cursor** — add the same block to your Cursor MCP config.

**Any MCP-compatible agent** — point it at `yoyo --mcp-server` over stdio.

---

## 3. Index your project

```bash
yoyo bake --path /path/to/your/project
```

Then start a session. The agent calls `llm_instructions` on first contact and picks up the tools automatically.

---

## Tools

### Bootstrap
| Tool | What it does |
|---|---|
| `bake` | Parse the project, write the AST index. Run first. |
| `shake` | Language breakdown, file count, top-complexity functions. |
| `llm_instructions` | Agent bootstrap: tool list, workflows, prime directives. |

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
| `health` | Dead code, god functions, duplicate names. |
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
| `patch` | Write by symbol name, line range, or string match. Auto-reindexes. |
| `patch_bytes` | Write at exact byte offsets. |
| `multi_patch` | N edits across M files in one call. |
| `graph_rename` | Rename a symbol at definition + every call site, atomically. |
| `graph_create` | Create a new file with an initial function scaffold. |
| `graph_add` | Insert a function scaffold into an existing file. |
| `graph_move` | Move a function between files. |
| `graph_delete` | Remove a function by name. Checks blast radius first. |

**Languages:** TypeScript, JavaScript, Rust, Python, Go, C, C++, C#, Java, Kotlin, PHP, Ruby, Swift, Bash

---

Full documentation: [`docs/README.md`](./docs/README.md) · [Eval report](./evals/REPORT.md) · [Changelog](./CHANGELOG.md) · MIT
