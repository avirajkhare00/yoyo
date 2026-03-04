use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

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
        let mut top_functions: Vec<ShakeFunctionSummary> = bake
            .functions
            .iter()
            .map(|f| ShakeFunctionSummary {
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

        let express_endpoints: Vec<ShakeEndpointSummary> = bake
            .endpoints
            .iter()
            .take(20)
            .map(|e| ShakeEndpointSummary {
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
        .with_context(|| format!("Failed to create bakes dir: {}", bakes_dir.display()))?;
    let bake_path = bakes_dir.join("bake.json");

    let json = serde_json::to_string_pretty(&bake)?;
    fs::write(&bake_path, &json)
        .with_context(|| format!("Failed to write bake index to {}", bake_path.display()))?;

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

/// Public entrypoint for the `search` tool: fuzzy search over functions and files.
pub fn search(path: Option<String>, query: String, limit: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let limit = limit.unwrap_or(10);
    let q = query.to_lowercase();

    let mut function_hits: Vec<SearchFunctionHit> = bake
        .functions
        .iter()
        .filter_map(|f| {
            let name = f.name.to_lowercase();
            let file = f.file.to_lowercase();

            let score = if name == q {
                3.0
            } else if name.contains(&q) {
                2.0
            } else if file.contains(&q) {
                1.0
            } else {
                0.0
            };

            if score <= 0.0 {
                return None;
            }

            Some(SearchFunctionHit {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                complexity: f.complexity,
                score,
            })
        })
        .collect();

    function_hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then(b.complexity.cmp(&a.complexity))
    });
    function_hits.truncate(limit);

    let mut file_hits: Vec<SearchFileHit> = bake
        .files
        .iter()
        .filter_map(|f| {
            let path_str = f.path.to_string_lossy().to_lowercase();
            let lang = f.language.to_lowercase();

            let score = if path_str == q {
                2.0
            } else if path_str.contains(&q) {
                1.5
            } else if lang.contains(&q) {
                1.0
            } else {
                0.0
            };

            if score <= 0.0 {
                return None;
            }

            Some(SearchFileHit {
                path: f.path.to_string_lossy().into_owned(),
                language: f.language.clone(),
                bytes: f.bytes,
                score,
            })
        })
        .collect();

    file_hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then(a.path.cmp(&b.path))
    });
    file_hits.truncate(limit);

    let payload = SearchPayload {
        tool: "search",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        query,
        limit,
        function_hits,
        file_hits,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `symbol` tool: detailed lookup by function name.
/// When `include_source` is true, each match includes the function body (lines start_line..end_line).
pub fn symbol(path: Option<String>, name: String, include_source: bool) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let needle = name.to_lowercase();

    let mut matches: Vec<SymbolMatch> = bake
        .functions
        .iter()
        .filter_map(|f| {
            let fname = f.name.to_lowercase();
            if fname == needle || fname.contains(&needle) {
                Some(SymbolMatch {
                    name: f.name.clone(),
                    file: f.file.clone(),
                    start_line: f.start_line,
                    end_line: f.end_line,
                    complexity: f.complexity,
                    source: None,
                })
            } else {
                None
            }
        })
        .collect();

    matches.sort_by(|a, b| {
        // Prefer exact matches, then higher complexity.
        let a_exact = (a.name.to_lowercase() == needle) as i32;
        let b_exact = (b.name.to_lowercase() == needle) as i32;
        b_exact
            .cmp(&a_exact)
            .then(b.complexity.cmp(&a.complexity))
            .then(a.file.cmp(&b.file))
    });

    if include_source {
        for m in &mut matches {
            let full_path = root.join(&m.file);
            if let Ok(content) = fs::read_to_string(&full_path) {
                let all_lines: Vec<&str> = content.lines().collect();
                let total = all_lines.len() as u32;
                let s = (m.start_line.saturating_sub(1) as usize).min(all_lines.len());
                let e = (m.end_line.min(total).saturating_sub(1) as usize).min(all_lines.len());
                if s <= e {
                    m.source = Some(all_lines[s..=e].join("\n"));
                }
            }
        }
    }

    let payload = SymbolPayload {
        tool: "symbol",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        name,
        matches,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `all_endpoints` tool: list Express-style endpoints.
pub fn all_endpoints(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let endpoints: Vec<AllEndpointSummary> = bake
        .endpoints
        .iter()
        .map(|e| AllEndpointSummary {
            method: e.method.clone(),
            path: e.path.clone(),
            file: e.file.clone(),
            handler_name: e.handler_name.clone(),
        })
        .collect();

    let payload = AllEndpointsPayload {
        tool: "all_endpoints",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoints,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `slice` tool: read a specific line range of a file.
pub fn slice(
    path: Option<String>,
    file: String,
    start: u32,
    end: u32,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    if start == 0 || end == 0 || end < start {
        return Err(anyhow!(
            "Invalid range: start and end must be >= 1 and end >= start (got start={}, end={})",
            start,
            end
        ));
    }

    let full_path = root.join(&file);
    let content = fs::read_to_string(&full_path).with_context(|| {
        format!(
            "Failed to read file {} (resolved to {})",
            file,
            full_path.display()
        )
    })?;

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len() as u32;

    let s = start.saturating_sub(1) as usize;
    let e = end.min(total_lines).saturating_sub(1) as usize;

    if s >= all_lines.len() {
        return Err(anyhow!(
            "Start line {} is beyond end of file (total_lines={})",
            start,
            total_lines
        ));
    }

    let mut lines = Vec::new();
    for i in s..=e {
        lines.push(all_lines[i].to_string());
    }

    let payload = SlicePayload {
        tool: "slice",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end: end.min(total_lines),
        total_lines,
        lines,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `api_surface` tool: exported API summary by module (TypeScript-only for now).
pub fn api_surface(
    path: Option<String>,
    package: Option<String>,
    limit: Option<usize>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let limit = limit.unwrap_or(20);
    let package_filter = package.clone().map(|p| p.to_lowercase());

    use std::collections::BTreeMap;
    let mut modules: BTreeMap<String, Vec<ApiSurfaceFunction>> = BTreeMap::new();

    for f in &bake.functions {
        let module = module_from_path(&f.file);
        if let Some(ref pf) = package_filter {
            if !module.to_lowercase().contains(pf) && !f.file.to_lowercase().contains(pf) {
                continue;
            }
        }

        modules
            .entry(module)
            .or_default()
            .push(ApiSurfaceFunction {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                complexity: f.complexity,
            });
    }

    let mut modules_vec: Vec<ApiSurfaceModule> = modules
        .into_iter()
        .map(|(module, mut functions)| {
            functions.sort_by(|a, b| b.complexity.cmp(&a.complexity));
            functions.truncate(limit);
            ApiSurfaceModule { module, functions }
        })
        .collect();

    modules_vec.sort_by(|a, b| a.module.cmp(&b.module));

    let payload = ApiSurfacePayload {
        tool: "api_surface",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        package,
        limit,
        modules: modules_vec,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `file_functions` tool: per-file function overview.
pub fn file_functions(
    path: Option<String>,
    file: String,
    include_summaries: Option<bool>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let rel_file = file.clone();

    let mut funcs: Vec<FileFunctionSummary> = bake
        .functions
        .iter()
        .filter(|f| f.file == rel_file)
        .map(|f| FileFunctionSummary {
            name: f.name.clone(),
            start_line: f.start_line,
            end_line: f.end_line,
            complexity: f.complexity,
            // For now we do not compute summaries; this can be extended later.
            summary: None,
        })
        .collect();

    funcs.sort_by(|a, b| a.start_line.cmp(&b.start_line));

    let payload = FileFunctionsPayload {
        tool: "file_functions",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        include_summaries: include_summaries.unwrap_or(true),
        functions: funcs,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `supersearch` tool: text-based search over source files.
///
/// This first implementation is line-oriented and uses the bake index to
/// decide which files to scan. It is not yet fully AST-aware but keeps the
/// interface compatible with the PRD.
pub fn supersearch(
    path: Option<String>,
    query: String,
    context: String,
    pattern: String,
    exclude_tests: Option<bool>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let exclude_tests = exclude_tests.unwrap_or(false);
    let q = query.to_lowercase();

    // Normalize context/pattern to known values; fall back to "all" if unknown.
    let context_norm = match context.as_str() {
        "all" | "strings" | "comments" | "identifiers" => context.clone(),
        _ => "all".to_string(),
    };
    let pattern_norm = match pattern.as_str() {
        "all" | "call" | "assign" | "return" => pattern.clone(),
        _ => "all".to_string(),
    };

    let mut matches = Vec::new();

    for file in &bake.files {
        let lang = file.language.as_str();
        if !matches!(lang, "typescript" | "javascript" | "rust" | "python") {
            continue;
        }

        let path_str = file.path.to_string_lossy();
        if exclude_tests && (path_str.contains("test") || path_str.contains("spec")) {
            continue;
        }

        let full_path = root.join(&file.path);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let file_rel = file.path.to_string_lossy().into_owned();

        let mut used_ast = false;
        if let Some(analyzer) = crate::lang::find_analyzer(lang) {
            if analyzer.supports_ast_search() {
                let mut file_matches =
                    analyzer.ast_search(&content, &q, &context_norm, &pattern_norm);
                // Deduplicate by line — the AST walk can emit multiple nodes per line.
                file_matches.sort_by_key(|m| m.line);
                file_matches.dedup_by_key(|m| m.line);
                for m in file_matches {
                    matches.push(SupersearchMatch {
                        file: file_rel.clone(),
                        line: m.line,
                        snippet: m.snippet,
                    });
                }
                used_ast = true;
            }
        }
        if !used_ast {
            // Fallback: line-oriented text search for unsupported languages (html, yaml, etc.).
            for (idx, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    matches.push(SupersearchMatch {
                        file: file_rel.clone(),
                        line: (idx + 1) as u32,
                        snippet: line.trim().to_string(),
                    });
                }
            }
        }
    }

    let payload = SupersearchPayload {
        tool: "supersearch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        query,
        context,
        pattern,
        exclude_tests,
        matches,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `package_summary` tool: summarize a module/directory.
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
            functions.push(PackageFunctionSummary {
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
            endpoints.push(PackageEndpointSummary {
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
pub fn architecture_map(path: Option<String>, intent: String) -> Result<String> {
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

        if path_str.contains("routes") || path_str.contains("controllers") {
            if !entry.roles.contains(&"http-endpoints".to_string()) {
                entry.roles.push("http-endpoints".to_string());
            }
        }
        if path_str.contains("services") {
            if !entry.roles.contains(&"services".to_string()) {
                entry.roles.push("services".to_string());
            }
        }
        if path_str.contains("models") || path_str.contains("entities") {
            if !entry.roles.contains(&"models".to_string()) {
                entry.roles.push("models".to_string());
            }
        }
    }

    let mut dirs: Vec<ArchitectureDir> = directories.into_values().collect();
    dirs.sort_by(|a, b| a.path.cmp(&b.path));

    // Very simple suggestion heuristic based on intent keywords.
    let intent_lc = intent.to_lowercase();
    let mut suggestions = Vec::new();
    for dir in &dirs {
        let mut score = 0u32;
        let path_lc = dir.path.to_lowercase();

        if intent_lc.contains("handler") || intent_lc.contains("endpoint") {
            if path_lc.contains("routes") || path_lc.contains("controllers") {
                score += 5;
            }
        }
        if intent_lc.contains("service") {
            if path_lc.contains("service") {
                score += 5;
            }
        }
        if intent_lc.contains("model") {
            if path_lc.contains("model") {
                score += 5;
            }
        }

        if score > 0 {
            suggestions.push(ArchitectureSuggestion {
                directory: dir.path.clone(),
                score,
                rationale: format!("Matches intent \"{}\" based on directory name/role.", intent),
            });
        }
    }
    suggestions.sort_by(|a, b| b.score.cmp(&a.score));

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

/// Public entrypoint for the `crud_operations` tool.
pub fn crud_operations(path: Option<String>, entity: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let entity_filter = entity.clone().map(|e| e.to_lowercase());
    let mut entities: BTreeMap<String, CrudEntitySummary> = BTreeMap::new();

    for e in &bake.endpoints {
        let path_seg = infer_entity_from_path(&e.path);
        if path_seg.is_empty() {
            continue;
        }
        if let Some(ref ef) = entity_filter {
            if !path_seg.to_lowercase().contains(ef) {
                continue;
            }
        }

        let entry = entities.entry(path_seg.clone()).or_insert_with(|| CrudEntitySummary {
            entity: path_seg.clone(),
            operations: Vec::new(),
        });

        let op = match e.method.as_str() {
            "GET" => "read",
            "POST" => "create",
            "PUT" | "PATCH" => "update",
            "DELETE" => "delete",
            _ => "other",
        };

        entry.operations.push(CrudOperation {
            operation: op.to_string(),
            method: e.method.clone(),
            path: e.path.clone(),
            file: e.file.clone(),
        });
    }

    let mut entities_vec: Vec<CrudEntitySummary> = entities.into_values().collect();
    entities_vec.sort_by(|a, b| a.entity.cmp(&b.entity));

    let payload = CrudOperationsPayload {
        tool: "crud_operations",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        entity,
        entities: entities_vec,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `api_trace` tool.
pub fn api_trace(
    path: Option<String>,
    endpoint: String,
    method: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let method_lc = method.clone().map(|m| m.to_uppercase());
    let endpoint_lc = endpoint.to_lowercase();

    let mut traces = Vec::new();

    for e in &bake.endpoints {
        if !e.path.to_lowercase().contains(&endpoint_lc) {
            continue;
        }
        if let Some(ref m) = method_lc {
            if &e.method != m {
                continue;
            }
        }

        traces.push(ApiTraceEntry {
            method: e.method.clone(),
            path: e.path.clone(),
            file: e.file.clone(),
            handler_name: e.handler_name.clone(),
        });
    }

    let payload = ApiTracePayload {
        tool: "api_trace",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        endpoint,
        method: method_lc,
        traces,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `find_docs` tool.
pub fn find_docs(path: Option<String>, doc_type: String) -> Result<String> {
    let root = resolve_project_root(path)?;

    let mut matches = Vec::new();

    fn walk_docs(dir: &Path, root: &Path, doc_type: &str, out: &mut Vec<DocMatch>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if matches!(name, ".git" | "node_modules" | "dist" | "build" | "target") {
                        continue;
                    }
                }
                walk_docs(&path, root, doc_type, out)?;
            } else if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();

                let is_match = match doc_type {
                    "readme" => name.to_lowercase().starts_with("readme"),
                    "env" => name.starts_with(".env") || name.to_lowercase().contains("env"),
                    "config" => name.to_lowercase().contains("config") || name.ends_with(".json"),
                    "docker" => name.to_lowercase().contains("docker"),
                    "all" => true,
                    _ => false,
                };

                if is_match {
                    let snippet = fs::read_to_string(&path)
                        .ok()
                        .map(|s| s.lines().take(5).collect::<Vec<_>>().join("\n"));
                    out.push(DocMatch {
                        path: rel,
                        snippet,
                    });
                }
            }
        }
        Ok(())
    }

    walk_docs(&root, &root, &doc_type, &mut matches)?;

    let payload = FindDocsPayload {
        tool: "find_docs",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        doc_type,
        matches,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `patch` tool.
pub fn patch(
    path: Option<String>,
    file: String,
    start: u32,
    end: u32,
    new_content: String,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    if start == 0 || end == 0 || end < start {
        return Err(anyhow!(
            "Invalid range: start and end must be >= 1 and end >= start (got start={}, end={})",
            start,
            end
        ));
    }

    let full_path = root.join(&file);
    let content = fs::read_to_string(&full_path).with_context(|| {
        format!(
            "Failed to read file {} (resolved to {})",
            file,
            full_path.display()
        )
    })?;

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let total_lines = lines.len() as u32;

    let s = start.saturating_sub(1) as usize;
    let e = end.min(total_lines).saturating_sub(1) as usize;

    if s >= lines.len() {
        return Err(anyhow!(
            "Start line {} is beyond end of file (total_lines={})",
            start,
            total_lines
        ));
    }

    let replacement_lines: Vec<String> = new_content.lines().map(|s| s.to_string()).collect();

    lines.splice(s..=e, replacement_lines.into_iter());

    let new_text = lines.join("\n");
    fs::write(&full_path, new_text).with_context(|| {
        format!(
            "Failed to write patched file {} (resolved to {})",
            file,
            full_path.display()
        )
    })?;

    let payload = PatchPayload {
        tool: "patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end,
        total_lines,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

#[derive(Serialize)]
struct LlmInstructionsPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    languages: Vec<String>,
    files_indexed: usize,
    guidance: String,
}

#[derive(Serialize)]
struct ShakePayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    languages: Vec<String>,
    files_indexed: usize,
    notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_functions: Option<Vec<ShakeFunctionSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    express_endpoints: Option<Vec<ShakeEndpointSummary>>,
}

#[derive(Serialize)]
struct BakeSummary {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    bake_path: PathBuf,
    files_indexed: usize,
    languages: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct BakeIndex {
    version: String,
    project_root: PathBuf,
    languages: BTreeSet<String>,
    files: Vec<BakeFile>,
    #[serde(default)]
    functions: Vec<crate::lang::IndexedFunction>,
    #[serde(default)]
    endpoints: Vec<crate::lang::IndexedEndpoint>,
}

#[derive(Serialize, Deserialize)]
struct BakeFile {
    path: PathBuf,
    language: String,
    bytes: u64,
}

#[derive(Serialize)]
struct ShakeFunctionSummary {
    name: String,
    file: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
}

#[derive(Serialize)]
struct ShakeEndpointSummary {
    method: String,
    path: String,
    file: String,
    handler_name: Option<String>,
}

#[derive(Serialize)]
struct SearchPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    query: String,
    limit: usize,
    function_hits: Vec<SearchFunctionHit>,
    file_hits: Vec<SearchFileHit>,
}

#[derive(Serialize)]
struct SearchFunctionHit {
    name: String,
    file: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
    score: f32,
}

#[derive(Serialize)]
struct SearchFileHit {
    path: String,
    language: String,
    bytes: u64,
    score: f32,
}

#[derive(Serialize)]
struct SymbolPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    name: String,
    matches: Vec<SymbolMatch>,
}

#[derive(Serialize)]
struct SymbolMatch {
    name: String,
    file: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

#[derive(Serialize)]
struct AllEndpointsPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    endpoints: Vec<AllEndpointSummary>,
}

#[derive(Serialize)]
struct AllEndpointSummary {
    method: String,
    path: String,
    file: String,
    handler_name: Option<String>,
}

#[derive(Serialize)]
struct SupersearchPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    query: String,
    context: String,
    pattern: String,
    exclude_tests: bool,
    matches: Vec<SupersearchMatch>,
}

#[derive(Serialize)]
struct SupersearchMatch {
    file: String,
    line: u32,
    snippet: String,
}

#[derive(Serialize)]
struct PackageSummaryPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    package: String,
    files: Vec<PackageFileSummary>,
    functions: Vec<PackageFunctionSummary>,
    endpoints: Vec<PackageEndpointSummary>,
}

#[derive(Serialize)]
struct PackageFileSummary {
    path: String,
    language: String,
    bytes: u64,
}

#[derive(Serialize)]
struct PackageFunctionSummary {
    name: String,
    file: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
}

#[derive(Serialize)]
struct PackageEndpointSummary {
    method: String,
    path: String,
    file: String,
    handler_name: Option<String>,
}

#[derive(Serialize)]
struct ArchitectureMapPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    intent: String,
    directories: Vec<ArchitectureDir>,
    suggestions: Vec<ArchitectureSuggestion>,
}

#[derive(Serialize)]
struct ArchitectureDir {
    path: String,
    file_count: u32,
    languages: BTreeSet<String>,
    roles: Vec<String>,
}

#[derive(Serialize)]
struct ArchitectureSuggestion {
    directory: String,
    score: u32,
    rationale: String,
}

#[derive(Serialize)]
struct SuggestPlacementPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    function_name: String,
    function_type: String,
    related_to: Option<String>,
    suggestions: Vec<PlacementSuggestion>,
}

#[derive(Serialize)]
struct PlacementSuggestion {
    file: String,
    score: u32,
    rationale: String,
}

#[derive(Serialize)]
struct CrudOperationsPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    entity: Option<String>,
    entities: Vec<CrudEntitySummary>,
}

#[derive(Serialize)]
struct CrudEntitySummary {
    entity: String,
    operations: Vec<CrudOperation>,
}

#[derive(Serialize)]
struct CrudOperation {
    operation: String,
    method: String,
    path: String,
    file: String,
}

#[derive(Serialize)]
struct ApiTracePayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    endpoint: String,
    method: Option<String>,
    traces: Vec<ApiTraceEntry>,
}

#[derive(Serialize)]
struct ApiTraceEntry {
    method: String,
    path: String,
    file: String,
    handler_name: Option<String>,
}

#[derive(Serialize)]
struct FindDocsPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    doc_type: String,
    matches: Vec<DocMatch>,
}

#[derive(Serialize)]
struct DocMatch {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet: Option<String>,
}

#[derive(Serialize)]
struct PatchPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    file: String,
    start: u32,
    end: u32,
    total_lines: u32,
}

#[derive(Serialize)]
struct SlicePayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    file: String,
    start: u32,
    end: u32,
    total_lines: u32,
    lines: Vec<String>,
}

#[derive(Serialize)]
struct ApiSurfacePayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    package: Option<String>,
    limit: usize,
    modules: Vec<ApiSurfaceModule>,
}

#[derive(Serialize)]
struct ApiSurfaceModule {
    module: String,
    functions: Vec<ApiSurfaceFunction>,
}

#[derive(Serialize)]
struct ApiSurfaceFunction {
    name: String,
    file: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
}

#[derive(Serialize)]
struct FileFunctionsPayload {
    tool: &'static str,
    version: &'static str,
    project_root: PathBuf,
    file: String,
    include_summaries: bool,
    functions: Vec<FileFunctionSummary>,
}

#[derive(Serialize)]
struct FileFunctionSummary {
    name: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

struct Snapshot {
    languages: BTreeSet<String>,
    files_indexed: usize,
}

fn project_snapshot(root: &PathBuf) -> Result<Snapshot> {
    let mut languages = BTreeSet::new();
    let mut files_indexed = 0usize;

    fn walk(dir: &Path, languages: &mut BTreeSet<String>, count: &mut usize) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip common heavy/irrelevant directories for a quick snapshot.
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if matches!(
                        name,
                        ".git" | "node_modules" | "target" | "dist" | "build" | "__pycache__"
                    ) {
                        continue;
                    }
                }
                walk(&path, languages, count)?;
            } else if path.is_file() {
                *count += 1;
                let lang = detect_language(&path);
                if lang != "other" {
                    languages.insert(lang.to_string());
                }
            }
        }
        Ok(())
    }

    walk(root, &mut languages, &mut files_indexed)?;

    Ok(Snapshot {
        languages,
        files_indexed,
    })
}

fn load_bake_index(root: &PathBuf) -> Result<Option<BakeIndex>> {
    let bake_path = root.join("bakes").join("latest").join("bake.json");
    if !bake_path.exists() {
        return Ok(None);
    }

    let data =
        fs::read_to_string(&bake_path).with_context(|| format!("Failed to read {}", bake_path.display()))?;
    let bake: BakeIndex = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse bake index from {}", bake_path.display()))?;
    Ok(Some(bake))
}

fn build_bake_index(root: &PathBuf) -> Result<BakeIndex> {
    let mut languages = BTreeSet::new();
    let mut files = Vec::new();
    let mut functions = Vec::new();
    let mut endpoints = Vec::new();

    fn walk(
        dir: &Path,
        root: &Path,
        languages: &mut BTreeSet<String>,
        files: &mut Vec<BakeFile>,
        functions: &mut Vec<crate::lang::IndexedFunction>,
        endpoints: &mut Vec<crate::lang::IndexedEndpoint>,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if matches!(
                        name,
                        ".git" | "node_modules" | "target" | "dist" | "build" | "__pycache__"
                    ) {
                        continue;
                    }
                }
                walk(&path, root, languages, files, functions, endpoints)?;
            } else if path.is_file() {
                let meta = entry.metadata()?;
                let bytes = meta.len();
                let lang = detect_language(&path);
                if lang != "other" {
                    languages.insert(lang.to_string());
                }
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                files.push(BakeFile {
                    path: rel,
                    language: lang.to_string(),
                    bytes,
                });

                if let Some(analyzer) = crate::lang::find_analyzer(lang) {
                    let (funcs, eps) = analyzer.analyze_file(root, &path)?;
                    functions.extend(funcs);
                    endpoints.extend(eps);
                }
            }
        }
        Ok(())
    }

    walk(
        root.as_path(),
        root.as_path(),
        &mut languages,
        &mut files,
        &mut functions,
        &mut endpoints,
    )?;

    Ok(BakeIndex {
        version: env!("CARGO_PKG_VERSION").to_string(),
        project_root: root.clone(),
        languages,
        files,
        functions,
        endpoints,
    })
}

fn resolve_project_root(path: Option<String>) -> Result<PathBuf> {
    if let Some(p) = path {
        let pb = PathBuf::from(p);
        let meta = fs::metadata(&pb).with_context(|| format!("Failed to stat path: {}", pb.display()))?;
        if !meta.is_dir() {
            anyhow::bail!("Provided path is not a directory: {}", pb.display());
        }
        return Ok(pb);
    }

    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    Ok(cwd)
}

fn default_guidance_text() -> String {
    r#"yoyo is a local code-intelligence engine.

Typical workflow:
- Call `llm_instructions` once to learn the project layout and available tools.
- Use `shake` (repository overview) to understand the architecture.
- Use `search` and `symbol` to navigate functions and types.
- Use `slice` and `file_functions` to read and summarize individual files.
- Use `supersearch` for AST-aware searches (prefer it over grep/ripgrep).
- Use `api_surface`, `all_endpoints`, and `api_trace` to understand APIs.
- Use `architecture_map` and `suggest_placement` when adding new features.
"#
    .to_string()
}

fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("kt") => "kotlin",
        Some("php") => "php",
        Some("rb") => "ruby",
        Some("swift") => "swift",
        Some("scala") => "scala",
        Some("vue") => "vue",
        Some("sql") => "sql",
        Some("tf") | Some("tfvars") => "terraform",
        Some("yml") | Some("yaml") => "yaml",
        Some("json") => "json",
        Some("html") => "html",
        _ => "other",
    }
}

fn module_from_path(path: &str) -> String {
    // Heuristic: use directory portion of the path as the "module".
    if let Some((dir, _file)) = path.rsplit_once('/') {
        dir.to_string()
    } else {
        ".".to_string()
    }
}

fn infer_entity_from_path(path: &str) -> String {
    // Heuristic: first non-empty segment that is not ":"-parameter.
    for seg in path.split('/') {
        if seg.is_empty() {
            continue;
        }
        if seg.starts_with(':') {
            continue;
        }
        return seg.to_string();
    }
    String::new()
}

