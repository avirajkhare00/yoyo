## yoyo – Local Code Intelligence Engine & MCP Server

yoyo (this repo) is a **pure Rust** code‑intelligence engine inspired by the original CartoGopher project:

- Analyzes your project with **Tree‑sitter** and writes a persistent **bake index** to disk.
- Exposes a rich set of **LLM‑friendly tools** via:
  - A **Rust CLI** (`yoyo`) for direct human use.
  - A **Rust MCP server** over stdio for AI assistants (Cursor, Claude Code, etc.).
- Runs entirely locally – **no API keys, no SaaS, no telemetry**.

This implementation currently focuses on **TypeScript / Node.js + Express**, with support for:

- Function indexing (`ts_functions`) with rough complexity.
- Express endpoint detection (`express_endpoints`).
- Repository navigation, API discovery, CRUD matrices, and documentation search on top of the bake index.

The older Go/Node version and its API‑key based setup in `START_HERE.md` and `INSTALL.txt` are **legacy**; this Rust version is the path forward.

---

## Installation

### Prerequisites

- **Rust** (stable, edition 2021)
  Install via `rustup` if you don’t already have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Build the binary

From the repo root:

```bash
cd yoyo
cargo build --release
```

The compiled binary will be at:

```text
yoyo/target/release/yoyo
```

Optionally put it on your `PATH`, e.g.:

```bash
cp target/release/yoyo /usr/local/bin/yoyo
```

---

## CLI Usage

All CLI commands accept an optional `--path` flag pointing to the project root.
If omitted, yoyo uses the current working directory.

### 1. Bake – build the index

```bash
yoyo bake --path /path/to/your/project
```

This:

- Walks the project.
- Detects languages.
- For `.ts` files, builds a Tree‑sitter AST and extracts:
  - Functions (name, file, start/end lines, rough complexity).
  - Express‑style endpoints (`app.get('/foo', handler)`, `router.post(...)`).
- Writes `bakes/latest/bake.json` under the project root.

Almost all other tools assume that `bake` has been run at least once.

### 2. llm‑instructions – prime directive

```bash
yoyo llm-instructions --path /path/to/your/project
```

Returns JSON with:

- Project snapshot (languages, file counts).
- Guidance text for an AI assistant describing the available tools and recommended workflow.

### 3. shake – repository overview

```bash
yoyo shake --path /path/to/your/project
```

If a bake exists, `shake` loads `bakes/latest/bake.json` and returns:

- Languages seen.
- Files indexed.
- Top complex TypeScript functions.
- Sample Express endpoints.

If no bake exists yet, it falls back to a fast directory scan.

### 4. search – fuzzy search for symbols/files

```bash
yoyo search --path /path/to/your/project --q schema --limit 10
```

Searches:

- Baked TypeScript functions (`ts_functions`) by name and file.
- Baked files by path and language.

Returns ranked `function_hits` and `file_hits` in JSON.

### 5. symbol – function lookup

```bash
yoyo symbol --path /path/to/your/project --name generateSchema
```

Returns a list of matching functions with:

- Name, file, start/end lines, complexity.
- Exact matches are ranked ahead of partials.

### 6. slice – read a file region

```bash
yoyo slice \
  --path /path/to/your/project \
  --file src/services/schemaGenerator.ts \
  --start 1 \
  --end 40
```

Returns JSON with:

- `total_lines`.
- The requested `lines` `[start, end]` (1‑based, inclusive).

### 7. api‑surface – exported API by module (TS only)

```bash
yoyo api-surface --path /path/to/your/project --limit 20
yoyo api-surface --path /path/to/your/project --package services --limit 10
```

Groups baked TS functions into “modules” by directory and returns:

- Per‑module lists of functions, sorted by complexity.

### 8. file‑functions – per‑file overview

```bash
yoyo file-functions \
  --path /path/to/your/project \
  --file src/services/schemaGenerator.ts
```

Lists functions in a single file with name, line range, and complexity.

