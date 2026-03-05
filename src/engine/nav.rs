use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};

use super::types::{
    ArchitectureDir, ArchitectureMapPayload, ArchitectureSuggestion, EndpointSummary,
    FunctionSummary, PackageFileSummary, PackageSummaryPayload, PlacementSuggestion,
    SuggestPlacementPayload,
};
use super::util::{load_bake_index, resolve_project_root};

/// Public entrypoint for the `package_summary` tool.
pub fn package_summary(path: Option<String>, package: String) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let package_lc = package.to_lowercase();

    let mut files = Vec::new();
    let mut functions = Vec::new();
    let mut endpoints = Vec::new();

    for file in &bake.files {
        let path_str = file.path.to_string_lossy();
        if path_str.to_lowercase().contains(&package_lc) {
            files.push(PackageFileSummary {
                path: path_str.to_string(),
                language: file.language.clone(),
                bytes: file.bytes,
            });
        }
    }

    for f in &bake.functions {
        if f.file.to_lowercase().contains(&package_lc) {
            functions.push(FunctionSummary {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                complexity: f.complexity,
            });
        }
    }

    for e in &bake.endpoints {
        if e.file.to_lowercase().contains(&package_lc) || e.path.to_lowercase().contains(&package_lc) {
            endpoints.push(EndpointSummary {
                method: e.method.clone(),
                path: e.path.clone(),
                file: e.file.clone(),
                handler_name: e.handler_name.clone(),
            });
        }
    }

    let payload = PackageSummaryPayload {
        tool: "package_summary",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        package,
        files,
        functions,
        endpoints,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `architecture_map` tool: project structure and placement hints.
pub fn architecture_map(path: Option<String>, intent: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let mut directories: BTreeMap<String, ArchitectureDir> = BTreeMap::new();

    for file in &bake.files {
        let path_str = file.path.to_string_lossy();
        let dir = if let Some((d, _)) = path_str.rsplit_once('/') {
            d.to_string()
        } else {
            ".".to_string()
        };

        let entry = directories.entry(dir.clone()).or_insert_with(|| ArchitectureDir {
            path: dir.clone(),
            file_count: 0,
            languages: BTreeSet::new(),
            roles: Vec::new(),
        });
        entry.file_count += 1;
        entry.languages.insert(file.language.clone());

        let p = path_str.to_lowercase();
        let role_map: &[(&str, &[&str])] = &[
            ("http-endpoints", &["routes", "controllers", "handlers", "resolvers"]),
            ("services",       &["services", "service"]),
            ("models",         &["models", "entities", "entity", "schemas", "schema"]),
            ("middleware",     &["middleware", "interceptors"]),
            ("repositories",   &["repositories", "repo", "repos", "store", "stores"]),
            ("components",     &["components", "widgets", "views"]),
            ("utils",          &["utils", "helpers", "lib", "shared", "common"]),
            ("api-client",     &["api", "network", "client", "clients"]),
            ("hooks",          &["hooks"]),
            ("factories",      &["factories", "factory"]),
        ];
        for (role, keywords) in role_map {
            if keywords.iter().any(|kw| p.contains(kw)) {
                if !entry.roles.contains(&role.to_string()) {
                    entry.roles.push(role.to_string());
                }
            }
        }
    }

    let mut dirs: Vec<ArchitectureDir> = directories.into_values().collect();
    dirs.sort_by(|a, b| a.path.cmp(&b.path));

    let intent_str = intent.clone().unwrap_or_default();
    let intent_lc = intent_str.to_lowercase();
    let mut suggestions = Vec::new();

    if !intent_lc.is_empty() {
        for dir in &dirs {
            let mut score = 0u32;
            let path_lc = dir.path.to_lowercase();

            if intent_lc.contains("handler") || intent_lc.contains("endpoint") || intent_lc.contains("route") {
                if path_lc.contains("routes") || path_lc.contains("controllers") || path_lc.contains("handlers") {
                    score += 5;
                }
            }
            if intent_lc.contains("service") {
                if path_lc.contains("service") { score += 5; }
            }
            if intent_lc.contains("model") || intent_lc.contains("entity") || intent_lc.contains("schema") {
                if path_lc.contains("model") || path_lc.contains("entit") || path_lc.contains("schema") { score += 5; }
            }
            if intent_lc.contains("middleware") {
                if path_lc.contains("middleware") { score += 5; }
            }
            if intent_lc.contains("util") || intent_lc.contains("helper") {
                if path_lc.contains("util") || path_lc.contains("helper") || path_lc.contains("lib") { score += 5; }
            }
            if intent_lc.contains("repo") || intent_lc.contains("store") {
                if path_lc.contains("repo") || path_lc.contains("store") { score += 5; }
            }

            if score > 0 {
                suggestions.push(ArchitectureSuggestion {
                    directory: dir.path.clone(),
                    score,
                    rationale: format!("Matches intent \"{}\" based on directory name/role.", intent_str),
                });
            }
        }
        suggestions.sort_by(|a, b| b.score.cmp(&a.score));
    }

    let payload = ArchitectureMapPayload {
        tool: "architecture_map",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        intent,
        directories: dirs,
        suggestions,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `suggest_placement` tool.
pub fn suggest_placement(
    path: Option<String>,
    function_name: String,
    function_type: String,
    related_to: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let fn_type = function_type.to_lowercase();
    let mut candidates = Vec::new();

    for file in &bake.files {
        let path_str = file.path.to_string_lossy();
        let mut score = 0u32;
        let path_lc = path_str.to_lowercase();

        match fn_type.as_str() {
            "handler" => {
                if path_lc.contains("route") || path_lc.contains("controller") {
                    score += 5;
                }
            }
            "service" => {
                if path_lc.contains("service") {
                    score += 5;
                }
            }
            "repository" => {
                if path_lc.contains("repository") || path_lc.contains("repo") {
                    score += 5;
                }
            }
            "model" => {
                if path_lc.contains("model") || path_lc.contains("entity") {
                    score += 5;
                }
            }
            "util" => {
                if path_lc.contains("util") || path_lc.contains("helper") {
                    score += 5;
                }
            }
            "test" => {
                if path_lc.contains("test") || path_lc.contains("spec") {
                    score += 5;
                }
            }
            _ => {}
        }

        if let Some(ref rel) = related_to {
            if path_lc.contains(&rel.to_lowercase()) {
                score += 2;
            }
        }

        if score > 0 {
            candidates.push(PlacementSuggestion {
                file: path_str.to_string(),
                score,
                rationale: format!(
                    "Heuristic match for type \"{}\" and related_to {:?}.",
                    fn_type, related_to
                ),
            });
        }
    }

    candidates.sort_by(|a, b| b.score.cmp(&a.score));
    candidates.truncate(10);

    let payload = SuggestPlacementPayload {
        tool: "suggest_placement",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        function_name,
        function_type: fn_type,
        related_to,
        suggestions: candidates,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}
