# yoyo — Metrics

Single source of truth for yoyo's measurable state. Updated every release.

---

## Current (v1.5.2 — 2026-03-11)

| Metric | Value |
|---|---|
| Version | v1.5.2 |
| MCP tools | 30 |
| Languages (primary) | 4 (Rust, Go, Zig, TypeScript) |
| Unit tests | 111 passing / 0 failing |
| Binary size (macOS arm64, release) | 58 MB |
| Eval score — structural | 63/63 — 100% |
| Eval score — semantic | 18/18 — 100% |
| Token benchmark — yoyo+linux accuracy | 8–9/10 (tokio, ripgrep) |
| Token benchmark — linux-only accuracy | 1–4/10 (tokio, ripgrep) |
| Baseline (Claude Code, no index) | 20/81 — 25% |
| Delta vs baseline | +75pp |

---

## Language focus

yoyo targets 4 languages with deep, tested support. Other languages may parse but are not a priority.

`Calibrated version` = the version Claude is trained on and the eval harness pins to.

| Language | Calibrated version | bake | symbol | supersearch | file_functions | endpoints | trace_down | eval repos |
|---|---|---|---|---|---|---|---|---|
| **Rust** | 1.75–1.80 (edition 2021) | yes | yes | yes | yes | yes (actix/rocket) | yes | tokio, ripgrep |
| **Go** | 1.21–1.23 | yes | yes | yes | yes | yes (gin/echo/net-http) | yes | gin, httprouter |
| **Zig** | **0.14.1** | yes | yes | yes | yes | no | no | tigerbeetle, zig-lang |
| **TypeScript** | 5.0–5.4 | yes | yes | yes | yes | yes (express) | no | typescript, vscode |

`trace_down` / `flow` call-chain tracing: Rust + Go only.

### Other languages (parses, not prioritised)

JavaScript, Python, C, C++, C#, Java, Kotlin, PHP, Ruby, Swift, Bash — `bake` + `symbol` + `supersearch` work but not eval-tested and not a roadmap priority. See [Serena](https://github.com/oraios/serena) for broader language coverage.

---

## Token benchmark eval

Eval harness at `evals/token_benchmark/` — 12 tasks, 5 dimensions, repo-agnostic (dynamic task generation).

| Repo | Language | Lines | Linux acc | yoyo+linux acc |
|---|---|---|---|---|
| tokio | Rust | 102K | 4/10 | 9/10 |
| ripgrep | Rust | 52K | 7/10 | 8/10 |
| gin | Go | 24K | — | — |
| httprouter | Go | 3K | — | — |
| tigerbeetle | Zig | 149K | — | — |
| zig-lang | Zig | 688K | — | — |
| typescript | TypeScript | 453K | — | — |
| vscode | TypeScript | 1.7M | — | — |

Rows marked `—` = pending run. See `evals/results/` for full JSON.

---

## History

| Version | Date | Languages | Tools | Tests | Binary | Eval |
|---|---|---|---|---|---|---|
| v1.5.2 | 2026-03-11 | 4 primary | 30 | 111 | 58 MB | 100% |
| v1.5.1 | 2026-03-10 | 4 primary | 30 | 111 | 58 MB | 100% |
| v1.5.0 | 2026-03-10 | 4 primary | 30 | 105 | 58 MB | 100% |
| v1.3.0 | 2026-03-09 | 15 | 30 | 85 | 58 MB | 100% |
| v1.2.1 | 2026-03-09 | 15 | 29 | 85 | 58 MB | 100% |
| v1.2.0 | 2026-03-09 | 15 | 29 | 85 | 58 MB | 100% |
| v1.1.1 | 2026-03-09 | 15 | 28 | 34 | 58 MB | 100% |
| v1.1.0 | 2026-03-09 | 15 | 28 | 29 | 58 MB | 100% |
| v1.0.2 | 2026-03-08 | 14 | 28 | 29 | — | 100% |
| v1.0.0 | 2026-03-08 | 14 | 28 | 29 | — | 100% |

---

## What these numbers mean

**Languages (primary)** — Rust, Go, Zig, TypeScript. Deep support: bake + symbol + supersearch + file_functions + eval-tested against real production repos.

**Tools** — MCP tools exposed via `llm_instructions`. CLI commands are not counted separately.

**Tests** — `cargo test` passing count. Integration tests (e2e) and unit tests combined.

**Binary size** — `target/release/yoyo` on macOS arm64 after `cargo build --release`.

**Token benchmark** — accuracy scored by LLM-as-judge (gpt-4o-mini) on 12 tasks per repo. See `evals/token_benchmark/` for harness, `evals/results/` for raw JSON.

**Baseline** — Claude Code with no index, same questions. The delta is what yoyo adds.

---

## Update instructions

After every release, update the "Current" table and append a row to "History". Fill in pending `—` rows in the token benchmark table as evals run. One commit per release, keep it mechanical.