### 9. all‑endpoints – enumerate API routes

```bash
yoyo all-endpoints --path /path/to/your/project
```

Returns all detected Express endpoints from the bake:

- HTTP method, path, file, handler name (when inferable).

### 10. supersearch – text search over TS/JS

```bash
yoyo supersearch \
  --path /path/to/your/project \
  --query prisma \
  --context all \
  --pattern all \
  --exclude-tests
```

Currently line‑oriented and best‑effort (not fully AST‑aware yet), but matches the PRD interface.

### 11. package‑summary – deep dive into a module

```bash
yoyo package-summary \
  --path /path/to/your/project \
  --package services
```

Summarizes:

- Files under matching directories.
- Functions whose file paths contain the package substring.
- Endpoints whose file or path match that package.

### 12. architecture‑map – structure & placement hints

```bash
yoyo architecture-map \
  --path /path/to/your/project \
  --intent "user handler"
```

Provides:

- Directory list with file counts and languages.
- Rough “roles” inferred from path names (e.g. `routes`, `services`, `models`).
- Suggestions for where to place code for the given intent.

### 13. suggest‑placement – where to put new code

```bash
yoyo suggest-placement \
  --path /path/to/your/project \
  --function-name createUserHandler \
  --function-type handler \
  --related-to user
```

Returns candidate files with scores and rationales based on:

- Function type (`handler | service | repository | model | util | test`).
- Path heuristics and optional `related_to` substring.

### 14. crud‑operations – entity‑level CRUD matrix

```bash
yoyo crud-operations --path /path/to/your/project
yoyo crud-operations --path /path/to/your/project --entity user
```

Infers entities from endpoint paths (e.g. `/users`, `/users/:id`) and classifies:

- `create`, `read`, `update`, `delete` operations with method, path, and file.

### 15. api‑trace – follow an endpoint through handlers

```bash
yoyo api-trace \
  --path /path/to/your/project \
  --endpoint /users \
  --method GET
```

Returns matching Express endpoints for that path/method with handler info.

### 16. find‑docs – documentation discovery

```bash
yoyo find-docs --path /path/to/your/project --doc-type readme
yoyo find-docs --path /path/to/your/project --doc-type all
```

Searches for:

- `readme | env | config | docker | all` and returns paths with a short snippet.

### 17. patch – apply a line‑range patch

```bash
yoyo patch \
  --path /path/to/your/project \
  --file src/example.ts \
  --start 10 \
  --end 20 \
  --new-content $'// new content\nconsole.log(\"hello\");'
```

Safely replaces the specified `[start, end]` line range in a file and writes it back to disk.

---

## MCP Usage

yoyo can also run as an **MCP server** over stdio, exposing the same tools to AI assistants.

### 1. Basic MCP config (Cursor)

Assuming you’ve built the binary as described above and it’s available at `/path/to/yoyo/target/release/yoyo`, a minimal Cursor MCP config is:

```json
{
  "mcpServers": {
    "yoyo": {
      "type": "stdio",
      "command": "/path/to/yoyo/target/release/yoyo",
      "args": ["--mcp-server"],
      "env": {
        "CURSOR_WORKSPACE": "${workspaceFolder}"
      }
    }
  }
}
```

- `--mcp-server` switches the binary into MCP mode (JSON‑RPC 2.0 over stdin/stdout).
- `CURSOR_WORKSPACE` tells yoyo which project root to analyze.

### 2. Tools exposed over MCP

When running in MCP mode, `list_tools` advertises tools matching the CLI surface, including:

- `llm_instructions`, `shake`, `bake`
- `search`, `symbol`, `slice`, `supersearch`
- `api_surface`, `file_functions`, `package_summary`
- `architecture_map`, `suggest_placement`
- `all_endpoints`, `api_trace`, `crud_operations`
- `find_docs`, `patch`

Each tool accepts a JSON arguments object mirroring the CLI flags and returns JSON text content suitable for direct model consumption.

