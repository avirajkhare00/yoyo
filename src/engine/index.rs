use std::fs;

use anyhow::Result;

use super::types::{BakeSummary, EndpointSummary, FunctionSummary, LlmInstructionsPayload, ShakePayload};
use super::util::{build_bake_index, default_guidance_text, load_bake_index, project_snapshot, resolve_project_root};

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
        guidance: default_guidance_text(),
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
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
