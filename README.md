# yoyo

yoyo gives Claude or Cursor 27 tools to read and edit your codebase over MCP. Every answer comes from the AST — not model memory.

**99% eval accuracy** across 7 real codebases (120 tasks) vs 26% baseline. No API keys. No SaaS. No telemetry.

---

## Install

```bash
# macOS Apple Silicon
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-aarch64-apple-darwin.tar.gz | tar xz
sudo mv yoyo-aarch64-apple-darwin /usr/local/bin/yoyo

# Linux x86_64
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv yoyo-x86_64-unknown-linux-gnu /usr/local/bin/yoyo
```

## Connect to Claude or Cursor

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

Then index your project:

```bash
yoyo bake --path /path/to/your/project
```

Claude calls the tools automatically.

---

## Tools

| Tool | What it does |
|---|---|
| `bake` | Parse the project, write the index. Run first. |
| `shake` | Language breakdown, file count, top-complexity functions. |
| `symbol` | Find a function by name — file, line, optionally full body. |
| `slice` | Read any line range from any file. |
| `supersearch` | AST-aware search across all files. |
| `file_functions` | Every function in a file with complexity scores. |
| `blast_radius` | All transitive callers of a symbol + affected files. |
| `trace_down` | Call chain to db/http/queue boundary. Rust + Go. |
| `flow` | Endpoint → handler → call chain in one call. Replaces `api_trace` + `trace_down` + `symbol`. |
| `health` | Dead code, god functions, duplicate names. |
| `package_summary` | Functions, endpoints, complexity for a module path. |
| `architecture_map` | Directory tree with inferred roles. |
| `api_surface` | Exported functions grouped by module. |
| `suggest_placement` | Ranked files to add a new function to. |
| `find_docs` | Locate README, .env, Dockerfile, config files. |
| `all_endpoints` | All detected HTTP routes. |
| `api_trace` | Route path + method → handler function. |
| `crud_operations` | CRUD matrix inferred from routes. |
| `semantic_search` | Find functions by intent. Local ONNX, no API key. |
| `patch` | Write by symbol name, line range, or string match. Auto-reindexes. |
| `patch_bytes` | Write at exact byte offsets. |
| `multi_patch` | N edits across M files in one call. |
| `graph_rename` | Rename a symbol at definition + every call site, atomically. |
| `graph_create` | Create a new file with an initial function scaffold. |
| `graph_add` | Insert a function scaffold into an existing file. |
| `graph_move` | Move a function between files. |
| `graph_delete` | Remove a function by name. |

**Languages:** TypeScript, JavaScript, Rust, Python, Go, C, C++, C#, Java, Kotlin, PHP, Ruby, Swift, Bash

---

Full documentation: [`docs/README.md`](./docs/README.md) · [Eval report](./evals/REPORT.md) · [Changelog](./CHANGELOG.md) · MIT
