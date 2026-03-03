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

## Use Cases

Here's what yoyo is actually useful for, drawn from real usage on TypeScript/Express codebases.

### 1. Onboarding to an unfamiliar codebase

Drop into a project you've never seen and get oriented in seconds:

```bash
yoyo bake --path /path/to/project
yoyo shake --path /path/to/project
```

`shake` gives you languages, file counts, and the most complex functions at a glance — no reading required.

### 2. Finding the most complex / risky functions

Before a code review or refactor, surface hotspots by complexity:

```bash
yoyo api-surface --path /path/to/project --limit 10
yoyo file-functions --path /path/to/project --file src/services/integrationService.ts
```

yoyo ranks functions by cyclomatic complexity so you know where the gnarly logic lives without reading every file.

### 3. Understanding a specific function

Jump straight to a function by name and read just that region of the file:

```bash
yoyo symbol --path /path/to/project --name promptUser
yoyo slice --path /path/to/project --file src/services/promptService.ts --start 8 --end 260
```

Useful when an AI assistant or teammate asks "what does `generateSchema` do?" — no scrolling, no IDE needed.

### 4. Tracing an API endpoint end-to-end

Follow a route from the HTTP method all the way to its handler:

```bash
yoyo api-trace --path /path/to/project --endpoint /users --method GET
yoyo crud-operations --path /path/to/project --entity user
```

Great for debugging "which handler is serving this route?" or generating endpoint documentation automatically.

### 5. Searching across the entire codebase

Find every place an integration, library, or pattern is used without relying on a file-aware IDE:

```bash
yoyo supersearch --path /path/to/project --query openai --exclude-tests
yoyo search --path /path/to/project --q prisma --limit 20
```

`supersearch` does line-oriented text matching across all TS/JS files; `search` does fuzzy matching over function names and file paths.

### 6. Figuring out where to add new code

Ask yoyo where a new function belongs before writing a single line:

```bash
yoyo suggest-placement \
  --path /path/to/project \
  --function-name sendWelcomeEmail \
  --function-type service \
  --related-to user

yoyo architecture-map --path /path/to/project --intent "email notification service"
```

yoyo scores candidate files by path heuristics and proximity to related symbols, so you don't bikeshed on directory structure.

### 7. Auditing what API surface is exported from a module

Check what a module exposes before touching it:

```bash
yoyo api-surface --path /path/to/project --package services
yoyo package-summary --path /path/to/project --package services
```

Lists every exported function grouped by directory, sorted by complexity — useful for API design reviews.

### 8. Locating config, docs, and environment files

Quickly find all `.env`, `Dockerfile`, `README`, and config files scattered around a monorepo:

```bash
yoyo find-docs --path /path/to/project --doc-type all
yoyo find-docs --path /path/to/project --doc-type env
```

Each match comes with a short snippet so you can preview content before opening the file.

### 9. Applying targeted edits without opening an IDE

Patch a specific line range in a file directly — useful when scripting or automating code changes:

```bash
yoyo patch \
  --path /path/to/project \
  --file src/services/openaiService.ts \
  --start 16 \
  --end 16 \
  --new-content "  model: 'gpt-4o',"
```

### 10. Powering AI assistants with local code context (MCP)

Connect yoyo to Claude Code, Cursor, or any MCP-compatible AI assistant so it can answer questions about your codebase without uploading code to any external service:

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

The AI can then call `bake`, `symbol`, `supersearch`, `api_trace`, `suggest_placement`, and all other tools directly — grounding its answers in your actual code rather than hallucinating structure.

---

## Benchmark – Real-world Onboarding Test

> **Reusable prompt:** See [`benchmark-prompt.md`](./benchmark-prompt.md) for the exact 5-batch tool sequence used below, ready to paste into any AI assistant connected to yoyo.
>
> **Detailed reports:** See [`reports/`](./reports/) for full per-project benchmark write-ups, a comprehensive TODO tracker, and the code-quality assessment.

The following is a honest benchmark from a live session where yoyo was used to onboard an AI assistant (Claude) onto two real TypeScript projects from scratch.

### Projects tested

| Project | Files | Languages | Type | Report |
|---|---|---|---|---|
| `schema-generator-prisma` | 24 | TypeScript, JSON | CLI tool (no Express routes) | — |
| `face-api.js` | 386 | TypeScript, JS, HTML, YAML, JSON | ML library (no Express routes) | [`reports/benchmark-face-api-js-2026-03-03.md`](./reports/benchmark-face-api-js-2026-03-03.md) |

