# yoyo — Metrics

Single source of truth for yoyo's measurable state. Updated every release.

---

## Current (v1.2.1 — 2026-03-09)

| Metric | Value |
|---|---|
| Version | v1.2.1 |
| MCP tools | 29 |
| Languages supported | 15 |
| Unit tests | 85 passing / 0 failing |
| Binary size (macOS arm64, release) | 58 MB |
| Eval score — structural | 63/63 — 100% |
| Eval score — semantic | 18/18 — 100% |
| Baseline (Claude Code, no index) | 20/81 — 25% |
| Delta vs baseline | +75pp |

---

## Language support matrix

| Language | bake | symbol | supersearch | file_functions | endpoints | trace_down |
|---|---|---|---|---|---|---|
| Rust | yes | yes | yes | yes | yes (actix/rocket) | yes |
| Go | yes | yes | yes | yes | yes (gin/echo/net-http) | yes |
| TypeScript | yes | yes | yes | yes | yes (express) | no |
| JavaScript | yes | yes | yes | yes | yes (express) | no |
| Python | yes | yes | yes | yes | no | no |
| C | yes | yes | yes | yes | no | no |
| C++ | yes | yes | yes | yes | no | no |
| C# | yes | yes | yes | yes | no | no |
| Java | yes | yes | yes | yes | no | no |
| Kotlin | yes | yes | yes | yes | no | no |
| PHP | yes | yes | yes | yes | no | no |
| Ruby | yes | yes | yes | yes | no | no |
| Swift | yes | yes | yes | yes | no | no |
| Bash | yes | yes | yes | yes | no | no |
| Zig | yes | yes | yes | yes | no | no |

`trace_down` / `flow` call-chain tracing: Rust + Go only (emit structured call-graph data at bake time).

---

## History

| Version | Date | Languages | Tools | Tests | Binary | Eval |
|---|---|---|---|---|---|---|
| v1.2.1 | 2026-03-09 | 15 | 29 | 85 | 58 MB | 100% |
| v1.2.0 | 2026-03-09 | 15 | 29 | 85 | 58 MB | 100% |
| v1.1.1 | 2026-03-09 | 15 | 28 | 34 | 58 MB | 100% |
| v1.1.0 | 2026-03-09 | 15 | 28 | 29 | 58 MB | 100% |
| v1.0.2 | 2026-03-08 | 14 | 28 | 29 | — | 100% |
| v1.0.0 | 2026-03-08 | 14 | 28 | 29 | — | 100% |

---

## What these numbers mean

**Languages** — a language counts when it has `bake` + `symbol` + `supersearch` + `file_functions` all working. Partial support doesn't count.

**Tools** — MCP tools exposed via `llm_instructions`. CLI commands are not counted separately.

**Tests** — `cargo test` passing count. Integration tests (e2e) and unit tests combined.

**Binary size** — `target/release/yoyo` on macOS arm64 after `cargo build --release`. Watching for growth.

**Eval** — structural + semantic score on tokio + ripgrep + axum. See [`evals/REPORT.md`](./evals/REPORT.md) for full breakdown.

**Baseline** — Claude Code with no index, same questions. The delta is what yoyo adds.

---

## Update instructions

After every release, update the "Current" table and append a row to "History". One commit per release, keep it mechanical.
