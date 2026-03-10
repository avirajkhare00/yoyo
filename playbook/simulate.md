# Simulate — Live Demos and Blog Posts

## The pattern

When a feature ships, the best proof it works is a live run against the actual codebase.
Not a unit test. Not a description. A real tool call with real output.

The sequence:

```
ship feature → simulate live → observe what's interesting → write a blog post from what you saw
```

The blog post is not documentation. It is a record of what actually happened —
the result, the edge cases the simulation revealed, and the reasoning behind the design.

---

## How to simulate

### 1. Pick the right target

Simulate against the yoyo codebase itself (dogfooding). It is always available,
always real, and any gap the simulation reveals is a gap worth fixing.

### 2. Run the tool live, not in a test

Tests prove correctness. Simulations reveal behaviour in context — what gets flagged,
what gets missed, and why.

```
# script — dead code triage
let dead = health().dead_code;
dead.filter(|f| blast_radius(f.name).total_callers == 0)
    .map(|f| #{name: f.name, file: f.file, lines: f.lines})
```

```
# compiler guard — patch with a deliberate type error
patch parse_params with: let _boom: nonexistent_type_xyz = 42;
→ expect: error returned, file restored, cargo error message surfaced
```

### 3. Read the output, not just the result

When a simulation runs, the output tells you things the code doesn't:

- Which results are false positives (trait impls look dead to static analysis)
- Which edge cases the safety net actually catches
- What the error message looks like to an agent (is it useful or cryptic?)

### 4. Verify the invariant

After a compiler guard test, always re-read the patched function.
The file should be byte-for-byte identical to before the bad patch.
If it isn't, the rollback failed — that's a critical bug, not a minor one.

---

## What makes a simulation blogworthy

Not every simulation needs a post. Write one when:

- The simulation reveals something the code or docs didn't say clearly
- An edge case came up that changes how you'd use the tool
- The live output shows the design rationale better than prose could

The v1.3.8 compiler guard post came from running a bad patch live and seeing:
`patch rejected: compiler errors (file restored to original): cannot find type nonexistent_type_xyz`.
That one line of output explained the feature better than three paragraphs of description.

The v1.4.0 script post came from running the dead code triage script and noticing
that `fn` was a reserved keyword in Rhai — caught live, fixed in the same session.

---

## Blog post structure

Keep it tight. The structure that works:

1. **What existed before** — one paragraph, what the old behaviour was
2. **Why it wasn't enough** — what the old behaviour couldn't catch
3. **What changed** — the new behaviour, shown with a real code snippet or output
4. **Why the design is what it is** — the reasoning behind the constraint (e.g. why only `graph_delete` in script)
5. **Net change** (optional) — tools added/removed, test count, version bump

No headers inside blog posts. It is prose, not docs.

---

## Meta-learning from this session

### Rhai reserved keywords bite early

`fn` is a keyword in Rhai. Any closure written as `|fn|` fails with a confusing
"Expecting name of a variable" error. The fix is obvious (`|f|`), but the error
message is not. Document the gotcha in the tool description so agents don't hit it.

### Static analysis misses trait impls

`blast_radius` reports zero callers for `ts_language` and `node_kinds` across
every `src/lang/*.rs`. These are trait methods — called via dynamic dispatch,
invisible to a static call graph. The simulation revealed the pattern:
if every language module has the same "dead" function, it is almost certainly
a trait impl, not genuinely dead.

Rule of thumb: if `health` flags the same function name across many files,
it is a trait or interface impl. Do not delete without checking the trait definition.

### Compiler guard is structural, not documentary

The blog post wrote itself once the live output appeared:
`patch rejected: compiler errors (file restored to original)`.
The guard does not warn — it acts. That is the point of a structural safety property.
The blog post's job was to explain *why* the guard does not just warn.

### Simulate → observe → blog is a feedback loop

Every simulation is a test of the docs. If the simulation output needs a paragraph
of explanation before it makes sense, the docs are incomplete. Fix the docs,
then write the post. The post is evidence the fix worked.