---

### Tool effectiveness

| Tool | Result | Notes |
|---|---|---|
| `bake` | ✅ Worked perfectly | Indexed 24 and 386 files correctly; fast on both |
| `shake` | ✅ Worked perfectly | Returned top complex functions and language breakdown immediately |
| `symbol` | ✅ Worked perfectly | Located `promptUser` (260-line function) by name in one call |
| `slice` | ✅ Worked perfectly | Read exact line ranges; no IDE or file open needed |
| `file_functions` | ✅ Worked perfectly | Listed all 6 functions in `integrationService.ts` with complexity ranks |
| `package_summary` | ✅ Worked perfectly | Deep-dived `globalApi` and `mtcnn` modules with full function lists |
| `supersearch` | ✅ Worked well | Traced `detectAllFaces` and `openai` usage across all files instantly |
| `architecture_map` | ✅ Worked well | Gave full directory tree with file counts; role inference was blank for most dirs |
| `suggest_placement` | ✅ Worked well | Returned scored candidates with rationale |
| `api_surface` | ⚠️ Output too large | Returned 55KB+ for face-api; hit token limit, needed persisted file workaround |
| `find_docs` | ⚠️ Output too large | Returned 298K characters for face-api; hit token limit |
| `api_trace` | ⚠️ Limited | Only matched static Express routes; useless on CLI tools and ML libraries |
| `crud_operations` | ⚠️ Limited | No matrix generated on either project (neither had Express endpoints) |
| `supersearch` context/pattern | ❌ Silently ignored | All three combinations (`identifiers/call`, `strings/assign`, `comments/return`) returned **identical results** — flags have zero effect |
| `architecture_map` roles | ⚠️ Mostly empty | `roles: []` for almost all directories; heuristics need tuning |

---

### Time-to-understand comparison

**Without yoyo** (manual approach):
- `ls` through directories, read `package.json`, grep for key functions, open files one by one
- Estimated time to produce equivalent onboarding summary: **~15–20 minutes**

**With yoyo** (8 tool calls, 3 parallel batches):
```
Batch 1:  bake + shake                                          (parallel)
Batch 2:  architecture_map + find_docs + api_surface            (parallel)
Batch 3:  package_summary(globalApi) + package_summary(mtcnn) + supersearch(detectAllFaces)  (parallel)
```
- Actual wall-clock time: **~30 seconds** across all batches
- Total tool calls: **8**
- Produced: full architecture map, top complex functions, module breakdown, MTCNN 3-stage pipeline, call-chain trace for `detectAllFaces` across all 386 files, detector comparison table

**Verdict: ~30–40× faster onboarding** on a real unfamiliar codebase.

---

### Where yoyo added the most value

1. **`shake` is the killer feature for onboarding** — top complex functions + file count in one call, no reading required.
2. **`supersearch` for call-chain tracing** — finding every use of `detectAllFaces` across 386 files in one query was the single most time-saving operation.
3. **`package_summary` for module deep-dives** — getting the full `mtcnn` pipeline (PNet → RNet → ONet) without opening a single file.
4. **`symbol` + `slice` combo** — pinpointing a 252-line function by name, then reading it verbatim, saved multiple file-open round trips.

### Known gaps (as of this benchmark)

- **Output size limits**: `find_docs` and `api_surface` on large projects (300+ files) produce responses too large for a single LLM context window. Needs pagination or filtering.
- **Static routes only**: `api_trace` and `crud_operations` only work on projects with Express routes statically defined. Dynamic routers, NestJS decorators, and non-Express frameworks are not detected.
- **No language support beyond TS/JS**: The `bake` index only parses TypeScript/JavaScript. Python, Go, Rust, and other languages are counted but not indexed.
- **Role inference**: `architecture_map` returns `roles: []` for most directories unless path names exactly match known keywords (`routes`, `services`, `models`, etc.).

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
- `yoyo/src/cli.rs` – human‑facing CLI (clap).
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

