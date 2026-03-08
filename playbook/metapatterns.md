# Metapatterns — The Five Shapes of yoyo Workflows

## What is a metapattern?

Every yoyo workflow is an instance of a small set of high-level shapes.
If you know the five shapes, you know the right tool sequence for any task —
even one you've never seen before.

A metapattern is not a rule. It is a compression of experience.
Once you recognise the shape of a problem, the tools fall into place.

---

## The Five Shapes

### 1. Orient → Scope → Read

**When:** You're unfamiliar with a codebase, a module, or a domain area.
Build the mental model before touching anything.

```
shake / architecture_map       →  package_summary / api_surface / all_endpoints  →  symbol / slice
       (Orient)                              (Scope)                                    (Read)
```

**Instances:** "Orient to an unfamiliar codebase", "Deep-dive into a module",
"Find a function by intent (semantic search)"

**Why this order matters:** Jumping straight to `symbol` without orientation
means you don't know what you don't know. `shake` gives the 30-second map.
`architecture_map` gives directory roles. Only then do individual symbols
make sense in context.

---

### 2. Read → Safety → Write → Verify

**When:** You're about to mutate code.
Never write blind — always read first, check blast radius, then patch.

```
symbol / slice  →  blast_radius  →  patch / multi_patch / graph_rename  →  symbol / slice
   (Read)            (Safety)                  (Write)                         (Verify)
```

**Instances:** "Edit a function", "Rename with safety check",
"Fix a broken API endpoint end-to-end"

**Why this order matters:** Skipping Safety means a rename or delete can
silently break callers you didn't know existed. Skipping Verify means you
trust the patch applied correctly without evidence. Both skips cause
compounding failures later.

---

### 3. Suspect → Confirm → Remove

**When:** You think something is dead weight.
Surface candidates, confirm no hidden callers, then delete.

```
health  →  blast_radius  →  graph_delete
(Suspect)   (Confirm)        (Remove)
```

**Instances:** "Safely delete dead code"

**Why this order matters:** `health` can miss router-registered handlers —
functions that appear dead but are wired via runtime config.
`blast_radius` cross-checks at the call-graph level. Only when both agree
is it safe to delete. `graph_delete` itself blocks if callers still exist,
making this triple-safe.

---

### 4. Orient → Place → Scaffold → Implement

**When:** You're adding new functionality.
Find the right home first, scaffold the shape, then fill in the body.

```
architecture_map  →  suggest_placement  →  graph_create / graph_add  →  patch
   (Orient)              (Place)               (Scaffold)                (Implement)
```

**Instances:** "Add a new feature", "Add a function scaffold"

**Why this order matters:** Skipping Orient and Place leads to functions
landing in the wrong file — violating module boundaries, duplicating logic,
or creating import cycles. The placement tools encode the project's own
conventions, not just generic heuristics.

---

### 5. Trace → Read → Fix

**When:** Something is broken.
Follow the path from entry point to failure, read each layer, then patch the root cause.

```
flow / supersearch / trace_down  →  symbol / slice  →  multi_patch / patch
         (Trace)                       (Read)               (Fix)
```

**Instances:** "Fix a broken API endpoint end-to-end", "Trace a call chain",
"Understand an API endpoint"

**Why this order matters:** Patching without tracing means you fix symptoms,
not causes. The trace tells you the full call path so you know which layer
owns the bug. `multi_patch` lets you fix every affected layer in one atomic
call — no partial fixes.

---

## The Discovery

These five shapes emerged from watching how effective yoyo sessions actually run.
Every workflow in the catalog is a specialisation of one of these shapes.

The insight came from comparing yoyo to ast-grep's agent skill approach.
ast-grep composes AST transforms; yoyo composes intelligence tools.
Both follow the same fundamental pattern: **combinations are the power,
not individual capabilities.**

In yoyo tournaments, a yoyo can make many shapes from one string.
This is it — one program, many tools, many combinations.
The combinations are what's deadly.

---

## How agents learn metapatterns

Metapatterns are embedded in `llm_instructions` output under the `metapatterns`
key. Each entry has:

- `shape` — the abstract label
- `when` — trigger condition in plain English
- `steps` — phases with the concrete tools that implement each
- `instances` — named workflows in the catalog that are instances of this shape

Agents see metapatterns before any tool is called (via MCP instructions)
and again as structured data in `llm_instructions`. The goal: an agent that
reads metapatterns first needs fewer retries — it reaches for the right
tool sequence immediately.

---

## Encoding in code

- `src/engine/types.rs` — `Metapattern` and `MetapatternStep` structs
- `src/engine/index.rs` — `metapattern_catalog()` function; wired into `llm_instructions`
- `src/engine/types.rs:LlmInstructionsPayload` — `metapatterns: Vec<Metapattern>` field
