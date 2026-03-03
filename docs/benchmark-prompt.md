# Benchmark Prompt (reusable for any project)

You are onboarding me to a new codebase. Use ONLY the yoyo MCP server.
Call all tools in this exact sequence, in parallel batches where possible:

## BATCH 1 (project foundation)
- `llm_instructions(path)`       → get yoyo guidance
- `shake(path)`                  → language/file stats + top complex functions
- `bake(path)`                   → build the index (required before most tools)

## BATCH 2 (structure & APIs)
- `architecture_map(path, intent="<project type>")`  → directory roles
- `all_endpoints(path)`          → HTTP endpoints
- `api_surface(path)`            → exported public API by module
- `crud_operations(path)`        → entity CRUD matrix

## BATCH 3 (deep dives)
- `package_summary(path, package="<core module 1>")`  → module files + functions
- `package_summary(path, package="<core module 2>")`  → module files + functions
- `search(path, q="<main entry function>")`           → fuzzy symbol search
- `supersearch(path, query="<key pattern>")`          → AST-aware text search

## BATCH 4 (symbol inspection)
- `symbol(path, name="<most complex function>")`      → definition + location
- `symbol(path, name="<key data type>")`              → definition + location
- `file_functions(path, file="<main entry file>")`    → all functions in file
- `file_functions(path, file="<core logic file>")`    → all functions in file

## BATCH 5 (code reading & placement)
- `slice(path, file, start, end)`   → read the most complex function
- `slice(path, file, start, end)`   → read the main entry point
- `api_trace(path, endpoint="<key function>")`        → trace call chain
- `suggest_placement(path, function_name="<new feature>", function_type="util", related_to="<existing fn>")`

---

Compile all results into a structured onboarding document covering:

1. **Project type & language stack**
2. **Architecture overview** (modules, their roles)
3. **Public API surface**
4. **Most complex/important functions** (with file:line refs)
5. **Key data flow** (how input reaches the core logic)
6. **Where to add a new feature**
