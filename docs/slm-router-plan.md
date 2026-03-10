# SLM Router — Full Implementation Plan

A self-improving code generation pipeline: a fine-tuned SLM handles the common case, a full LLM handles the hard case, and the compiler is the ground truth for both.

Target: **80% of requests handled by SLM without escalation.**

---

## Phase 0 — Data

**Source: Zigistry as repo index.** Do not generate Zig from scratch — LLMs hallucinate APIs and wrong versions. Use real ecosystem code as the ground truth output, generate only the instruction.

1. Download [Zigistry/Zigistry-complete-dataset](https://huggingface.co/datasets/Zigistry/Zigistry-complete-dataset) (3,420 Zig repos with metadata).
2. Filter: `has_build_zig=true` + `zig_minimum_version=0.14.x` + stars > threshold.
3. Clone filtered repos — these are the ground truth outputs.
4. Run `yoyo bake` + `file_functions` to extract functions from each repo.
5. For each real function: prompt a SOTA LLM to generate a natural language task that would produce it → synthetic `(instruction, output)` pair.
   - Include relevant stdlib signatures in each prompt (Phase 2.5 mechanism) — the SLM trains on correct API usage from day one.
6. Verify every pair: `zig build-lib` / `zig test`. Keep only what compiles clean.
7. Target ~800 verified pairs; hard floor is enough to fill the split.
8. Split: **640 train / 160 validation**.

**Why answer-first:** the output already exists and already compiles. The LLM only generates the question — much lower error surface than pure synthetic generation.

---

## Phase 1 — SLM

1. Set up Python env + MLX (Apple Silicon only — deliberate, not a constraint to paper over).
2. Download **Qwen3 0.6B 4-bit** first for speed experiments; fall back to 1.7B if quality is insufficient.
3. Run SFT via `mlx_lm.lora` on the 640-example training split. Training runs on **Metal (GPU)** — not the ANE. The ANE is inference-only and only accessible via Core ML; it cannot be programmed directly.
4. **Do not merge the adapter.** Keep base model and adapter separate — see Phase 1.5.
5. Serve via `mlx_lm.server --model Qwen/Qwen3-0.6B --adapter-path ./zig-adapter` on localhost.

Reference: [hollance/neural-engine](https://github.com/hollance/neural-engine) — ground truth on what the ANE can and cannot do.

---

## Phase 1.5 — Weights deployment

The adapter and base model are deployed separately. The base model is never shipped — users download it once via the standard HF cache. Only the adapter is versioned and distributed.

```
base model  (Qwen3 0.6B 4-bit)  ~400MB  downloaded once → ~/.cache/huggingface/
adapter     (zig-slm fine-tune)  ~20-50MB  versioned → Hugging Face Hub
```

### Steps

1. Push adapter to Hugging Face Hub after every retrain (not the merged weights).
2. Tag adapter versions to match yoyo releases (e.g. `zig-slm-v1.0`).
3. Add `yoyo slm pull` sub-command: fetches adapter for the current yoyo version from HF Hub.
4. Add `yoyo slm update` sub-command: pulls latest adapter tag.
5. Router starts `mlx_lm.server` as a subprocess on demand (base model + adapter path), waits for ready, kills on exit.
6. Adapter path: `~/.yoyo/adapters/zig-slm/` — well-known, consistent.

### Why not merge

- Adapter is ~50MB vs ~400MB merged model — 8x smaller distribution
- Retrain = push a new adapter, users pull small delta
- Rollback = pull a previous adapter tag
- Base model is shared HF cache — not yoyo's problem to manage

---

## Phase 2 — yoyo Zig support

1. Add `src/lang/zig.rs` using the `tree-sitter-zig` grammar.
2. Mirror the `src/lang/rust.rs` pattern exactly.
3. Cover full language support: symbol indexing, function/type extraction, flow tracing, endpoint detection where applicable.
4. **Index the Zig stdlib** — see Phase 2.5.
5. Release a new yoyo version.

---

## Phase 2.5 — Stdlib-aware indexing ✅ SHIPPED (v1.4.2–v1.4.5)

LLM training data is months behind the pinned Zig version. Injecting a manual constraints file is an anti-pattern — it goes stale and pollutes the prompt. The fix is to pull live signatures off disk, filtered to only what the task actually touches.

### How it works

```
flow(target_fn) → identifies stdlib callees on the call path
                        ↓
symbol(callee, stdlib=true) → pulls exact current signature off disk
                        ↓
inject: 3-5 relevant signatures into prompt (not the whole stdlib)
```

The call graph is the relevance filter. If a stdlib function is not on the call path, it never appears in the prompt. No noise.

### Stdlib path detection (automatic, no user config) — shipped

| Language | Command | Path | Shipped |
|---|---|---|---|
| Zig | `zig env --json` → `lib_dir` | `<lib_dir>/std/` | v1.4.2 |
| Go | `go env GOROOT` | `$GOROOT/src/` | v1.4.2 |
| Rust | `rustc --print sysroot` | `<sysroot>/lib/rustlib/src/rust/library/` | v1.4.2 |
| TypeScript | `npm`/`pnpm`/`yarn root -g` | `<global_modules>/typescript/lib/` | v1.4.4–v1.4.5 |

`symbol(name, stdlib=true)` walks the installed toolchain, parses candidates, returns matches tagged `is_stdlib: true`. Project results always rank first.

### Drift defense stack

```
stdlib signatures pulled live via call graph    ← covers API drift
           +
pinned compiler version                         ← covers syntax drift
           +
compiler loop                                   ← catches everything else
```

Training cutoff becomes irrelevant for stdlib calls. Works identically for SLM and LLM. No manual maintenance.

### Applies to

Zig, Go, Rust, TypeScript — same mechanism, same precision.

---

## Phase 3 — Router

yoyo ships as a single binary. The Router is a new sub-command (or a new crate inside the same workspace) — not a separate repo.

The SLM does not route itself. An external classifier makes the routing decision before the SLM is ever called. The SLM is a pure code generator — it never self-reports confidence.

### Routing logic

1. Receive the user's natural language request.
2. Pull AST context via yoyo MCP: `blast_radius`, `flow`, symbol complexity.
3. **Classifier** scores the request on task complexity signals:
   - Estimated AST diff size (how many nodes will change)
   - `blast_radius` — how many callers/callees are affected
   - Cyclomatic complexity of the target function
   - Task type: completion (easy) vs refactor (medium) vs cross-file change (hard)
4. Classifier outputs: `simple` | `complex`
   - `simple` → SLM handles → yoyo MCP apply → compiler verify
   - `complex` → LLM handles → yoyo MCP apply

### Classifier implementation

Start with a rule-based scorer (no ML needed for v1):
- `blast_radius > N` → complex
- target function LOC > threshold → complex
- request spans > 1 file → complex
- everything else → simple

Replace with a learned classifier once enough routing decisions are logged.

---

## Phase 4 — Compiler Loop

1. SLM output → `zig`/`rustc`.
2. On failure: retry SLM with the original prompt + full compiler error, max **3 retries**.
3. After 3 failures: escalate to LLM.

---

## Phase 5 — Integration and Benchmarking

1. End-to-end test: English prompt → Router → patched file.
2. Benchmark metrics:
   - **% SLM-handled without escalation** — target: 80%
   - **Classifier precision** — % of `simple` decisions that actually compile first try
   - **Escalation breakdown** — classifier escalation vs compiler-loop exhaustion
3. Use classifier logs to identify task types the SLM can't handle → fix dataset.
4. Iterate: tighten classifier thresholds, expand dataset on weak spots, retrain.

---

## Design decisions

| Decision | Choice | Reason |
|---|---|---|
| Platform | Apple Silicon only | MLX is the fastest local inference path on M-series; no need to abstract it away |
| SLM size | Start at 0.6B | Speed-first; upgrade to 1.7B only if eval shows quality gap |
| Adapter vs merge | Ship adapter only, not merged weights | 8x smaller distribution; base model is a one-time HF cache download; rollback is a tag pull |
| Routing | External classifier, not SLM self-report | Small models are poorly calibrated; asking the SLM to know what it doesn't know is unreliable |
| Classifier v1 | Rule-based (blast_radius, LOC, file count) | No training data needed; logs routing decisions for a future learned classifier |
| Retry payload | Full error + original prompt | More context = better repair |
| Escalation target | 80% SLM, 20% LLM | Forces dataset quality discipline; a low SLM rate means data, not model |
| LLM drift | Stdlib signatures pulled live via call graph | Training cutoff irrelevant; model reads correct API off disk same as a developer would |
| Stdlib context | Call-graph filtered (3-5 signatures max) | Full stdlib is noise; only symbols on the call path are injected |
| Binary shape | Single yoyo binary | Consistent with existing architecture; no new deployment surface |