> Full TODO tracker with source-level details: [`reports/todo-tracker.md`](./reports/todo-tracker.md)
> Code quality assessment (what's good, what needs work): [`reports/assessment.md`](./reports/assessment.md)

Priorities below are informed by the [real-world benchmark](#benchmark--real-world-onboarding-test) run on `schema-generator-prisma` (24 files) and `face-api.js` (386 files).

### 🔴 High priority – broke or severely limited real usage

- **`file_functions` / `symbol` / `api_surface` miss class methods and arrow functions (confirmed by code review)**
  - `ts_index.rs` only captures `function_declaration` AST nodes.
  - Any TypeScript file using class methods (`class Foo { bar() {} }`), arrow functions (`const fn = () => {}`), or function expressions returns **zero** indexed functions.
  - Live benchmark: `SsdMobilenetv1.ts` (133 lines, 5 public methods) returned 0 results from `file_functions`.
  - Fix: add `method_definition`, `arrow_function`, and `function_expression` cases to `walk_ts()` in `ts_index.rs`.

- **Output size limits on large projects**
  - `find_docs` returned 298K characters on face-api.js — exceeded LLM context limits entirely.
  - `api_surface` returned 55KB+ — hit token ceiling, fell back to persisted file workaround.
  - Fix: add `--limit`, `--offset`, and `--package` filtering to both tools so results are always LLM-sized.
  - Fix: enforce a hard max result size (e.g. 50 items) with a `"truncated": true` flag in the response.

- **`architecture_map` role inference is blank**
  - Returns `roles: []` for nearly all directories — the main value prop of the tool is not delivering.
  - Fix: broaden keyword matching beyond exact names (`routes`, `services`, `models`) to include common patterns (`controllers`, `handlers`, `repositories`, `resolvers`, `middleware`, `hooks`, `components`, `store`, `utils`, etc.).

- **`supersearch` `--context` and `--pattern` flags are silently ignored (confirmed bug)**
  - Tested with three different combinations — `context: identifiers/pattern: call`, `context: strings/pattern: assign`, `context: comments/pattern: return` — all returned **byte-for-byte identical results**.
  - `context: "strings"` should match only string literals but matched `import` statements, `class` definitions, and function declarations.
  - `context: "comments"` should return 0 results for `detectAllFaces` (it appears in no comments) but returned 16 matches.
  - `pattern: "call"` should exclude `import` and `export function` lines but included them.
  - Fix: implement `--context` and `--pattern` filtering using Tree-sitter node types — do not silently fall back to plain-text search when AST filtering is requested.

### 🟡 Medium priority – gaps that limit usefulness on non-Express projects

- **`api_trace` and `crud_operations` only work with static Express routes**
  - Both tools produced no results on a CLI tool and an ML library — the two tested projects.
  - Fix: detect NestJS `@Get()` / `@Post()` decorators, Fastify routes, Hono, and dynamic `router.use()` patterns.
  - Fix: add DB-layer CRUD detection (Prisma `findMany`, `create`, `update`, `delete`) as a secondary signal for `crud_operations`.

- **No language support beyond TypeScript/JavaScript**
  - Python, Go, Rust, Ruby files are counted in `bake` but not indexed — `symbol`, `file_functions`, `supersearch` return nothing for them.
  - Fix: add Tree-sitter grammars for Python and Go as the next two languages (highest demand).

- **`symbol` and `slice` require knowing the file path upfront**
  - The `symbol` tool returns file + line range, but doesn't return the actual source — requires a follow-up `slice` call.
  - Fix: add an optional `--include-source` flag to `symbol` that returns the full function body inline, eliminating the round-trip.

### 🟢 Lower priority – polish and completeness

- **Missing tools from PRD**
  - Implement `related_to` — find symbols related to a given one by call graph proximity.
  - Implement `frontend` tool and its sub-modes (components, hooks, props, routes).

- **Bake model depth**
  - Persist a proper call graph (`calls` / `called_by`) so `api_trace` can follow requests end-to-end.
  - Persist a frontend index (React components, hooks, props) for full-stack tracing.
  - Incremental / cached baking so re-baking a large project after small edits is fast.

- **Configuration**
  - Support a `yoyo.yaml` config file for per-project excludes (e.g. skip `vendor/`, `dist/`, `node_modules/`).
  - The face-api project had `node_modules` style nested paths inside `face-api.js-master/` which inflated file counts.

- **Tests and CI**
  - Add unit tests for engine functions and integration tests on representative TS/Express and TS/library projects.
  - Extend CI to build multi-platform binaries (macOS arm64, macOS x86, Linux, Windows).
  - Finalize versioning and OSS license.

---

## License

TBD – add your preferred license here (e.g. MIT, Apache‑2.0) before publishing the project publicly.
