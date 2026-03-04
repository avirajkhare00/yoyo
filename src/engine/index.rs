use std::fs;

use anyhow::Result;

use super::types::{
    BakeSummary, EndpointSummary, FunctionSummary, LlmInstructionsPayload, ShakePayload,
    ToolDescription, Workflow, WorkflowStep,
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
        workflows: workflow_catalog(),
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

fn tool_catalog() -> Vec<ToolDescription> {
    vec![
        ToolDescription { name: "llm_instructions", description: "Read this first. Returns available tools, workflows, and project stats.", requires_bake: false },
        ToolDescription { name: "bake",             description: "Build or refresh the index. Auto-reindexes on version upgrade or source file change.", requires_bake: false },
        ToolDescription { name: "shake",            description: "Repository overview: languages, file counts, top complex functions, detected endpoints.", requires_bake: false },
        ToolDescription { name: "search",           description: "Fuzzy search over function names and file paths. Returns scored hits with line ranges.", requires_bake: true },
        ToolDescription { name: "symbol",           description: "Exact/partial function lookup. Set include_source=true to retrieve the function body inline.", requires_bake: true },
        ToolDescription { name: "slice",            description: "Read a specific line range of any file. Use start_line/end_line from search or symbol.", requires_bake: false },
        ToolDescription { name: "file_functions",   description: "List all functions in a file with line ranges and cyclomatic complexity.", requires_bake: true },
        ToolDescription { name: "supersearch",      description: "AST-aware search over source files. Prefer over grep. Supports context and pattern filters.", requires_bake: true },
        ToolDescription { name: "all_endpoints",    description: "List all detected HTTP endpoints (Express / Actix / Gin / net/http).", requires_bake: true },
        ToolDescription { name: "api_surface",      description: "Exported API summary grouped by module. Optionally filter by package name.", requires_bake: true },
        ToolDescription { name: "api_trace",        description: "Trace an endpoint path to its handler file and function.", requires_bake: true },
        ToolDescription { name: "crud_operations",  description: "Infer CRUD matrix (create/read/update/delete) from detected endpoints.", requires_bake: true },
        ToolDescription { name: "architecture_map", description: "Project directory structure with inferred roles (routes, services, models). Ranks directories by intent.", requires_bake: true },
        ToolDescription { name: "suggest_placement",description: "Suggest which existing file to add a new function to, based on type and related symbol.", requires_bake: true },
        ToolDescription { name: "package_summary",  description: "Deep-dive into a package/module: files, functions, and endpoints matching a path substring.", requires_bake: true },
        ToolDescription { name: "blast_radius",     description: "Find all functions that transitively call a given symbol. Returns callers and affected files.", requires_bake: true },
        ToolDescription { name: "find_docs",        description: "Find README, .env, config, or Docker files in the project.", requires_bake: false },
        ToolDescription { name: "patch",            description: "Replace a line range in a file with new content.", requires_bake: false },
        ToolDescription { name: "patch_by_symbol",  description: "Replace a function body by symbol name. Resolves location from the bake index.", requires_bake: true },
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
                WorkflowStep { tool: "search", hint: "Fuzzy search by function name or file path to get line ranges" },
                WorkflowStep { tool: "symbol", hint: "Exact lookup; set include_source=true to get the body inline" },
                WorkflowStep { tool: "slice",  hint: "Read surrounding context using start_line/end_line from symbol" },
            ],
        },
        Workflow {
            name: "Add a new feature",
            description: "Decide where to place a new function and write it.",
            steps: vec![
                WorkflowStep { tool: "architecture_map",  hint: "Understand directory roles; pass your intent (e.g. 'user handler')" },
                WorkflowStep { tool: "suggest_placement", hint: "Get ranked file suggestions for the new function" },
                WorkflowStep { tool: "file_functions",    hint: "See existing functions in the target file" },
                WorkflowStep { tool: "slice",             hint: "Read the relevant section before editing" },
                WorkflowStep { tool: "patch_by_symbol",   hint: "Replace or add the function by symbol name" },
            ],
        },
        Workflow {
            name: "Understand an API endpoint",
            description: "Trace an HTTP route to its handler and read the implementation.",
            steps: vec![
                WorkflowStep { tool: "all_endpoints", hint: "List every detected route to find the one you need" },
                WorkflowStep { tool: "api_trace",     hint: "Filter by path/method to get the handler file and name" },
                WorkflowStep { tool: "symbol",        hint: "Look up the handler with include_source=true" },
            ],
        },
        Workflow {
            name: "Impact analysis",
            description: "Find everything that will break if you change a function.",
            steps: vec![
                WorkflowStep { tool: "search",       hint: "Confirm the exact symbol name exists in the index" },
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
            name: "Edit a function",
            description: "Read a function and replace its body.",
            steps: vec![
                WorkflowStep { tool: "symbol",           hint: "Fetch the current body with include_source=true" },
                WorkflowStep { tool: "patch_by_symbol",  hint: "Write the new body; use match_index if multiple symbols match" },
            ],
        },
        Workflow {
            name: "CRUD analysis",
            description: "Map HTTP methods to entities to understand data flow.",
            steps: vec![
                WorkflowStep { tool: "crud_operations", hint: "Get create/read/update/delete matrix per entity" },
                WorkflowStep { tool: "api_trace",       hint: "Drill into a specific route to find the handler" },
                WorkflowStep { tool: "symbol",          hint: "Read the handler implementation" },
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
