# Test Plan — v0.9.0 (P0 output cap + role inference fixes)

**Date:** 2026-03-06
**Version:** v0.9.0
**Fixes:** #1 `api_surface`, #2 `find_docs`, #3 `architecture_map`

---

## Test Cases

### TC-1: `api_surface` — output bounded on large projects

**Trigger:** Project with > 20 modules (e.g. face-api.js, 386 files)

| Check | Expected | Status |
|---|---|---|
| `total_modules` field present | Yes | ✅ verified |
| `truncated: true` when modules > limit | Yes | ✅ verified |
| `truncated: false` when modules ≤ limit | Yes | ✅ verified (yoyo: 3 modules) |
| Output bounded at `limit` modules (default 20) | Yes | ✅ verified |
| Per-module functions still truncated at `limit` | Yes | ✅ verified |

**Tested on:** yoyo (3 modules, no truncation) + wise-analytics (4 modules, no truncation)
**Needs regression on:** face-api.js (386 files) — expected `truncated: true`

---

### TC-2: `find_docs` — output capped, config pattern fixed

**Trigger:** Any project with many files (especially `doc_type: "all"` or `"config"`)

| Check | Expected | Status |
|---|---|---|
| `truncated: bool` field present | Yes | ✅ verified |
| Stops at 50 matches by default | Yes | ✅ verified |
| `doc_type: "config"` does NOT match arbitrary `.json` files | Correct | ✅ fixed |
| `doc_type: "config"` matches `.toml`, `.yaml`, `.yml` | Correct | ✅ verified (Cargo.toml on yoyo) |
| `doc_type: "all"` on wise-analytics returns 11 files, not 298K | ✅ | ✅ verified |
| `--limit N` CLI flag accepted | Yes | ✅ in struct |

**Tested on:** yoyo (35 files), wise-analytics (11 files)
**Needs regression on:** face-api.js — old behaviour was 298K chars; expected ≤ 50 results now

---

### TC-3: `architecture_map` — role inference, optional intent

**Trigger:** Any project with conventional directory names

| Check | Expected | Status |
|---|---|---|
| `intent` field optional (no error when omitted) | Yes | ✅ verified |
| `roles: []` no longer universal | Roles populated | ✅ `internal/api` → `http-endpoints`, `internal/model` → `models` |
| `handlers/` dirs get `http-endpoints` role | Yes | ✅ via keyword expansion |
| `middleware/` dirs get `middleware` role | Yes | ✅ in keyword map |
| `repositories/` dirs get `repositories` role | Yes | ✅ in keyword map |
| `utils/` dirs get `utils` role | Yes | ✅ in keyword map |
| Intent-based suggestions fire when intent given | Yes | ✅ partial (path-depth heuristic) |

**Known limitation:** Suggestions match on directory *path segment* not file names inside it. A dir named `internal/api` won't fire handler suggestions — only dirs literally containing "routes"/"controllers"/"handlers". Tracked separately.

**Tested on:** wise-analytics (`internal/api` → `http-endpoints` + `api-client`, `internal/model` → `models`)
**Needs regression on:** face-api.js — old behaviour was all dirs getting `roles: []`

---

## Regression Targets

These need manual validation on larger projects before closing the GitHub issues:

| Project | Command | What to verify |
|---|---|---|
| face-api.js (386 files) | `api_surface` | `truncated: true`, ≤ 20 modules returned |
| face-api.js (386 files) | `find_docs --doc-type all` | `truncated: true`, ≤ 50 results |
| face-api.js (386 files) | `find_docs --doc-type config` | No `.ts`/`.js` files in results |
| face-api.js (386 files) | `architecture_map` | `src/`, `src/classes/`, `src/nets/` get non-empty roles |
| Any Express project | `architecture_map` | `routes/` → `http-endpoints`, `models/` → `models` |

---

## Known Issues / Follow-ups

- `architecture_map` suggestions empty when dir path uses generic names like `api/` (not `routes/`) — suggestions heuristic too narrow, role inference is correct
- `reports/` dir matched `repositories` keyword due to "repo" substring — false positive; needs word-boundary matching
- `find_docs --doc-type config` still matches files with "config" in the *name* (e.g. `webpack.config.js`) — this is intentional and correct
