# yoyo — Metrics

Single source of truth for yoyo's measurable state. Updated every release.

---

## Current (v1.3.0 — 2026-03-09)

| Metric | Value |
|---|---|
| Version | v1.3.0 |
| MCP tools | 30 |
| Languages supported | 15 |
| Unit tests | 85 passing / 0 failing |
| Binary size (macOS arm64, release) | 58 MB |
| Eval score — structural | 63/63 — 100% |
| Eval score — semantic | 18/18 — 100% |
| Baseline (Claude Code, no index) | 20/81 — 25% |
| Delta vs baseline | +75pp |

---

## Language support matrix

`Calibrated version` = the version Claude is trained on and the eval harness pins to.

| Language | Calibrated version | bake | symbol | supersearch | file_functions | endpoints | trace_down |
|---|---|---|---|---|---|---|---|
| Rust | 1.75–1.80 (edition 2021) | yes | yes | yes | yes | yes (actix/rocket) | yes |
| Go | 1.21–1.23 | yes | yes | yes | yes | yes (gin/echo/net-http) | yes |
| TypeScript | 5.0–5.4 | yes | yes | yes | yes | yes (express) | no |
| JavaScript | ES2022 | yes | yes | yes | yes | yes (express) | no |
| Python | 3.11–3.12 | yes | yes | yes | yes | no | no |
| C | C17 | yes | yes | yes | yes | no | no |
| C++ | C++17/20 | yes | yes | yes | yes | no | no |
| C# | .NET 8 | yes | yes | yes | yes | no | no |
| Java | 21 (LTS) | yes | yes | yes | yes | no | no |
| Kotlin | 1.9 | yes | yes | yes | yes | no | no |
| PHP | 8.2 | yes | yes | yes | yes | no | no |
| Ruby | 3.2 | yes | yes | yes | yes | no | no |
| Swift | 5.9 | yes | yes | yes | yes | no | no |
| Bash | — | yes | yes | yes | yes | no | no |
| Zig | **0.14.1** | yes | yes | yes | yes | no | no |

`trace_down` / `flow` call-chain tracing: Rust + Go only (emit structured call-graph data at bake time).

---

## History

| Version | Date | Languages | Tools | Tests | Binary | Eval |
|---|---|---|---|---|---|---|
| v1.3.0 | 2026-03-09 | 15 | 30 | 85 | 58 MB | 100% |
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
