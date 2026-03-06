# yoyo benchmark — 5 real codebases
**Date:** 2026-03-07
**yoyo version:** 0.14.0
**Methodology:** shallow clone at pinned commit → `yoyo bake` → `shake`, `health`, `architecture-map` → compare against README claims

---

## Projects

| Project | Language | Files | Commit |
|---|---|---|---|
| [ripgrep](https://github.com/BurntSushi/ripgrep) | Rust | 221 | `4519153` |
| [flask](https://github.com/pallets/flask) | Python | 235 | `3a9d54f` |
| [gin](https://github.com/gin-gonic/gin) | Go | 131 | `3e44fdc` |
| [express](https://github.com/expressjs/express) | JavaScript | 213 | `6c4249f` |
| [clap](https://github.com/clap-rs/clap) | Rust | 618 | `1536fb6` |

---

## 1. ripgrep

**README claim:** "A line-oriented search tool that recursively searches the current directory for a regex pattern. Respects gitignore rules."

### shake
- Languages: Rust (primary), Ruby, JSON, YAML
- 221 files indexed across 12 crates (Cargo workspace)
- Top complexity: `from_low_args` (17), `matched_ignore` (17), `run_one` (15)

### health
| Signal | Count |
|---|---|
| Dead code candidates | 92 |
| God functions | 10 |
| Duplicate names | 0 |

### yoyo vs README

**Confirms:** `matched_ignore` (complexity 17, `crates/ignore/src/dir.rs`) validates the README claim that gitignore handling is a first-class concern — it's one of the most complex functions in the codebase.

**Interesting:** `from_low_args` (complexity 17) in `hiargs.rs` — the flag/argument resolution layer is as complex as the core search logic. The README doesn't mention this; the CLI interface is a significant engineering surface.

**Gap found:** Architecture map returned `?` for all directories. ripgrep's Cargo workspace structure (`crates/core`, `crates/searcher`, `crates/ignore`) uses none of the web-framework keywords yoyo's role inference looks for. **yoyo's architecture_map is web-centric and fails on library/tool projects.**

**Dead code caveat:** 92 flagged — but ripgrep is a library crate. Public API functions are never "called" within the codebase by design; they're called by downstream consumers. yoyo's static call graph can't see external callers. False positive rate likely high here.

---

## 2. flask

**README claim:** "A lightweight WSGI web application framework. Designed to make getting started quick and easy, with the ability to scale up to complex applications."

### shake
- Languages: Python (primary), HTML, JSON, SQL, YAML
- 235 files indexed
- Top complexity: `register` (19, blueprints), `make_response` (14, app.py), `run` (12, app.py)

### health
| Signal | Count |
|---|---|
| Dead code candidates | 94 |
| God functions | 10 |
| Duplicate names | 0 |

### yoyo vs README

**Tension with "lightweight":** The top god function is `register` (complexity 19) in `sansio/blueprints.py` and `make_response` (complexity 14) in `app.py`. The response handling and blueprint registration system carries significant complexity. "Lightweight" describes the API surface, not the implementation depth.

**Confirms "scale up to complex":** `url_for` (complexity 11) and `find_app_by_string` (complexity 11) in `cli.py` suggest non-trivial URL resolution and app discovery logic — the machinery needed to support complex applications is already present.

**Endpoint detection false positives:** All 20 detected endpoints came from `tests/test_basic.py`, not from `src/flask/`. yoyo's Flask route detection correctly finds `@app.route()` patterns but can't distinguish production routes from test fixtures. A test-file filter is needed here (distinct from #30).

**Architecture map:** All `?`. Flask's `src/flask/sansio/` structure is not recognized.

---

## 3. gin

**README claim:** "Gin is a HTTP web framework written in Go. Features a martini-like API with performance that is up to 40 times faster than martini."

### shake
- Languages: Go only, YAML
- 131 files indexed (most compact codebase of the 5)
- Top complexity: `findCaseInsensitivePathRec` (**46**), `getValue` (**42**), `handleHTTPRequest` (16)

### health
| Signal | Count |
|---|---|
| Dead code candidates | 9 |
| God functions | 10 |
| Duplicate names | 0 |

### yoyo vs README

**Confirms the performance claim:** The two highest-complexity functions in the entire codebase are both in `tree.go` — the radix tree router. `findCaseInsensitivePathRec` (complexity 46) and `getValue` (42) are deeply optimized trie traversal algorithms. The "40x faster" claim is backed by genuinely complex routing code — this is hand-optimized, not accidental complexity.

**Lowest dead code of any project (9):** Go's exported function convention (capitalized names are public API) aligns with yoyo's call-graph analysis. Go packages are more self-contained than Python/Rust library crates, so fewer false positives.

**`handleHTTPRequest` (complexity 16):** The core dispatch function is the third most complex — expected for a framework that handles method matching, middleware chains, and 404/405 logic in one place.

**Architecture map:** `binding/` directory not recognized as a distinct role. All `?`. The flat Go package structure doesn't trigger any role heuristics.

---

## 4. express

**README claim:** "Fast, unopinionated, minimalist web framework for Node.js."

### shake
- Languages: JavaScript (primary), HTML, JSON, YAML
- 213 files indexed
- **Top functions: none detected**
- **Endpoints: none detected**

### health
| Signal | Count |
|---|---|
| Dead code candidates | 0 |
| God functions | 0 |
| Duplicate names | 0 |

### yoyo vs README

**yoyo is blind to express.** Express uses the CommonJS module pattern throughout:

```js
app.use = function use(fn) { ... }
exports.application = app;
proto.route = function route(path) { ... }
```

yoyo's JavaScript/TypeScript parser (tree-sitter) indexes `function` declarations and arrow functions assigned to `const`/`let`/`var`. It does **not** detect:
- `exports.name = function() {}`
- `proto.method = function() {}`
- `obj.method = function() {}`

This is a hard parser gap. Express has 40+ function definitions in `lib/application.js` alone — all invisible to yoyo. The "0 functions, 0 endpoints, 0 dead code" result is entirely a false negative.

**Action required:** File an issue to add CommonJS method assignment patterns to the JavaScript analyzer.

---

## 5. clap

**README claim:** "Command Line Argument Parser for Rust."

### shake
- Languages: Rust (primary), Python, YAML
- 618 files — **largest codebase by 2.8x**
- Top complexity: `assert_app` (35, debug_asserts.rs), `parse` (33, parser.rs), `gen_fish_inner` (24)

### health
| Signal | Count |
|---|---|
| Dead code candidates | 134 |
| God functions | 10 |
| Duplicate names | 0 |

### yoyo vs README

**Complexity matches scope:** `parse` at complexity 33 (`clap_builder/src/parser/parser.rs`) reflects the genuine complexity of a comprehensive argument parser handling subcommands, value types, validation, aliases, environment variables, and defaults simultaneously.

**`assert_app` (complexity 35)** is the debug-mode validation function that checks for conflicting argument configurations — it's a comprehensive sanity-checker that runs only in debug builds. High complexity expected and appropriate.

**`push_attrs` (complexity 23)** in `clap_derive/src/item.rs` — the derive macro attribute parser. The macro layer is nearly as complex as the runtime parser.

**618 files, multi-crate:** clap is a workspace with `clap_builder`, `clap_derive`, `clap_complete`, `clap_mangen`. yoyo handles this correctly — it indexes across all crates without configuration.

**Dead code (134):** Highest of the 5. Large public API surface + extensive derive macro generated code = many functions never called within the codebase itself.

---

## Cross-project findings

### What yoyo gets right

| Finding | Evidence |
|---|---|
| Complexity correlates with README claims | gin's router complexity confirms "40x faster"; ripgrep's `matched_ignore` confirms gitignore depth |
| Go has lower false-positive dead code | 9 vs 92–134 for Rust/Python — Go exported function convention is cleaner |
| Health signals are consistent across languages | Works on Rust, Python, Go without tuning |
| Bake handles multi-crate Cargo workspaces | ripgrep (12 crates), clap (4 crates) indexed correctly |

### What yoyo gets wrong

| Gap | Impact | Issue |
|---|---|---|
| CommonJS `exports.fn = function(){}` not detected | Express entirely invisible | File new issue |
| `architecture_map` role inference is web-framework-centric | All 5 projects show `?` for all directories | Known gap |
| Endpoint detection finds test routes, not production routes | Flask shows 20 test endpoints as production routes | Related to #30 |
| Dead code false positives on library public APIs | 92–134 flagged for Rust library crates | Known (#30 variant) |

### Complexity distribution

| Project | Max complexity | Median (top 10) | Character |
|---|---|---|---|
| gin | 46 | 17 | Performance-optimized internals |
| clap | 35 | 23 | Comprehensive feature surface |
| flask | 19 | 11 | Moderate, spread across concerns |
| ripgrep | 17 | 12 | Tight, well-bounded functions |
| express | — | — | Not measurable (parser gap) |

---

## New issues to file from this report

1. **JavaScript CommonJS method assignment not parsed** — `exports.fn = function(){}`, `proto.method = function(){}` pattern blind spot. Affects all pre-ESM JavaScript codebases.
2. **Endpoint detection includes test files** — Flask (and likely other frameworks) routes in test files surface as production endpoints. Needs a test-file filter in the route detector.
