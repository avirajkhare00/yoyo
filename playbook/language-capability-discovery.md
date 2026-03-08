# Playbook: Language Capability Discovery

**Issue:** #77
**Released:** v0.22.5
**Problem class:** Silent degradation — tool returns wrong answer instead of no answer

---

## The Problem

A user on a PHP project calls `flow("/api/users")`. They get back:

```json
{
  "endpoint": { "method": "GET", "path": "/api/users" },
  "handler": { "name": "get_users", "file": "app/Http/Controllers/UserController.php" },
  "call_chain": [],
  "boundaries": [],
  "summary": "GET /api/users → get_users"
}
```

No error. No warning. The agent reads `call_chain: []` and concludes: "this handler calls nothing." It proposes changes without understanding the depth below. The answer is wrong. The tool didn't fail — it silently returned a misleading result.

This is worse than an error. An error is visible. Silent degradation is invisible.

---

## Thought Process

### Step 1: Verify the problem is real, not speculative

Before writing any code, I read `trace_down` in `src/engine/graph.rs`. Key finding: there is no language check anywhere. The function finds the symbol in the bake index, calls `trace_chain`, and returns whatever comes back. For PHP, `trace_chain` returns an empty chain because PHP's bake output doesn't include call edges. The tool succeeds. The result is useless.

Same for `flow` — it calls `trace_chain` internally. Handler found, chain empty, no indication why.

The problem is confirmed and live, not hypothetical.

### Step 2: Find where language information exists

The fix needs to know the language. I checked `IndexedFunction` in `src/lang/mod.rs`:

```rust
pub struct IndexedFunction {
    pub name: String,
    pub file: String,
    pub language: String,  // <-- already there
    ...
}
```

`start.language` is available at the exact point where the fix needs to go. No schema changes, no new bake fields. The data is already in the index.

### Step 3: Decide the fix layer

Two options:

**Option A: Fail early with a structured error**
Check language before running `trace_chain`. If unsupported, return a JSON response with `supported: false`, the language name, the reason, and alternatives. This is honest — the tool says "I cannot help with this" instead of returning empty data.

**Option B: Add a warning field alongside empty results**
Run the tool anyway, but attach a `chain_warning` to the response explaining the limitation.

These are not mutually exclusive. The right answer is both, applied where each fits:

- `trace_down`: Option A. There is no useful result to return for non-Rust/Go. Returning an honest "not supported" response is strictly better than returning empty chain.
- `flow`: Option B. The handler and endpoint are still useful even when the chain can't be traced. Return the handler (valuable), and add `chain_warning` so the agent knows the chain is empty by limitation, not because the function has no callees.

### Step 4: Fix the schema descriptions too

Runtime fixes help agents at call time. But agents see tool schemas before calling. The schema description for `trace_down` should say "Rust and Go only" so an agent on a PHP project reaches for `supersearch` instead.

Same for all endpoint tools — `flow`, `all_endpoints`, `api_trace`, `crud_operations` should list supported frameworks so an agent on a Django project doesn't waste a call.

This is the two-layer fix: schema descriptions teach before the call, runtime responses correct after the call.

---

## Implementation

### Layer 1: Tool schema descriptions (src/mcp.rs)

Added framework/language constraints to every tool that has them:

- `all_endpoints`: listed supported frameworks (Express, Actix-web, Rocket, Flask, FastAPI, gin, echo, net/http) and explicitly called out what is not supported (Axum, NestJS, Fastify, Django)
- `flow`: "call chain tracing: Rust and Go only — on other languages the handler is returned but the chain will be empty"
- `api_trace`, `crud_operations`: "same framework support as all_endpoints"
- `blast_radius`: import-graph expansion (affected files list) works for Rust, Go, Python, TS, JS only
- `api_surface`: "TypeScript only"

### Layer 2: Runtime structured response (src/engine/graph.rs)

In `trace_down`, immediately after finding the start symbol:

```rust
let lang = start.language.to_lowercase();
if lang != "rust" && lang != "go" {
    return Ok(serde_json::to_string_pretty(&serde_json::json!({
        "tool": "trace_down",
        "supported": false,
        "language": start.language,
        "reason": "trace_down call-chain tracing is only supported for Rust and Go...",
        "alternatives": [
            "supersearch with context=identifiers and pattern=call to find call sites manually",
            "symbol+include_source to read the function body and trace manually",
            "flow for endpoint-rooted tracing — handler is still returned even without chain"
        ]
    }))?);
}
```

The response is honest JSON. The agent reads `supported: false`, understands why, and gets three concrete alternatives to try next.

### Layer 3: chain_warning in flow (src/engine/types.rs + src/engine/api.rs)

Added `chain_warning: Option<String>` to `FlowPayload`. Field is skipped when `None` (no overhead for Rust/Go projects). For other languages, it's populated after finding the handler:

```rust
let warning = if lang != "rust" && lang != "go" {
    Some(format!(
        "Call-chain tracing is not supported for {}. Handler returned but call_chain will be empty. \
         Use supersearch (context=identifiers, pattern=call) to trace calls manually.",
        start_fn.language
    ))
} else {
    None
};
```

---

## What the agent sees now

**Before (PHP, trace_down):**
```json
{ "chain": [], "unresolved": [] }
```
Agent interprets: "this function calls nothing."

**After (PHP, trace_down):**
```json
{
  "supported": false,
  "language": "php",
  "reason": "trace_down call-chain tracing is only supported for Rust and Go.",
  "alternatives": ["supersearch...", "symbol+include_source...", "flow..."]
}
```
Agent interprets: "this tool cannot help here, use X instead."

**Before (PHP, flow):**
```json
{ "handler": {...}, "call_chain": [], "summary": "GET /api/users → get_users" }
```
Agent interprets: "handler has no callees."

**After (PHP, flow):**
```json
{
  "handler": {...},
  "call_chain": [],
  "chain_warning": "Call-chain tracing is not supported for php. Handler returned but call_chain will be empty. Use supersearch (context=identifiers, pattern=call) to trace calls manually.",
  "summary": "GET /api/users → get_users"
}
```
Agent interprets: "handler found, chain is empty by limitation, here's what to do next."

---

## Generalised pattern

When a tool has a language or framework constraint:

1. **Schema description** — state the constraint explicitly. Agents read this before calling.
2. **Runtime response** — return `supported: false` with reason and alternatives when the constraint is violated. Never return empty data without explanation.
3. **For partial support** — return what you can, attach a warning field for what you cannot. Don't refuse the whole call if some value is still possible.

The goal: an agent should never be worse off for having called a yoyo tool. Either it gets useful data, or it gets a clear explanation of why it didn't and where to go next.

---

## Files changed

| File | What changed |
|---|---|
| `src/mcp.rs` | Tool descriptions: framework/language constraints added to 6 tools |
| `src/engine/graph.rs` | `trace_down`: language check + structured not-supported response |
| `src/engine/api.rs` | `flow`: `chain_warning` populated for non-Rust/Go languages |
| `src/engine/types.rs` | `FlowPayload`: added `chain_warning: Option<String>` |
