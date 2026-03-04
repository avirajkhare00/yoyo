# yoyo – Local Code Intelligence Engine

**yoyo** is a pure-Rust code-intelligence engine and MCP server that indexes your TypeScript, JavaScript, Rust, and Python projects with Tree-sitter and exposes 17 LLM-ready tools over CLI and stdio.

No API keys. No SaaS. No telemetry. Your code stays on your machine.

---

## Why yoyo?

| | Without yoyo | With yoyo |
|---|---|---|
| Onboard to an unfamiliar codebase | ~15–20 min of `ls`, grep, file-by-file reading | ~30 seconds across 8 parallel tool calls |
| Find where a function is defined | Search IDE, scroll, guess filenames | `yoyo symbol --name myFn` → file + line range |
| Trace usage of a function across 386 files | Hours with grep | `yoyo supersearch --query detectAllFaces` → instant |
| Understand a module's public API | Open every file in the module | `yoyo package_summary --package services` → done |
| Know where to add new code | Team discussion, guesswork | `yoyo suggest_placement --function-name sendEmail` |

**Benchmarked on real codebases: 30–40× faster onboarding than manual methods.**

---

## Key strengths

**`shake` — the killer onboarding command.**
Top complex functions, language breakdown, and project structure in one call. No reading required.

**`supersearch` — whole-codebase search in seconds.**
Traced every usage of `detectAllFaces` across a 386-file ML library in a single query.

**`symbol` + `slice` — pinpoint any function.**
Find a 260-line function by name, then read it verbatim. Use `symbol --include-source` to get the
function body inline in one call; no second `slice` needed. No IDE, no scrolling.

**`package_summary` — full module deep-dive.**
Got the complete MTCNN 3-stage pipeline (PNet → RNet → ONet) without opening a single file.

**MCP server — grounds AI in real code.**
Connect to Claude Code, Cursor, or any MCP-compatible assistant. The AI calls your tools instead of hallucinating structure.

**Fully local.**
The bake index is a plain JSON file written inside your project. Nothing is sent anywhere.

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

Once configured, your AI assistant can call **all 17 yoyo tools as MCP methods**. Each method takes a `path` to the project (defaults to the current workspace in most editors) plus a few tool-specific arguments:

| MCP tool | Typical call | What it does |
|---|---|---|
| `bake` | `bake(path)` | Build the Tree-sitter index for the project. Must run before most other tools. |
| `shake` | `shake(path)` | High-level repo overview: languages, file counts, top-complex functions, endpoints. |
| `search` | `search(path, q)` | Fuzzy search over function names and file paths. Great for “where is X defined?”. |
| `symbol` | `symbol(path, name, include_source)` | Find a function by name, returning file, line range, and optionally the full body inline. |
| `slice` | `slice(path, file, start, end)` | Read an exact line range from a file for precise code inspection. |
| `supersearch` | `supersearch(path, query)` | AST-aware search across TypeScript, Rust, and Python source files. |
| `file_functions` | `file_functions(path, file)` | List all functions in a file with complexity scores. |
| `api_surface` | `api_surface(path, package)` | Exported/public API grouped by module; omit `package` for a project-wide view. |
| `package_summary` | `package_summary(path, package)` | Deep-dive a module: files, functions, endpoints, and complexity. |
| `architecture_map` | `architecture_map(path, intent)` | Directory tree with role hints based on file names and an optional intent string. |
| `suggest_placement` | `suggest_placement(path, function_name, function_type, related_to)` | Scored candidates for where new code should live, given a name/type and an optional related symbol. |
| `all_endpoints` | `all_endpoints(path)` | List all detected HTTP endpoints (Express/Actix/Flask/FastAPI). |
| `api_trace` | `api_trace(path, endpoint, method)` | Trace a single route through its handler for a given path + HTTP method. |
| `crud_operations` | `crud_operations(path, entity)` | Inferred CRUD matrix from routes, optionally filtered to a specific entity. |
| `find_docs` | `find_docs(path, doc_type)` | Locate READMEs, `.env`, Dockerfiles, and other config/docs (`doc_type` can be `readme`, `env`, `config`, `docker`, or `all`). |
| `patch` | `patch(path, ...)` | Apply structured edits by symbol (`symbol` + `new_content`) or by file + line range. |
| `llm_instructions` | `llm_instructions(path)` | Return a compact JSON “prime directive” with guidance on how assistants should use yoyo. |

In Claude, Cursor, and other MCP-aware tools, you typically don't call these methods manually — the assistant selects and calls them as needed to ground its answers in your actual code.

---

## Tool reference

All commands accept `--path /path/to/project` (defaults to current directory).

| Tool | Command | What it does |
|---|---|---|
| Bake | `yoyo bake` | Build Tree-sitter index → `bakes/latest/bake.json` |
| Shake | `yoyo shake` | Overview: languages, top complex functions, endpoints |
| Search | `yoyo search --q <term>` | Fuzzy search over function names and file paths |
| Symbol | `yoyo symbol --name <fn> [--include-source]` | Find a function by name → file + line range (optionally include body inline) |
| Slice | `yoyo slice --file <f> --start <n> --end <n>` | Read an exact line range |
| Supersearch | `yoyo supersearch --query <term>` | AST-aware search across TypeScript, Rust, and Python files |
| File functions | `yoyo file-functions --file <f>` | List functions in a file with complexity |
| API surface | `yoyo api-surface [--package <pkg>]` | Exported functions grouped by module |
| Package summary | `yoyo package-summary --package <pkg>` | Deep-dive a module: files, functions, endpoints |
| Architecture map | `yoyo architecture-map [--intent <desc>]` | Directory tree with role hints |
| Suggest placement | `yoyo suggest-placement --function-name <fn> --function-type <type>` | Scored candidates for placing new code |
| All endpoints | `yoyo all-endpoints` | List all detected Express routes |
| API trace | `yoyo api-trace --endpoint <path> --method <GET\|POST\|…>` | Trace a route through its handler |
| CRUD operations | `yoyo crud-operations [--entity <name>]` | CRUD matrix inferred from routes |
| Find docs | `yoyo find-docs --doc-type <readme\|env\|config\|docker\|all>` | Locate config and documentation files |
| Patch | `yoyo patch --symbol <name> --new-content <text>` or `--file <f> --start <n> --end <n> --new-content <text>` | Replace by symbol name (from index) or by line range |
| LLM instructions | `yoyo llm-instructions` | Prime directive JSON for AI assistants |

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

## Known limitations (current version)

- **Express/Actix/Flask routes only** — `api_trace` and `crud_operations` detect Express (TS), Actix/Rocket (Rust), and Flask/FastAPI (Python) routes. NestJS decorators, Fastify, and dynamic routers are not supported.
- **Large project output** — `find_docs` and `api_surface` on 300+ file projects can exceed LLM context limits. Use `--package` or `--limit` to filter.
- **No call graph** — `api_trace` cannot follow call chains deeper than the route handler itself.

---

## Project layout

```
src/
  main.rs           binary entrypoint, CLI vs MCP switch
  cli.rs            human-facing CLI (clap)
  engine.rs         core query functions backing all tools
  mcp.rs            MCP JSON-RPC server over stdio
  lang/
    mod.rs          LanguageAnalyzer trait, shared AST helpers
    typescript.rs   TypeScript/Express indexing
    rust.rs         Rust/Actix/Rocket indexing
    python.rs       Python/Flask/FastAPI indexing
```

---

## License

MIT — see [LICENSE](./LICENSE).
