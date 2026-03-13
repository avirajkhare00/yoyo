<p align="center">
  <img src="logo.svg" width="80" height="96" alt="yoyo logo"/>
</p>

# yoyo

**Grounded codebase answers for AI coding agents.**

yoyo is a local MCP server for repository reading and change work. It exists to make coding agents less hallucinated, more grounded, and more truthful when they answer questions about a real codebase.

The core product is not generic search. It is a smaller and more reliable interface to the repository:

- `judge_change` for ownership, invariants, and regression risk before edits
- `inspect` for cheap structured reads like signatures, type surfaces, file structure, and exact excerpts
- `change` for error-bounded writes through one task-shaped surface

Essays:

- [Why Recursive Language Models point in the same direction as yoyo](./docs/rlm-and-yoyo.html)
- [How we designed the yoyo eval harness](./docs/harness-design.html)

## Current status

Headline WIP result: in one clean directed ripgrep `read_only` eval, Codex used `yoyo` for `22/22` tool calls.

That run asked three explicit engineer questions: find the `3` most likely files or symbols, decide which layer should own the fix, and state the invariants and blast radius. `yoyo` localized the bug to `hiargs.rs`, `walk.rs`, and `gitignore.rs`, placed ownership in `crates/ignore`, and surfaced the key invariants and regression risks without making edits.

Write-side WIP result: `yoyo` is strongest when read judgment narrows the surface first, then `change` executes the write cleanly. The first clean directed `write_only` batch now shows that on `ripgrep`, `uuid`, and `httprouter`. `semver` is tracked separately: the patch in `src/eval.rs` is correct and passes manual `cargo test`, but the fixture's exact verify command, `cargo test --test *`, is malformed and still needs fixing before strict scoring.

This is groundedness and tool-use evidence under direction, not yet a broad with-vs-without benchmark. The old `119/120` tool-accuracy benchmark still exists as a legacy regression report, and the current compare smoke runs are still `6/6` ties across `v1.8.5` and `v1.7.3`.

Current directed artifacts:

- [`evals/results/directed-ripgrep-read-only-2026-03-13.md`](./evals/results/directed-ripgrep-read-only-2026-03-13.md)
- [`evals/results/directed-ripgrep-write-only-2026-03-13.md`](./evals/results/directed-ripgrep-write-only-2026-03-13.md)
- [`evals/results/directed-write-batch-2026-03-13.md`](./evals/results/directed-write-batch-2026-03-13.md)
- [`evals/results/directed-semver-write-only-2026-03-13.md`](./evals/results/directed-semver-write-only-2026-03-13.md)

See [`evals/README.md`](./evals/README.md) for the current eval direction.

## Why it exists

Coding agents are strong at local editing and weak at repository truth. They guess ownership layers, invent file paths, over-read source files, and lose the actual invariants of the system.

yoyo narrows that gap. It gives the model:

- a grounded repository index in `bakes/latest/bake.db`
- a judgment surface before edits
- a cheaper read surface for signatures and types
- a structured write path when direct file mutation is the wrong tool

The point is not to make toy tasks look slightly faster. The point is to make answers more truthful and more grounded in the code that actually exists.

## Language focus

yoyo is opinionated about depth. The primary languages are:

- Rust
- Go
- Zig
- TypeScript

Rust and Go currently have the strongest read surface, including indexed structured signatures in `inspect`.

## Install

macOS (Apple Silicon):

```bash
brew tap avirajkhare00/yoyo
brew install yoyo
```

Linux (x86_64):

```bash
curl -L https://github.com/avirajkhare00/yoyo/releases/latest/download/yoyo-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv yoyo-x86_64-unknown-linux-gnu /usr/local/bin/yoyo
```

Check:

```bash
yoyo --version
```

## Add as MCP

Claude Code or Cursor:

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

Codex CLI:

```bash
codex mcp add yoyo -- /usr/local/bin/yoyo --mcp-server
```

Gemini CLI:

```bash
gemini mcp add yoyo /usr/local/bin/yoyo --mcp-server
```

Then index the project once:

```bash
yoyo index --path /path/to/your/project
```

## Teach the agent to use it

For Codex, add this to `AGENTS.md`:

```md
## yoyo
Call `boot` and `index` first.
Prefer `inspect` for code reads, `search` over grep, and `change` for code changes.
Prefer `judge_change` before edits when ownership, invariants, or regression risk are unclear.
Prefer `impact` for relation and blast-radius questions.
```

Without this, the agent may see yoyo and still fall back to its default habits.

## MCP tools

yoyo currently exposes 13 MCP tools:

- Bootstrap: `boot`, `index`
- Discovery: `help`
- Read: `inspect`, `search`, `ask`
- Judge: `judge_change`
- Relate: `map`, `impact`, `routes`, `health`
- Write: `change`
- Orchestration: `script`

The MCP surface is intentionally smaller than the CLI surface. Humans still have broader CLI commands for debugging and direct use.

## Why not just LSP

LSP is for humans inside an editor. yoyo is for agents working over MCP.

- LSP is cursor- and file-oriented
- yoyo is repository-oriented
- LSP does not answer ownership or blast-radius questions
- yoyo is designed to answer those questions before the edit happens

Use both. LSP while you write. yoyo when your agent needs to understand or change code it has never seen.

## Contributors

- [Aviraj Khare](https://github.com/avirajkhare00) — [X](https://x.com/avirajkhare00)
- [Saurav Kumar](https://github.com/sauravtom) — [X](https://x.com/hackposthq)

## Links

- [Full docs](./docs/README.md)
- [Eval strategy](./evals/README.md)
- [Legacy eval report](./evals/REPORT.md)
- [Metrics](./METRICS.md)
- [Changelog](./CHANGELOG.md)
- Apache 2.0
