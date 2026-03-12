# yoyo — Metrics

Single source of truth for yoyo's measurable state. Updated every release.

---

## Current (v1.8.2 — 2026-03-12)

| Metric | Value |
|---|---|
| Version | v1.8.2 |
| MCP tools | 12 |
| Languages (primary) | 4 (Rust, Go, Zig, TypeScript) |
| Unit tests | 161 passing / 0 failing |
| Binary size (macOS arm64, release) | 58 MB |
| Eval score — structural | 63/63 — 100% |
| Eval score — semantic | 18/18 — 100% |
| Token benchmark — yoyo accuracy (ripgrep) | 7/10 avg, 41% fewer tokens than linux |
| Token benchmark — yoyo accuracy (tokio) | 7/10 avg vs linux 6/10 |
| Token benchmark — linux-only accuracy | 6/10 avg (ripgrep + tokio) |
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

Eval harness at `evals/token_benchmark/` — 18 tasks (6 structural, 6 semantic, 6 mutation), 5 dimensions, repo-agnostic (dynamic task generation).

| Repo | Language | Lines | Linux tok (avg) | yoyo tok (avg) | Linux acc | yoyo acc | Notes |
|---|---|---|---|---|---|---|---|
| ripgrep | Rust | 52K | 5,527 | 3,249 (-41%) | 6/10 | **7/10** | |
| tokio | Rust | 102K | 5,025 | 6,932 (+38%) | 6/10 | **7/10** | blast_radius inflates token count on high-caller fns |
| gin | Go | 24K | — | — | 6/10 | 9/10 | older run, pre-harness |
| httprouter | Go | 3K | — | — | 7/10 | 7/10 | small repo |
| tigerbeetle | Zig | 149K | — | — | 3/10 | 7/10 | 2 tasks hit 128K limit (#149) |
| zig-lang | Zig | 688K | — | — | 1/10 | 5/10 | massive overflow both sides |
| typescript | TypeScript | 453K | — | — | — | — | pending rerun |
| vscode | TypeScript | 1.7M | — | — | 3/10 | 4/10 | heavy overflow, 1.7M lines |

Rows marked `—` = pending run. See `evals/results/` for full JSON.

**Key finding (2026-03-11):** yoyo is +1 accuracy point on every tested repo. Token cost is mixed — yoyo wins big on structural tasks (complexity, call chains, function bodies: 43–95% fewer tokens) but `blast_radius` and `health` are verbose for simple queries and inflate yoyo's average on repos with highly-connected functions. Root cause: these tools return full transitive closure by design — correct for safety, expensive for simple lookups. A `--limit` / concise mode is the next fix.

**Inflation commands identified:**
- `blast_radius` on high-caller symbols → 20–34K tokens vs grep's 150–700
- `health` → 8K tokens (all categories); dead-code-only mode needed
- `architecture_map` → 4K tokens even at 100-dir cap

---

## History

| Version | Date | Languages | Tools | Tests | Binary | Eval |
|---|---|---|---|---|---|---|
| v1.8.2 | 2026-03-12 | 4 primary | 12 | 161 | 58 MB | 100% |
| v1.8.1 | 2026-03-12 | 4 primary | 21 | 146 | 58 MB | 100% |
| v1.8.0 | 2026-03-12 | 4 primary | 21 | 146 | 58 MB | 100% |
| v1.7.2 | 2026-03-11 | 4 primary | 30 | 146 | 58 MB | 100% |
| v1.6.0 | 2026-03-11 | 4 primary | 30 | 134 | 58 MB | 100% |
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

**Tools** — MCP tools exposed via `boot`. CLI commands are not counted separately.

**Tests** — `cargo test` passing count. Integration tests (e2e) and unit tests combined.

**Binary size** — `target/release/yoyo` on macOS arm64 after `cargo build --release`.

**Token benchmark** — accuracy scored by LLM-as-judge (gpt-4o-mini) on 12 tasks per repo. See `evals/token_benchmark/` for harness, `evals/results/` for raw JSON.

**Baseline** — Claude Code with no index, same questions. The delta is what yoyo adds.

---

## Update instructions

After every release, update the "Current" table and append a row to "History". Fill in pending `—` rows in the token benchmark table as evals run. One commit per release, keep it mechanical.
