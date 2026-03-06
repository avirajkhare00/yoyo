use std::fs;

use anyhow::{anyhow, Result};

use super::types::{
    FileFunctionSummary, FileFunctionsPayload, SupersearchMatch, SupersearchPayload, SymbolMatch,
    SymbolPayload,
};
use super::util::{load_bake_index, resolve_project_root};

/// Public entrypoint for the `symbol` tool: detailed lookup by function name.
/// When `include_source` is true, each match includes the function body (lines start_line..end_line).
pub fn symbol(
    path: Option<String>,
    name: String,
    include_source: bool,
    file: Option<String>,
    limit: Option<usize>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let needle = name.to_lowercase();
    let file_filter = file.as_deref().map(str::to_lowercase);

    // Build set of project-defined function names for call filtering (#47).
    let project_fns: std::collections::HashSet<String> = bake
        .functions
        .iter()
        .map(|f| f.name.to_lowercase())
        .collect();

    // Common single-word Rust/Go/Python identifiers that are overwhelmingly stdlib/trait
    // methods even when a project happens to define a function with the same name.
    // Using a denylist is the most reliable signal without AST type-resolution.
    const STDLIB_NOISE: &[&str] = &[
        "clone", "map", "filter", "from", "into", "len", "is_empty", "push",
        "pop", "contains", "get", "set", "default", "unwrap", "expect",
        "is_dir", "is_file", "is_symlink", "metadata", "path", "send", "recv",
        "iter", "iter_mut", "into_iter", "collect", "fold", "any", "all",
        "find", "flatten", "chain", "zip", "enumerate", "take", "skip",
        "to_string", "as_str", "as_bytes", "trim", "split", "join",
        "chars", "lines", "parse", "is_some", "is_none", "is_ok", "is_err",
        "ok", "err", "and_then", "or_else", "map_err", "unwrap_or",
        "write", "flush", "read", "open", "seek", "lock", "drop",
        "fmt", "hash", "eq", "cmp", "partial_cmp", "borrow", "deref",
        "index", "add", "sub", "mul", "div", "rem", "neg", "not",
        "run", "new", "close", "insert", "remove", "clear", "retain",
        "extend", "append", "drain", "sort", "dedup", "reverse",
    ];

    // Count incoming calls per callee name — used to rank primary match (#46).
    let mut incoming: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for f in &bake.functions {
        for c in &f.calls {
            *incoming.entry(c.callee.to_lowercase()).or_insert(0) += 1;
        }
    }

    let mut matches: Vec<SymbolMatch> = bake
        .functions
        .iter()
        .filter_map(|f| {
            let fname = f.name.to_lowercase();
            if fname == needle || fname.contains(&needle) {
                // Filter calls to project-defined callees, excluding common
                // stdlib/trait method names that produce false positives (#47).
                let calls: Vec<_> = f.calls.iter()
                    .filter(|c| {
                        let lc = c.callee.to_lowercase();
                        project_fns.contains(&lc) && !STDLIB_NOISE.contains(&lc.as_str())
                    })
                    .cloned()
                    .collect();
                Some(SymbolMatch {
                    name: f.name.clone(),
                    file: f.file.clone(),
                    start_line: f.start_line,
                    end_line: f.end_line,
                    complexity: f.complexity,
                    primary: false, // set below after sorting
                    kind: None,
                    source: None,
                    visibility: Some(f.visibility.clone()),
                    module_path: if f.module_path.is_empty() { None } else { Some(f.module_path.clone()) },
                    qualified_name: if f.qualified_name.is_empty() { None } else { Some(f.qualified_name.clone()) },
                    calls,
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
                    primary: false,
                    kind: Some(t.kind.clone()),
                    source: None,
                    visibility: Some(t.visibility.clone()),
                    module_path: if t.module_path.is_empty() { None } else { Some(t.module_path.clone()) },
                    qualified_name: None,
                    calls: vec![],
                })
            } else {
                None
            }
        }))
        .collect();

    // Narrow by file substring when caller specifies one.
    if let Some(ref ff) = file_filter {
        matches.retain(|m| m.file.to_lowercase().contains(ff.as_str()));
    }

    matches.sort_by(|a, b| {
        // Prefer exact name match, then most-called (incoming), then complexity.
        let a_exact = (a.name.to_lowercase() == needle) as i32;
        let b_exact = (b.name.to_lowercase() == needle) as i32;
        let a_in = incoming.get(&a.name.to_lowercase()).copied().unwrap_or(0);
        let b_in = incoming.get(&b.name.to_lowercase()).copied().unwrap_or(0);
        b_exact
            .cmp(&a_exact)
            .then(b_in.cmp(&a_in))
            .then(b.complexity.cmp(&a.complexity))
            .then(a.file.cmp(&b.file))
    });

    // Mark the first exact-name match as primary.
    if let Some(m) = matches.iter_mut().find(|m| m.name.to_lowercase() == needle) {
        m.primary = true;
    }

    matches.truncate(limit.unwrap_or(20));

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
    file_filter: Option<String>,
    limit: Option<usize>,
) -> Result<String> {
    use rayon::prelude::*;

    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let exclude_tests = exclude_tests.unwrap_or(false);
    let q = query.to_lowercase();
    let ff = file_filter.as_deref().map(str::to_lowercase);

    let context_norm = match context.as_str() {
        "all" | "strings" | "comments" | "identifiers" => context.clone(),
        _ => "all".to_string(),
    };
    let pattern_norm = match pattern.as_str() {
        "all" | "call" | "assign" | "return" => pattern.clone(),
        _ => "all".to_string(),
    };

    let mut matches: Vec<SupersearchMatch> = bake
        .files
        .par_iter()
        .filter(|file| {
            let lang = file.language.as_str();
            if !matches!(lang, "typescript" | "javascript" | "rust" | "python" | "go") {
                return false;
            }
            let path_str = file.path.to_string_lossy();
            if exclude_tests && (path_str.contains("test") || path_str.contains("spec")) {
                return false;
            }
            if let Some(ref f) = ff {
                if !path_str.to_lowercase().contains(f.as_str()) {
                    return false;
                }
            }
            true
        })
        .flat_map(|file| {
            let lang = file.language.as_str();
            let full_path = root.join(&file.path);
            let content = match fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(_) => return vec![],
            };
            let file_rel = file.path.to_string_lossy().into_owned();
            let mut file_matches = Vec::new();

            let analyzer = crate::lang::find_analyzer(lang);
            let mut used_ast = false;
            if let Some(analyzer) = analyzer {
                if analyzer.supports_ast_search() {
                    let mut ast_matches =
                        analyzer.ast_search(&content, &q, &context_norm, &pattern_norm);
                    ast_matches.sort_by_key(|m| m.line);
                    ast_matches.dedup_by_key(|m| m.line);
                    for m in ast_matches {
                        file_matches.push(SupersearchMatch {
                            file: file_rel.clone(),
                            line: m.line,
                            snippet: m.snippet,
                        });
                    }
                    used_ast = true;
                }
            }
            if !used_ast {
                for (idx, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&q) {
                        file_matches.push(SupersearchMatch {
                            file: file_rel.clone(),
                            line: (idx + 1) as u32,
                            snippet: line.trim().to_string(),
                        });
                    }
                }
            }
            file_matches
        })
        .collect();

    matches.truncate(limit.unwrap_or(200));

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

    Ok(serde_json::to_string_pretty(&payload)?)
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
