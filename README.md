<p align="center">
  <img src="logo.svg" width="80" height="96" alt="yoyo logo"/>
</p>

# yoyo

<a href="https://peerlist.io/avirajkhare00/project/yoyo" target="_blank" rel="noreferrer">
				<img
					src="https://peerlist.io/api/v1/projects/embed/PRJHDNDNMEQQ6O87M2NQQKM9BO8JNG?showUpvote=true&theme=dark"
					alt="YoYo"
					style="width: auto; height: 72px;"
				/>
			</a>

**Grounded codebase answers for AI coding agents.**

yoyo is an MCP server that gives your agent a curated, task-shaped set of AST-grounded tools to read, understand, and edit code. Less hallucination. More grounded answers. Facts from the source.

**99% eval accuracy** across 4 languages, 8 real codebases — vs 26% baseline (Claude Code alone).

---

## Why

Your AI agent reads code like a human with no IDE: grep, cat, hope. It hallucinates function names. It misses callers. It patches the wrong file.

yoyo gives it what it was missing: a structured interface to the codebase. The agent calls `judge_change` to answer ownership, invariants, and regression-risk questions before it edits. It calls `inspect` instead of raw file reads. It calls `impact` before deleting or renaming. It edits through `change`, the error-bounded write surface, not line-number roulette.

The point is not to make trivial tasks look marginally faster. The point is to make answers more truthful and more grounded in the code that actually exists.

---

## Language focus

> **Rust · Go · Zig · TypeScript — four languages, done deep.**

| Language | index | inspect | impact trace | routes | change |
|---|---|---|---|---|---|
| Rust | ✅ | ✅ | ✅ | ✅ actix/rocket | ✅ |
| Go | ✅ | ✅ | ✅ | ✅ gin/echo/net-http | ✅ |
| Zig | ✅ | ✅ | — | — | ✅ |
| TypeScript | ✅ | ✅ | partial | ✅ express | ✅ |

Not every language. The four where systems-level code intelligence matters most.

---

## The combinations are the point

One trick is fine. Fifty moves chained is transcendent.

| Combination | What it does |
|---|---|
| `search` → `inspect` → `change` | find it, read it, change it |
| `judge_change` → `inspect` → `change` | decide where the fix belongs, confirm it, patch it safely |
| `impact` → `health` → `change` | what breaks if I touch this? is it dead? change it safely |
| `impact` → `change` | trace the full request path, fix it end-to-end in one shot |
| `index` → `ask` → `map` | where does this new function belong? |
| `map` → `routes` → `change` | understand the shape, find the gap, fill it |

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
            "command": "echo '[yoyo] Use mcp__yoyo__search instead of Grep. Use mcp__yoyo__inspect for code reads. Use mcp__yoyo__impact for caller and route tracing. Use mcp__yoyo__change for code changes.'"
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
Call `boot` and `index` first.
Prefer `search` over grep, `inspect` for code reads, `change` for code changes.
Prefer `impact` for relation/trace questions.
Prefer `judge_change` for ownership, invariants, and regression-risk questions before edits.
```

Without this, your agent sees yoyo but won't reach for it first.

---

## Tools (13 MCP tools)

### Bootstrap
| Tool | What it does |
|---|---|
| `boot` | Lean bootstrap: tool names grouped by category, task-shaped capability families, common-task recommendations, and concurrency rules. Call first. |
| `index` | Parse the project, write the AST index. Run before any read-indexed tool. |
| `help` | Progressive discovery: params, output shape, example, and limitations for any tool. |

### Locate
| Tool | What it does |
|---|---|
| `inspect` | Inspect a symbol, file outline, or line range from one entrypoint. |
| `search` | AST-aware search across all files. Replaces grep. |
| `ask` | Find functions by intent. Local ONNX embeddings, no API key. |

### Judge
| Tool | What it does |
|---|---|
| `judge_change` | High-level read surface for ownership, candidate symbols/files, invariants, regression risks, and verification commands before editing. |

### Relate
| Tool | What it does |
|---|---|
| `map` | Directory tree with inferred roles. |
| `impact` | Task-shaped impact analysis for a symbol or endpoint. |
| `routes` | All detected HTTP routes. |
| `health` | Dead code, large functions, duplicate names. |

### Write
| Tool | What it does |
|---|---|
| `change` | Task-shaped write entrypoint over edit, bulk_edit, rename, move, delete, create, and add. |

### Orchestration
| Tool | What it does |
|---|---|
| `script` | Run a Rhai script over the same task-shaped yoyo functions exposed in MCP. |

### CLI-only mechanisms

These remain available in `yoyo <command>` for humans, but are not exposed through MCP:

`read`, `symbol`, `outline`, `flow`, `callers`, `edit`, `bulk_edit`, `rename`, `create`, `add`, `move`, `delete`

CLI still exposes broader engine capabilities for humans and debugging. MCP stays intentionally small and task-first.

---

## Why not just LSP?

LSP is for humans in an editor. yoyo is for AI agents understanding codebases.

| | LSP | yoyo |
|---|---|---|
| Consumer | Editor (VS Code, Neovim…) | AI agent (Claude, Codex, Cursor…) |
| Protocol | JSON-RPC to editor buffers | MCP stdio — agent calls tools directly |
| Scope | Per-file, cursor-aware | Whole codebase in one call |
| Setup | One server per language | One binary for all languages |
| "Where should new code go?" | No equivalent | `map` + `ask` + `change` |
| Edit by intent | No equivalent | `change` |

Use both. LSP while you write. yoyo when your agent needs to understand or change code it has never seen.

---

## Contributors

- [Aviraj Khare](https://github.com/avirajkhare00) — [X](https://x.com/avirajkhare00)
- [Saurav Kumar](https://github.com/sauravtom) — [X](https://x.com/hackposthq)

---

Full docs: [`docs/README.md`](./docs/README.md) · [Eval report](./evals/REPORT.md) · [Metrics](./METRICS.md) · [Changelog](./CHANGELOG.md) · Apache 2.0
