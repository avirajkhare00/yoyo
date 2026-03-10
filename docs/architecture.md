# yoyo Architecture

yoyo has three logical layers:

1. **Stable core** — parse code, build the index, answer structural questions, and make safe graph-aware edits.
2. **Volatile edges** — present that capability through CLI and MCP schemas without owning the logic.
3. **Feedback loop** — evals, end-to-end tests, and dogfooding that prove whether the system is actually getting better.

The rule of thumb is simple: fix root causes in the core, keep adapters thin, and let the feedback loop decide what deserves to exist.

---

## 1. Stable Core

This is the product.

- [`src/lang/`](../src/lang) parses language-specific syntax and extracts functions, types, calls, endpoints, and metadata.
- [`src/engine/`](../src/engine) turns that index into usable capabilities: search, flow tracing, blast radius, patching, graph edits, health analysis, semantic search, and orchestration.

Core modules:

| Module | Responsibility |
|---|---|
| [`src/lang/`](../src/lang) | Per-language analyzers and shared language abstractions |
| [`src/engine/index.rs`](../src/engine/index.rs) | Bake, bootstrap catalogs, instructions, workflow/reference metadata |
| [`src/engine/search.rs`](../src/engine/search.rs) | `symbol`, `supersearch`, `semantic_search`, stdlib lookup |
| [`src/engine/analysis.rs`](../src/engine/analysis.rs) | `blast_radius`, `health`, deletion safety, dead-code analysis |
| [`src/engine/api.rs`](../src/engine/api.rs) | Endpoint tracing and `flow` |
| [`src/engine/nav.rs`](../src/engine/nav.rs) | Architecture map, package summary, placement suggestions |
| [`src/engine/edit.rs`](../src/engine/edit.rs) | `patch`, `multi_patch`, `patch_bytes`, compiler/syntax guards |
| [`src/engine/graph.rs`](../src/engine/graph.rs) | Graph-aware rename, create, add, move, delete, trace-down |
| [`src/engine/script.rs`](../src/engine/script.rs) | Structured orchestration over engine tools |
| [`src/engine/types.rs`](../src/engine/types.rs) | Shared payload and wire-format types |

Why this layer is stable:

- Every interface depends on it.
- Wrong answers usually originate here, not in the adapters.
- Presentation changes are cheap only when the engine is correct.

If the engine is wrong, no amount of CLI or MCP polish will save the product.

---

## 2. Volatile Edges

These are adapters over the core.

- [`src/cli.rs`](../src/cli.rs) defines the human CLI surface.
- [`src/mcp.rs`](../src/mcp.rs) defines the MCP tool registry, schemas, and request handling.

This layer should stay thin:

- parse arguments
- validate shape
- call engine
- return structured output

It should not:

- reimplement engine logic
- invent safety rules the engine does not enforce
- paper over engine bugs with presentation tricks

This is the layer that should evolve fastest. The engine should evolve more carefully.

---

## 3. Feedback Loop

This is what keeps yoyo honest.

- [`src/engine/e2e_tests.rs`](../src/engine/e2e_tests.rs) verifies real tool behavior on fixture projects.
- [`evals/`](../evals) measures yoyo against real codebases and baselines.
- Dogfooding sessions expose pain while using yoyo to build yoyo.

This layer answers the only question that matters:

**Did the change improve the system in practice?**

Without this layer, yoyo would drift toward clever demos and local optimizations.
With it, the product stays grounded in observed behavior.

---

## How Work Should Flow

The intended direction of change is:

```text
feedback loop -> identifies failure
stable core   -> fixes root cause
volatile edge -> exposes the fix cleanly
```

Not this:

```text
feedback loop -> finds engine bug
volatile edge -> adds workaround
core          -> stays wrong
```

That second pattern creates rule-heavy software: more prompts, more exceptions, more special cases.

The first pattern creates rule-less software: the right behavior emerges from the product shape itself.

---

## Architecture Principle

The architecture is trying to enforce one idea:

**constraints should live as close as possible to the capability that needs them.**

Examples:

- deletion safety belongs in graph deletion logic
- reindexing belongs in write paths
- unsupported-language behavior belongs in the tool response
- context reduction belongs in response shaping, not only in prompt instructions

That is how yoyo stays intuitive as the system grows: fewer global rules, stronger local behavior.