---

## Contributing

### Project layout

- `yoyo/src/main.rs` – binary entrypoint, CLI vs MCP switch.
- `cartogopher-rs/src/cli.rs` – human‑facing CLI (clap).
- `yoyo/src/engine.rs` – core “query” functions backing all tools.
- `yoyo/src/mcp.rs` – minimal MCP JSON‑RPC server.
- `yoyo/src/ts_index.rs` – TypeScript/Express indexing using Tree‑sitter.
- `prd.md` – product requirements and intended tool surface.

### Development workflow

```bash
cd yoyo

# Fast feedback while editing
cargo check

# Run tests (once tests are added)
cargo test

# Optional: format + lint
cargo fmt
cargo clippy
```

To exercise the tools during development, it’s often easiest to point them at a real TS/Express project, e.g.:

```bash
cargo run -- bake --path /path/to/example-project
cargo run -- shake --path /path/to/example-project
cargo run -- all-endpoints --path /path/to/example-project
```

### Adding a new tool

1. **Engine**
   - Add a new function in `engine.rs` (e.g. `pub fn my_tool(...) -> Result<String>`).
   - Implement it purely in terms of:
     - `resolve_project_root`, `load_bake_index`, and the `BakeIndex` structure, or
     - Direct filesystem/Tree‑sitter analysis if it doesn’t need the bake.
   - Return a **JSON string** built from a serializable payload struct.

2. **CLI**
   - Add a subcommand to `Command` in `cli.rs` with a corresponding `Args` struct.
   - Implement a small `run_my_tool` function that:
     - Parses CLI flags.
     - Calls `crate::engine::my_tool(...)`.
     - Prints the returned JSON.

3. **MCP**
   - Add an entry to `list_tools()` in `mcp.rs` with the tool name and `inputSchema`.
   - Add a `match` arm in `call_tool` that:
     - Extracts arguments from `Value`.
     - Calls `crate::engine::my_tool(...)`.
     - Wraps the JSON in MCP `content` (`[{ "type": "text", "text": json }]`).

4. **Docs**
   - Update this `README.md` and/or `prd.md` with a short description of the new tool.

### Style & guidelines

- Prefer **small, composable engine functions** that operate on `BakeIndex` and plain data types.
- Keep all JSON I/O concerns in `cli.rs` / `mcp.rs`; the engine should just return `Result<String>`.
- Avoid adding new mandatory external services or environment variables; keep the engine **fully local**.

---

## TODO / Roadmap

- **Tooling parity with PRD**
  - Implement missing tools: `related_to`, `frontend` (and its sub-modes).
  - Deepen partially implemented tools to match specs:
    - `supersearch` as truly AST/context/pattern-aware.
    - `all_endpoints` with backend + frontend usage and `include_backend` / `include_frontend` flags.
    - `api_trace` that follows requests end-to-end (frontend → handler → CRUD/data layer).
    - `crud_operations` using both HTTP and DB patterns.
    - Richer `symbol`, `api_surface`, `file_functions` outputs (signatures, docs, relations).

- **Bake model & language coverage**
  - Extend the bake beyond TS/Node + Express to more languages from the PRD.
  - Persist a proper global symbol index, call graph (`calls` / `called_by`), endpoint index, and frontend index (components/hooks/props).

- **Configuration & environment**
  - Support a simple config file (`fast.yaml` / `yoyo.yaml`) for languages, excludes, heuristics.
  - Wire in additional env/root conventions from the PRD (e.g. CartoGopher-style root env vars) where they make sense.

- **Performance, tests, and polish**
  - Add unit/integration tests and basic performance baselines on representative TS/Express repos.
  - Implement incremental / cached baking and parallelism tuning for large codebases.
  - Extend CI to build multi-platform binaries (macOS/Windows), finalize versioning, and choose a concrete OSS license.

---

## License

TBD – add your preferred license here (e.g. MIT, Apache‑2.0) before publishing the project publicly.
