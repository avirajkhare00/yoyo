use std::cmp::Ordering;
use std::fs;

use anyhow::{anyhow, Result};

use super::types::{
    FileFunctionSummary, FileFunctionsPayload, SearchFileHit, SearchFunctionHit, SearchPayload,
    SupersearchMatch, SupersearchPayload, SymbolMatch, SymbolPayload,
};
use super::util::{load_bake_index, resolve_project_root};

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
                kind: None,
            })
        })
        .chain(bake.types.iter().filter_map(|t| {
            let name = t.name.to_lowercase();
            let file = t.file.to_lowercase();

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
                name: t.name.clone(),
                file: t.file.clone(),
                start_line: t.start_line,
                end_line: t.end_line,
                complexity: 0,
                score,
                kind: Some(t.kind.clone()),
            })
        }))
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
                    kind: None,
                    source: None,
                })
            } else {
                None
            }
        })
        .chain(bake.types.iter().filter_map(|t| {
            let tname = t.name.to_lowercase();
            if tname == needle || tname.contains(&needle) {
                Some(SymbolMatch {
                    name: t.name.clone(),
                    file: t.file.clone(),
                    start_line: t.start_line,
                    end_line: t.end_line,
                    complexity: 0,
                    kind: Some(t.kind.clone()),
                    source: None,
                })
            } else {
                None
            }
        }))
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

    // Cache analyzers by language to avoid reallocating all 4 boxed analyzers per file.
    let mut analyzer_cache: std::collections::HashMap<&str, Option<Box<dyn crate::lang::LanguageAnalyzer>>> =
        std::collections::HashMap::new();

    for file in &bake.files {
        let lang = file.language.as_str();
        if !matches!(lang, "typescript" | "javascript" | "rust" | "python" | "go") {
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
        let analyzer = analyzer_cache
            .entry(lang)
            .or_insert_with(|| crate::lang::find_analyzer(lang));
        if let Some(analyzer) = analyzer {
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
