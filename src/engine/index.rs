use std::fs;

use anyhow::Result;

use super::types::{
    BakeSummary, DecisionEntry, EndpointSummary, FunctionSummary, LlmInstructionsPayload,
    LlmWorkflowsPayload, Metapattern, MetapatternStep, ShakePayload, ToolDescription, Workflow,
    WorkflowStep,
};
use super::util::{build_bake_index, load_bake_index, project_snapshot, resolve_project_root};

/// Public entrypoint for the `llm_instructions` CLI/MCP tool.
pub fn llm_instructions(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let snapshot = project_snapshot(&root)?;

    let payload = LlmInstructionsPayload {
        tool: "llm_instructions",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        languages: snapshot.languages.into_iter().collect(),
        files_indexed: snapshot.files_indexed,
        tools: tool_catalog(),
        prime_directives: vec![
            "grep, cat, and read-file are text tools. They find strings. They cannot answer structural questions about code.",
            "For any question about visibility, module path, callers, callees, methods, fields, or trait implementations — use yoyo tools, not grep.",
            "Before adding any new function or tool, search the codebase first — it may already exist. Duplication is the first form of rot.",
            "Before writing, read. Use symbol or supersearch to understand existing code before proposing changes.",
            "Dead code is waste. Use health to identify unused functions and graph_delete to remove them.",
            "Write tools are destructive and irreversible. Always confirm safety with blast_radius or health before deleting.",
        ],
        concurrency_rules: vec![
            "Always call bake first and wait for completion before any read-indexed tool.",
            "llm_instructions can be called in parallel with bake on first contact.",
            "read + read: always parallelise freely (category=read or read-indexed).",
            "read-indexed tools are safe to parallelise with each other after bake completes.",
            "write tools are always sequential — wait for each to complete before the next.",
            "After any write, do not call read-indexed tools on the same file until the write response is received.",
        ],
        update_available: super::update::check_update(),
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `llm_workflows` CLI/MCP tool.
/// Returns the full reference catalog: workflows, decision map, antipatterns, metapatterns.
/// Call on demand — not required for basic tool use.
pub fn llm_workflows(path: Option<String>) -> Result<String> {
    let _ = path; // unused; kept for API symmetry with llm_instructions
    let payload = LlmWorkflowsPayload {
        tool: "llm_workflows",
        version: env!("CARGO_PKG_VERSION"),
        workflows: workflow_catalog(),
        decision_map: decision_map(),
        antipatterns: vec![
            "grep to count callers: overcounts — hits comments, docs, string literals, partial names. Use blast_radius.",
            "grep to find a definition: returns all files containing the string, not the canonical definition. Use symbol.",
            "reading raw source to determine visibility: pub/pub(crate)/nothing requires inference and is error-prone. Use symbol — visibility field is an exact enum.",
            "inferring module path from file path: conventions vary by language and project. Use symbol — module_path field is authoritative.",
            "str.replace to rename: corrupts partial matches (e.g. renaming is_match also renames is_match_candidate). Use graph_rename.",
            "deleting a function without checking callers: leaves the codebase broken. Use graph_delete — it blocks if callers exist.",
            "grep to list methods of a struct: returns all fn definitions in the file, not grouped by type. Use file_functions filtered by parent_type.",
            "grep to find trait implementors: matches impl blocks loosely, misses generic impls. Use symbol — implementors field on trait matches.",
            "reading struct source to get field types: works but is unstructured. Use symbol with include_source=true — fields array is parsed and typed.",
            "using Edit or Write to modify a function body: line numbers drift after edits, no reindex, no syntax check. Use patch — name mode resolves location from the index, returns patched_source for inline verification, and auto-reindexes.",
        ],
        metapatterns: metapattern_catalog(),
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

fn decision_map() -> Vec<DecisionEntry> {
    vec![
        DecisionEntry {
            question: "Where is function/struct/enum/trait X defined?",
            wrong_tool: "grep 'fn X' or 'struct X'",
            wrong_because: "Returns every file containing the string — comments, tests, re-exports, partials. 21 hits when answer is 1.",
            right_tool: "symbol",
            right_field: "file + start_line",
        },
        DecisionEntry {
            question: "Is X public, private, or crate-visible?",
            wrong_tool: "read raw source and infer from pub/pub(crate)/nothing",
            wrong_because: "Inference is error-prone and inconsistent across languages.",
            right_tool: "symbol",
            right_field: "visibility (exact enum: public | module | private)",
        },
        DecisionEntry {
            question: "What module or package does X belong to?",
            wrong_tool: "infer from file path",
            wrong_because: "Path conventions vary. For Rust, `src/` is stripped and crate name is inferred from workspace layout. mod re-exports break naive path inference entirely.",
            right_tool: "symbol",
            right_field: "module_path (e.g. tokio::sync, not tokio::src::sync)",
        },
        DecisionEntry {
            question: "What functions does X call?",
            wrong_tool: "grep for names inside the function body",
            wrong_because: "Cannot isolate calls *by* a specific function. Returns all occurrences in the file.",
            right_tool: "symbol",
            right_field: "calls[] (project-defined callees only, stdlib filtered)",
        },
        DecisionEntry {
            question: "Who calls X? How many callers?",
            wrong_tool: "grep for X and count lines",
            wrong_because: "Overcounts — hits comments, docs, string literals, partial names. 244 grep hits vs 29 real callers in tokio.",
            right_tool: "blast_radius",
            right_field: "callers[] (deduplicated, non-self, no false positives)",
        },
        DecisionEntry {
            question: "What methods does struct X have?",
            wrong_tool: "grep 'fn' in the struct's file",
            wrong_because: "Returns all functions in the file with no grouping by impl block.",
            right_tool: "file_functions",
            right_field: "functions[] filtered by parent_type == X",
        },
        DecisionEntry {
            question: "What fields does struct X have?",
            wrong_tool: "read struct source body",
            wrong_because: "Works but returns unstructured text — field types not queryable.",
            right_tool: "symbol with include_source=true",
            right_field: "fields[{name, type_str, visibility}] (Rust only)",
        },
        DecisionEntry {
            question: "What traits does struct X implement?",
            wrong_tool: "grep 'impl.*X'",
            wrong_because: "Matches loosely — hits impl blocks for other types, misses generic impls.",
            right_tool: "symbol",
            right_field: "implements[] on struct/enum matches",
        },
        DecisionEntry {
            question: "Which types implement trait X?",
            wrong_tool: "grep 'impl X for'",
            wrong_because: "Misses blanket impls, generic impls, re-exports. Requires manual deduplication.",
            right_tool: "symbol",
            right_field: "implementors[] on trait matches (deduplicated)",
        },
        DecisionEntry {
            question: "Which function is most complex / hardest to maintain?",
            wrong_tool: "none — no text tool can answer this",
            wrong_because: "Complexity requires parsing AST and counting branches across the whole codebase.",
            right_tool: "health",
            right_field: "large_functions[{name, file, score}]",
        },
        DecisionEntry {
            question: "What code is unused / dead?",
            wrong_tool: "none — no text tool can answer this",
            wrong_because: "Dead code detection requires a full call graph, not string search.",
            right_tool: "health",
            right_field: "dead_code[]",
        },
        DecisionEntry {
            question: "Rename X everywhere safely",
            wrong_tool: "str.replace / sed",
            wrong_because: "Corrupts partial matches — renaming is_match also renames is_match_candidate, is_match_at.",
            right_tool: "graph_rename",
            right_field: "word-boundary safe, scope-aware (file | package | project), atomic",
        },
        DecisionEntry {
            question: "Is it safe to delete X?",
            wrong_tool: "just delete it",
            wrong_because: "Leaves callers broken with no warning. spawn_blocking has 31 callers in tokio.",
            right_tool: "graph_delete",
            right_field: "blocks with caller list if callers exist; proceeds only when dead",
        },
        DecisionEntry {
            question: "Edit / patch function X by name",
            wrong_tool: "grep for line number, then Edit at that line",
            wrong_because: "Requires two tool calls. Line numbers drift after edits — stale lookups corrupt the wrong region.",
            right_tool: "patch with name= parameter",
            right_field: "resolves location from index — one call, immune to line drift",
        },
        DecisionEntry {
            question: "Find a function by what it does, not its name",
            wrong_tool: "grep for keywords or read many files",
            wrong_because: "No structural awareness. Returns every file containing the string, including comments, docs, tests. Cannot rank by relevance.",
            right_tool: "semantic_search",
            right_field: "results[{name, file, start_line, score}] — cosine similarity via local ONNX embeddings (score 0–1); TF-IDF fallback if DB absent",
        },
    ]
}

pub fn tool_catalog() -> Vec<ToolDescription> {
    vec![
        ToolDescription { name: "llm_instructions", description: "Bootstrap: lean tool catalog, prime directives, and concurrency rules. Call in parallel with bake on first contact — do not skip.", requires_bake: false, category: "bootstrap", parallelisable: false, output_shape: None },
        ToolDescription { name: "llm_workflows",   description: "Full reference catalog: 21 combination workflows, decision map, antipatterns, metapatterns. Call on demand — not required at bootstrap.", requires_bake: false, category: "bootstrap", parallelisable: false, output_shape: None },
        ToolDescription { name: "bake",             description: "Build the AST index all read-indexed tools depend on. Call in parallel with llm_instructions on first contact. Re-run after large external changes.", requires_bake: false, category: "bootstrap", parallelisable: false, output_shape: None },
        ToolDescription { name: "shake",            description: "Language breakdown, file count, top-complexity functions.", requires_bake: false, category: "read", parallelisable: true,
            output_shape: Some(r#"{"files_indexed":0,"languages":[],"top_functions":[{"name":"","file":"","start_line":0,"complexity":0}],"express_endpoints":[]}"#) },
        ToolDescription { name: "slice",            description: "Read any line range from any file. Use start_line/end_line from symbol output directly.", requires_bake: false, category: "read", parallelisable: true, output_shape: None },
        ToolDescription { name: "find_docs",        description: "Locate README, .env, Dockerfile, and config files.", requires_bake: false, category: "read", parallelisable: true, output_shape: None },
        ToolDescription { name: "architecture_map", description: "Directory tree with inferred roles (routes, services, models, utils).", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "symbol",           description: "Find a function by name — file, line range, visibility, calls, optionally full source. Pass include_source=true to read the body. Use file to scope when names collide.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"matches":[{"name":"","file":"","start_line":0,"end_line":0,"visibility":"public|module|private","complexity":0,"calls":[],"source":"(if include_source=true)"}]}"#) },
        ToolDescription { name: "file_functions",   description: "Every function in a file with line ranges and cyclomatic complexity.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "supersearch",      description: "AST-aware search — replaces grep/rg entirely. Use context=identifiers+pattern=call for call-site search.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "semantic_search",  description: "Find functions by intent using local ONNX embeddings. No API key. Use when supersearch finds nothing.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "all_endpoints",    description: "All detected HTTP routes. Frameworks: Express, Actix-web, Rocket, Flask, FastAPI, gin, echo, net/http. Use when flow returns no match.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "flow",             description: "One-call vertical slice: endpoint → handler → call chain to db/http/queue boundary. Call chain: Rust + Go only.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"endpoint":{"method":"","path":""},"handler":{"name":"","file":"","start_line":0,"source":"(if include_source=true)"},"call_chain":[{"name":"","file":"","depth":0}],"boundaries":[],"summary":""}"#) },
        ToolDescription { name: "suggest_placement",description: "Ranked file suggestions for where to add a new function, based on related symbols.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "package_summary",  description: "All functions, endpoints, and complexity for a module path substring.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "blast_radius",     description: "All transitive callers of a symbol + affected files. Always run before graph_delete or graph_rename.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"symbol":"","callers":[{"caller":"","file":"","depth":1,"complexity":0}],"affected_files":[],"total_callers":0}"#) },
        ToolDescription { name: "trace_down",       description: "BFS call chain from a function to db/http/queue boundaries. Rust + Go only. Prefer flow for endpoint tracing.", requires_bake: true, category: "read-indexed", parallelisable: true, output_shape: None },
        ToolDescription { name: "patch",            description: "Replaces Edit/Write for all function-level changes. Modes: name (name+new_content), content-match (file+old_string+new_string), line-range (file+start+end+new_content). Auto-reindexes and syntax-checks after every write.", requires_bake: false, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "patch_bytes",      description: "Splice at exact byte offsets from the bake index. Prefer patch for function-level edits.", requires_bake: true, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "multi_patch",      description: "Apply N edits across M files in one call — bottom-up ordering is automatic. Use after flow to fix an entire call chain end-to-end.", requires_bake: true, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "graph_rename",     description: "Rename a symbol at its definition and every call site atomically. Always prefer over str.replace or multi_patch for renames.", requires_bake: false, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "graph_create",     description: "Create a new file with an initial function scaffold. Errors if file exists or parent dir is missing.", requires_bake: false, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "graph_add",        description: "Insert a function scaffold into an existing file, optionally after a named symbol. Pair with patch to fill in the body immediately after.", requires_bake: false, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "graph_move",       description: "Move a function between files atomically — removes from source, appends to destination, reindexes both.", requires_bake: true, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "graph_delete",     description: "Remove a function by name. Blocks if callers exist. Always run health→blast_radius first to confirm the function is dead.", requires_bake: true, category: "write", parallelisable: false, output_shape: None },
        ToolDescription { name: "health",           description: "Dead code, large functions, and duplicate name hints. Router-registered handlers may appear dead — cross-check with blast_radius before deleting.", requires_bake: true, category: "read-indexed", parallelisable: true,
            output_shape: Some(r#"{"large_functions":[{"name":"","file":"","start_line":0,"score":0,"complexity":0,"fan_out":0}],"dead_code":[{"name":"","file":"","start_line":0,"lines":0}],"duplicate_code":[{"stem":"","functions":[{"name":"","file":""}]}],"long_methods":[{"name":"","file":"","start_line":0,"lines":0}],"feature_envy":[{"name":"","file":"","envies":"","cross_file_calls":0}],"shotgun_surgery":[{"name":"","file":"","caller_files":0}]}"#) },
        ToolDescription { name: "script", description: "Execute a Rhai script with read tools and graph_delete available as functions. Supports loops, map/filter, arbitrary conditionals. Read: symbol(name), blast_radius(name), health(), supersearch(query), file_functions(file), flow(endpoint, method), slice(file, start, end). Write: graph_delete(name). Returns {result: <last_expr>}. Tip: fn is a reserved keyword in Rhai — use f or item as closure parameter names.", requires_bake: false, category: "orchestration", parallelisable: false, output_shape: None },
    ]
}

fn workflow_catalog() -> Vec<Workflow> {
    vec![
        Workflow {
            name: "First-time setup",
            description: "Index the project before using any bake-dependent tool.",
            steps: vec![
                WorkflowStep { tool: "bake",  hint: "Build the index (auto-refreshes on future source changes)" },
                WorkflowStep { tool: "shake", hint: "Get a high-level overview of the codebase" },
            ],
        },
        Workflow {
            name: "Explore a function",
            description: "Find a function by name and read its source.",
            steps: vec![
                WorkflowStep { tool: "supersearch", hint: "Search by name or pattern to find the function" },
                WorkflowStep { tool: "symbol",      hint: "Exact lookup; set include_source=true to get the body inline" },
                WorkflowStep { tool: "slice",       hint: "Read surrounding context using start_line/end_line from symbol" },
            ],
        },
        Workflow {
            name: "Add a new feature",
            description: "Decide where to place a new function and scaffold it.",
            steps: vec![
                WorkflowStep { tool: "architecture_map",  hint: "Understand directory roles; pass your intent (e.g. 'user handler')" },
                WorkflowStep { tool: "suggest_placement", hint: "Get ranked file suggestions for the new function" },
                WorkflowStep { tool: "graph_create",      hint: "If adding to a new file: create the file + initial scaffold in one call. Errors if file exists." },
                WorkflowStep { tool: "graph_add",         hint: "If adding to an existing file: insert a scaffold at the right location (optionally after_symbol); index auto-updates" },
                WorkflowStep { tool: "patch",             hint: "Fill in the scaffold body — use name mode (pass symbol name) or old_string/new_string mode" },
            ],
        },
        Workflow {
            name: "Understand an API endpoint",
            description: "Trace an HTTP route to its handler and full call chain in one call.",
            steps: vec![
                WorkflowStep { tool: "flow", hint: "Pass endpoint path substring (and optional method). Returns handler + call chain + boundaries in one call. Prefer over api_trace + trace_down + symbol." },
                WorkflowStep { tool: "all_endpoints", hint: "If flow returns no match, list all detected routes to find the right path substring" },
            ],
        },
        Workflow {
            name: "Impact analysis",
            description: "Find everything that will break if you change a function.",
            steps: vec![
                WorkflowStep { tool: "symbol",       hint: "Confirm the exact symbol name exists in the index" },
                WorkflowStep { tool: "blast_radius", hint: "Get all transitive callers and affected files" },
                WorkflowStep { tool: "symbol",       hint: "Inspect each caller for context" },
                WorkflowStep { tool: "slice",        hint: "Read caller bodies to understand the coupling" },
            ],
        },
        Workflow {
            name: "Deep-dive into a module",
            description: "Understand a package or directory end-to-end.",
            steps: vec![
                WorkflowStep { tool: "package_summary", hint: "Get all files, functions, and endpoints for a path substring" },
                WorkflowStep { tool: "file_functions",  hint: "List functions per file with complexity scores" },
                WorkflowStep { tool: "slice",           hint: "Read specific functions using their line ranges" },
            ],
        },
        Workflow {
            name: "Search for code patterns",
            description: "Find usages, assignments, or calls across the codebase.",
            steps: vec![
                WorkflowStep { tool: "supersearch", hint: "Use context=identifiers and pattern=call for call-site search" },
                WorkflowStep { tool: "slice",       hint: "Read matches in context using the returned line numbers" },
            ],
        },
        Workflow {
            name: "Find a function by intent (semantic search)",
            description: "You know what a function does but not its name. Use semantic_search to find ranked candidates.",
            steps: vec![
                WorkflowStep { tool: "semantic_search", hint: "Pass a natural-language query, e.g. 'validate user token' or 'spawn blocking task'. Returns cosine-similarity ranked matches (0–1 score). Requires bake to have run first to build the embeddings DB." },
                WorkflowStep { tool: "symbol",          hint: "Confirm the top match with include_source=true to read the body" },
            ],
        },
        Workflow {
            name: "Edit a function",
            description: "Read a function and replace its body.",
            steps: vec![
                WorkflowStep { tool: "symbol",           hint: "Fetch the current body with include_source=true" },
                WorkflowStep { tool: "patch",  hint: "Write the new body — pass name + new_content, or use old_string/new_string for content-match mode" },
            ],
        },
        Workflow {
            name: "Find configuration and docs",
            description: "Locate README, .env, config, or Dockerfile.",
            steps: vec![
                WorkflowStep { tool: "find_docs", hint: "Use doc_type: readme | env | config | docker | all" },
                WorkflowStep { tool: "slice",     hint: "Read the first N lines of any matched file" },
            ],
        },
        Workflow {
            name: "Graph rename (one-shot)",
            description: "Rename an identifier at its definition and every call site in one call. No multi-step setup required.",
            steps: vec![
                WorkflowStep { tool: "graph_rename", hint: "Pass name (old) and new_name; word-boundary matching prevents partial renames; index is auto-updated" },
                WorkflowStep { tool: "symbol",       hint: "Verify the definition now carries the new name" },
            ],
        },
        Workflow {
            name: "Add a function scaffold",
            description: "Insert a new empty function body at the right location, then fill it in.",
            steps: vec![
                WorkflowStep { tool: "graph_add",        hint: "Specify entity_type (fn/function/def/func), name, file, and optionally after_symbol" },
                WorkflowStep { tool: "patch",  hint: "Fill in the generated scaffold — use name mode or old_string/new_string" },
            ],
        },
        Workflow {
            name: "Move a function between files",
            description: "Relocate a function to a different module/file and keep the index consistent.",
            steps: vec![
                WorkflowStep { tool: "bake",       hint: "Ensure byte_start/byte_end offsets are fresh" },
                WorkflowStep { tool: "graph_move", hint: "Pass the function name and destination file; source removal and dest append happen atomically" },
            ],
        },
        Workflow {
            name: "Safely delete dead code",
            description: "Confirm a function is truly unused before removing it. The combination prevents broken builds.",
            steps: vec![
                WorkflowStep { tool: "health",       hint: "Get dead code candidates — functions with no detected callers" },
                WorkflowStep { tool: "blast_radius", hint: "Cross-check: list all transitive callers of the candidate (health can miss router-registered handlers)" },
                WorkflowStep { tool: "graph_delete", hint: "Remove the function — tool blocks if callers still exist, so this is safe to run" },
            ],
        },
        Workflow {
            name: "Fix a broken API endpoint end-to-end",
            description: "Trace a route to its full call chain and patch every affected layer in one session.",
            steps: vec![
                WorkflowStep { tool: "flow",        hint: "Pass the endpoint path substring — returns handler + full call chain + boundaries in one call" },
                WorkflowStep { tool: "symbol",      hint: "Read each function in the chain with include_source=true to understand the failure" },
                WorkflowStep { tool: "multi_patch", hint: "Apply all fixes across all files in one call — bottom-up ordering is automatic" },
            ],
        },
        Workflow {
            name: "Rename with safety check",
            description: "Understand the blast radius before renaming, then rename atomically.",
            steps: vec![
                WorkflowStep { tool: "blast_radius", hint: "Scope the impact — see all callers and affected files before touching anything" },
                WorkflowStep { tool: "graph_rename", hint: "Rename at definition + every call site atomically; word-boundary matching prevents partial renames" },
                WorkflowStep { tool: "symbol",       hint: "Verify the definition carries the new name" },
            ],
        },
        Workflow {
            name: "Orient to an unfamiliar codebase",
            description: "Build a mental model of a new project from the outside in.",
            steps: vec![
                WorkflowStep { tool: "shake",            hint: "Language breakdown, file count, top-complexity functions — 30-second overview" },
                WorkflowStep { tool: "architecture_map", hint: "Directory tree with inferred roles (routes, services, models, etc.)" },
                WorkflowStep { tool: "all_endpoints",    hint: "All HTTP routes — understand the API surface" },
                WorkflowStep { tool: "health",           hint: "Dead code and large functions — where is the rot?" },
            ],
        },
        Workflow {
            name: "Graph-level rename (manual — prefer graph_rename)",
            description: "[DEPRECATED: use graph_rename for one-shot rename] Manual rename via byte-precise edits with multi_patch. Use only when you need fine-grained control over which occurrences to rename.",
            steps: vec![
                WorkflowStep { tool: "bake",         hint: "Ensure the index is fresh so byte_start/byte_end are accurate" },
                WorkflowStep { tool: "symbol",        hint: "Confirm the definition: note file, byte_start, byte_end" },
                WorkflowStep { tool: "blast_radius",  hint: "Find all callers and affected files" },
                WorkflowStep { tool: "supersearch",   hint: "Search for the old name (context=identifiers) to collect call-site offsets" },
                WorkflowStep { tool: "multi_patch",   hint: "Pass all edits (definition + call sites) in one call; bottom-up order is handled automatically" },
            ],
        },
        Workflow {
            name: "Precise in-line edit",
            description: "Replace a single identifier or expression at exact byte position without touching surrounding code.",
            steps: vec![
                WorkflowStep { tool: "symbol",      hint: "Look up the function; note byte_start/byte_end from the index" },
                WorkflowStep { tool: "slice",       hint: "Read the relevant lines to confirm the target byte range" },
                WorkflowStep { tool: "patch_bytes", hint: "Splice new_content at byte_start..byte_end; only those bytes change" },
            ],
        },
        Workflow {
            name: "Trace a call chain",
            description: "Follow a function's callees downward to database, HTTP, or queue boundaries.",
            steps: vec![
                WorkflowStep { tool: "bake",       hint: "Ensure index is fresh so call edges are populated" },
                WorkflowStep { tool: "trace_down", hint: "Pass symbol name; optionally set depth (default 5) and file to disambiguate" },
                WorkflowStep { tool: "symbol",     hint: "Inspect any resolved callee with include_source=true" },
            ],
        },
        Workflow {
            name: "Run a multi-tool pipeline",
            description: "Chain multiple tools in a single atomic call. Each step can reference previous step output via {{step_id.field[N].subfield}}. Steps with a false 'if' condition are skipped.",
            steps: vec![
                WorkflowStep { tool: "pipeline", hint: "Pass spec: a JSON array of steps. Each step has id, tool, args (with optional {{refs}}), and optional if condition. Output: {steps: [{id, tool, ok, result}]}." },
            ],
        },
        Workflow {
            name: "Pipeline: safe dead code removal",
            description: "Find dead candidates, confirm zero callers, delete only if safe — in one atomic pipeline call. The if condition is the machine-enforced safety net: if blast_radius finds callers (e.g. a router registration invisible to the AST), the delete is skipped automatically.",
            steps: vec![
                WorkflowStep { tool: "pipeline", hint: r#"spec: [{"id":"audit","tool":"health"},{"id":"check","tool":"blast_radius","args":{"symbol":"{{audit.dead_code[0].name}}","depth":2}},{"id":"remove","tool":"graph_delete","args":{"name":"{{audit.dead_code[0].name}}"},"if":"{{check.callers | length == 0}}"}]"# },
            ],
        },
        Workflow {
            name: "Pipeline: find by intent, read source, check blast radius",
            description: "Locate a function by natural-language description, read its full source, and measure its blast radius — before touching anything. All three steps in one call.",
            steps: vec![
                WorkflowStep { tool: "pipeline", hint: r#"spec: [{"id":"find","tool":"semantic_search","args":{"query":"<describe what the function does>"}},{"id":"read","tool":"symbol","args":{"name":"{{find.results[0].name}}","include_source":true}},{"id":"scope","tool":"blast_radius","args":{"symbol":"{{find.results[0].name}}"}}]"# },
            ],
        },
        Workflow {
            name: "Pipeline: trace endpoint, check blast radius only if handler found",
            description: "Trace an HTTP endpoint to its handler, then measure the handler's blast radius — but only if the endpoint matched. The if guard prevents a spurious blast_radius call when the endpoint is not found.",
            steps: vec![
                WorkflowStep { tool: "pipeline", hint: r#"spec: [{"id":"trace","tool":"flow","args":{"endpoint":"/api/your-route"}},{"id":"scope","tool":"blast_radius","args":{"symbol":"{{trace.handler.name}}"},"if":"{{trace.handler.name}}"}]"# },
            ],
        },
    ]
}

fn metapattern_catalog() -> Vec<Metapattern> {
    vec![
        Metapattern {
            shape: "Orient → Scope → Read",
            when: "You're unfamiliar with a codebase, a module, or a domain area. Build the mental model before touching anything.",
            steps: vec![
                MetapatternStep { phase: "Orient",  tools: vec!["shake", "architecture_map"] },
                MetapatternStep { phase: "Scope",   tools: vec!["package_summary", "all_endpoints"] },
                MetapatternStep { phase: "Read",    tools: vec!["symbol", "slice"] },
            ],
            instances: vec!["Orient to an unfamiliar codebase", "Deep-dive into a module", "Find a function by intent (semantic search)"],
        },
        Metapattern {
            shape: "Read → Safety → Write → Verify",
            when: "You're about to mutate code. Never write blind — always read first, check blast radius, then patch.",
            steps: vec![
                MetapatternStep { phase: "Read",    tools: vec!["symbol", "slice"] },
                MetapatternStep { phase: "Safety",  tools: vec!["blast_radius"] },
                MetapatternStep { phase: "Write",   tools: vec!["patch", "multi_patch", "graph_rename"] },
                MetapatternStep { phase: "Verify",  tools: vec!["symbol", "slice"] },
            ],
            instances: vec!["Edit a function", "Rename with safety check", "Fix a broken API endpoint end-to-end"],
        },
        Metapattern {
            shape: "Suspect → Confirm → Remove",
            when: "You think something is dead weight. Surface candidates, confirm no hidden callers, then delete.",
            steps: vec![
                MetapatternStep { phase: "Suspect", tools: vec!["health"] },
                MetapatternStep { phase: "Confirm", tools: vec!["blast_radius"] },
                MetapatternStep { phase: "Remove",  tools: vec!["graph_delete"] },
            ],
            instances: vec!["Safely delete dead code"],
        },
        Metapattern {
            shape: "Orient → Place → Scaffold → Implement",
            when: "You're adding new functionality. Find the right home first, scaffold the shape, then fill in the body.",
            steps: vec![
                MetapatternStep { phase: "Orient",    tools: vec!["architecture_map"] },
                MetapatternStep { phase: "Place",     tools: vec!["suggest_placement"] },
                MetapatternStep { phase: "Scaffold",  tools: vec!["graph_create", "graph_add"] },
                MetapatternStep { phase: "Implement", tools: vec!["patch"] },
            ],
            instances: vec!["Add a new feature", "Add a function scaffold"],
        },
        Metapattern {
            shape: "Trace → Read → Fix",
            when: "Something is broken. Follow the path from entry point to failure, read each layer, then patch the root cause.",
            steps: vec![
                MetapatternStep { phase: "Trace", tools: vec!["flow", "supersearch", "trace_down"] },
                MetapatternStep { phase: "Read",  tools: vec!["symbol", "slice"] },
                MetapatternStep { phase: "Fix",   tools: vec!["multi_patch", "patch"] },
            ],
            instances: vec!["Fix a broken API endpoint end-to-end", "Trace a call chain", "Understand an API endpoint"],
        },
    ]
}

/// Public entrypoint for the `shake` (repository overview) tool.
pub fn shake(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;

    if let Some(bake) = load_bake_index(&root)? {
        // Use rich data from the bake index when available.
        let mut top_functions: Vec<FunctionSummary> = bake
            .functions
            .iter()
            .map(|f| FunctionSummary {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                complexity: f.complexity,
            })
            .collect();
        // Sort by descending complexity and trim.
        top_functions.sort_by(|a, b| b.complexity.cmp(&a.complexity));
        top_functions.truncate(10);

        let express_endpoints: Vec<EndpointSummary> = bake
            .endpoints
            .iter()
            .take(20)
            .map(|e| EndpointSummary {
                method: e.method.clone(),
                path: e.path.clone(),
                file: e.file.clone(),
                handler_name: e.handler_name.clone(),
            })
            .collect();

        let payload = ShakePayload {
            tool: "shake",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            languages: bake.languages.into_iter().collect(),
            files_indexed: bake.files.len(),
            notes: "Shake is using the bake index: languages, files, top complex functions, and Express endpoints are derived from bakes/latest/bake.json.".to_string(),
            top_functions: Some(top_functions),
            express_endpoints: Some(express_endpoints),
        };

        let json = serde_json::to_string_pretty(&payload)?;
        Ok(json)
    } else {
        // Fallback: lightweight filesystem scan if no bake exists yet.
        let snapshot = project_snapshot(&root)?;

        let payload = ShakePayload {
            tool: "shake",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            languages: snapshot.languages.into_iter().collect(),
            files_indexed: snapshot.files_indexed,
            notes: "Shake is currently backed by a lightweight filesystem scan (languages + file counts). Run `bake` first to unlock richer summaries.".to_string(),
            top_functions: None,
            express_endpoints: None,
        };

        let json = serde_json::to_string_pretty(&payload)?;
        Ok(json)
    }
}

/// Public entrypoint for the `bake` tool: build and persist a basic project index.
///
/// This first version records files, languages, and sizes, and writes
/// `bakes/latest/bake.json` under the project root.
pub fn bake(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = build_bake_index(&root)?;

    let bakes_dir = root.join("bakes").join("latest");
    fs::create_dir_all(&bakes_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create bakes dir: {}: {}", bakes_dir.display(), e))?;
    let bake_path = bakes_dir.join("bake.json");

    let json = serde_json::to_string_pretty(&bake)?;
    fs::write(&bake_path, &json)
        .map_err(|e| anyhow::anyhow!("Failed to write bake index to {}: {}", bake_path.display(), e))?;

    // Build embeddings DB for semantic_search (best-effort — never fails the bake)
    if let Err(e) = crate::engine::embed::build_embeddings(&bakes_dir) {
        eprintln!("[yoyo] Embeddings skipped: {e}");
    }

    let summary = BakeSummary {
        tool: "bake",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        bake_path,
        files_indexed: bake.files.len(),
        languages: bake.languages.iter().cloned().collect(),
    };

    let out = serde_json::to_string_pretty(&summary)?;
    Ok(out)
}
