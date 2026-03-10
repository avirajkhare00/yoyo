use std::fs;

use anyhow::{anyhow, Result};

use super::types::{
    FileFunctionSummary, FileFunctionsPayload, SemanticMatch, SemanticSearchPayload,
    SupersearchMatch, SupersearchPayload, SymbolMatch, SymbolPayload,
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
    stdlib: bool,
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
        .filter(|f| !f.is_stdlib)
        .filter_map(|f| {
            let fname = f.name.to_lowercase();
            if fname == needle || fname.contains(&needle) {
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
                    primary: false,
                    kind: None,
                    source: None,
                    visibility: Some(f.visibility.clone()),
                    module_path: if f.module_path.is_empty() { None } else { Some(f.module_path.clone()) },
                    qualified_name: if f.qualified_name.is_empty() { None } else { Some(f.qualified_name.clone()) },
                    calls,
                    parent_type: f.parent_type.clone(),
                    implements: vec![],
                    implementors: vec![],
                    fields: vec![],
                    is_stdlib: false,
                    sig_hash: f.sig_hash.clone(),
                })
            } else {
                None
            }
        })
        .chain(bake.types.iter().filter(|t| !t.is_stdlib).filter_map(|t| {
            let tname = t.name.to_lowercase();
            if tname == needle || tname.contains(&needle) {
                let implements: Vec<String> = bake.impls.iter()
                    .filter(|i| i.type_name.to_lowercase() == tname)
                    .filter_map(|i| i.trait_name.clone())
                    .collect();
                let implementors: Vec<String> = if t.kind == "trait" {
                    let mut seen = std::collections::HashSet::new();
                    bake.impls.iter()
                        .filter(|i| i.trait_name.as_deref().map(|tr| tr.to_lowercase()) == Some(tname.clone()))
                        .map(|i| i.type_name.clone())
                        .filter(|n| seen.insert(n.clone()))
                        .collect()
                } else {
                    vec![]
                };
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
                    parent_type: None,
                    implements,
                    implementors,
                    fields: t.fields.clone(),
                    is_stdlib: false,
                    sig_hash: None,
                })
            } else {
                None
            }
        }))
        .collect();

    if let Some(ref ff) = file_filter {
        matches.retain(|m| m.file.to_lowercase().contains(ff.as_str()));
    }

    if stdlib {
        let stdlib_matches = symbol_stdlib(&needle, include_source);
        matches.extend(stdlib_matches);
    }

    matches.sort_by(|a, b| {
        let a_exact = (a.name.to_lowercase() == needle) as i32;
        let b_exact = (b.name.to_lowercase() == needle) as i32;
        let a_in = incoming.get(&a.name.to_lowercase()).copied().unwrap_or(0);
        let b_in = incoming.get(&b.name.to_lowercase()).copied().unwrap_or(0);
        let a_stdlib = a.is_stdlib as i32;
        let b_stdlib = b.is_stdlib as i32;
        a_stdlib
            .cmp(&b_stdlib)
            .then(b_exact.cmp(&a_exact))
            .then(b_in.cmp(&a_in))
            .then(b.complexity.cmp(&a.complexity))
            .then(a.file.cmp(&b.file))
    });

    if let Some(m) = matches.iter_mut().find(|m| m.name.to_lowercase() == needle) {
        m.primary = true;
    }

    matches.truncate(limit.unwrap_or(20));

    if include_source {
        for m in &mut matches {
            if m.source.is_some() { continue; }
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

    Ok(serde_json::to_string_pretty(&payload)?)
}

/// On-demand stdlib lookup. Walks installed toolchain stdlib dirs (Zig/Go/Rust),
/// fast-scans files for `needle`, parses candidates, returns matches tagged `is_stdlib: true`.
fn symbol_stdlib(needle: &str, include_source: bool) -> Vec<SymbolMatch> {
    let stdlib_paths = super::util::detect_stdlib_paths();
    let mut matches = Vec::new();
    for (lang, stdlib_dir) in &stdlib_paths {
        let analyzer = match crate::lang::find_analyzer(lang) {
            Some(a) => a,
            None => continue,
        };
        walk_stdlib_dir(&stdlib_dir, needle, &*analyzer, &stdlib_dir, include_source, &mut matches);
    }
    matches
}

fn walk_stdlib_dir(
    dir: &std::path::Path,
    needle: &str,
    analyzer: &dyn crate::lang::LanguageAnalyzer,
    root: &std::path::Path,
    include_source: bool,
    matches: &mut Vec<SymbolMatch>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        let path = entry.path();
        if path.is_dir() {
            walk_stdlib_dir(&path, needle, analyzer, root, include_source, matches);
        } else if path.is_file() {
            let content = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !content.to_lowercase().contains(needle) {
                continue;
            }
            let (funcs, _, types, _) = match analyzer.analyze_file(root, &path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
            let lines: Vec<&str> = content.lines().collect();

            for f in &funcs {
                let fname = f.name.to_lowercase();
                if fname == needle || fname.contains(needle) {
                    let src = if include_source {
                        let s = (f.start_line.saturating_sub(1) as usize).min(lines.len());
                        let e = (f.end_line.saturating_sub(1) as usize).min(lines.len());
                        if s <= e { Some(lines[s..=e].join("\n")) } else { None }
                    } else { None };
                    matches.push(SymbolMatch {
                        name: f.name.clone(),
                        file: rel.clone(),
                        start_line: f.start_line,
                        end_line: f.end_line,
                        complexity: f.complexity,
                        primary: false,
                        kind: None,
                        source: src,
                        visibility: Some(f.visibility.clone()),
                        module_path: if f.module_path.is_empty() { None } else { Some(f.module_path.clone()) },
                        qualified_name: if f.qualified_name.is_empty() { None } else { Some(f.qualified_name.clone()) },
                        calls: vec![],
                        parent_type: f.parent_type.clone(),
                        implements: vec![],
                        implementors: vec![],
                        fields: vec![],
                        is_stdlib: true,
                        sig_hash: f.sig_hash.clone(),
                    });
                }
            }
            for t in &types {
                let tname = t.name.to_lowercase();
                if tname == needle || tname.contains(needle) {
                    let src = if include_source {
                        let s = (t.start_line.saturating_sub(1) as usize).min(lines.len());
                        let e = (t.end_line.saturating_sub(1) as usize).min(lines.len());
                        if s <= e { Some(lines[s..=e].join("\n")) } else { None }
                    } else { None };
                    matches.push(SymbolMatch {
                        name: t.name.clone(),
                        file: rel.clone(),
                        start_line: t.start_line,
                        end_line: t.end_line,
                        complexity: 0,
                        primary: false,
                        kind: Some(t.kind.clone()),
                        source: src,
                        visibility: Some(t.visibility.clone()),
                        module_path: if t.module_path.is_empty() { None } else { Some(t.module_path.clone()) },
                        qualified_name: None,
                        calls: vec![],
                        parent_type: None,
                        implements: vec![],
                        implementors: vec![],
                        fields: t.fields.clone(),
                        is_stdlib: true,
                        sig_hash: None,
                    });
                }
            }
        }
    }
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
            // Never search stdlib files by default.
            if file.origin == "stdlib" {
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

/// Split a symbol/query string into lowercase tokens on `_`, `-`, space, `.`, `/`, `:`
/// and camelCase boundaries. Tokens shorter than 2 chars are dropped.
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in s.chars() {
        if matches!(c, '_' | '-' | ' ' | '/' | '.' | ':') {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else if c.is_uppercase() && !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
            current.push(c);
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens.into_iter().filter(|t| t.len() >= 2).collect()
}

/// Score a single function against the query tokens.
/// Weights: name token ×3, callee name ×1, file path ×0.5 — all TF-IDF scaled.
fn score_fn<F: Fn(&str) -> f32>(
    func: &crate::lang::IndexedFunction,
    query_tokens: &[String],
    idf: F,
) -> f32 {
    let name_set: std::collections::HashSet<String> = tokenize(&func.name).into_iter().collect();
    let callee_set: std::collections::HashSet<String> = func
        .calls
        .iter()
        .flat_map(|c| tokenize(&c.callee))
        .collect();
    let file_set: std::collections::HashSet<String> = tokenize(&func.file).into_iter().collect();

    let mut score = 0.0f32;
    for qt in query_tokens {
        let w = idf(qt);
        if name_set.contains(qt)   { score += 3.0 * w; }
        if callee_set.contains(qt) { score += 1.0 * w; }
        if file_set.contains(qt)   { score += 0.5 * w; }
    }
    score
}

/// Public entrypoint for the `semantic_search` tool.
/// Uses embedding-backed cosine similarity when `bakes/latest/embeddings.db` exists
/// (built by `bake` via fastembed + SQLite). Falls back to TF-IDF otherwise.
pub fn semantic_search(
    path: Option<String>,
    query: String,
    limit: Option<usize>,
    file: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let limit = limit.unwrap_or(10).min(50);
    let file_filter = file.as_deref().map(str::to_lowercase);
    let bake_dir = root.join("bakes").join("latest");

    // Try vector search first
    if let Ok(Some(matches)) = crate::engine::embed::vector_search(
        &bake_dir,
        &query,
        limit,
        file_filter.as_deref(),
    ) {
        let results: Vec<SemanticMatch> = matches
            .into_iter()
            .map(|m| SemanticMatch {
                name: m.name,
                file: m.file,
                start_line: m.start_line,
                score: m.score,
                parent_type: m.parent_type,
                kind: "function",
            })
            .collect();
        let payload = SemanticSearchPayload {
            tool: "semantic_search",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            query,
            results,
        };
        return Ok(serde_json::to_string_pretty(&payload)?);
    }

    // Fallback: TF-IDF
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first."))?;

    let query_tokens = tokenize(&query);
    if query_tokens.is_empty() {
        return Err(anyhow!("Query produced no tokens after tokenisation."));
    }

    let n = bake.functions.len() as f32;
    let mut doc_freq: std::collections::HashMap<String, f32> =
        std::collections::HashMap::new();
    for func in &bake.functions {
        for tok in tokenize(&func.name)
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
        {
            *doc_freq.entry(tok).or_insert(0.0) += 1.0;
        }
    }
    let idf = |tok: &str| -> f32 {
        let df = doc_freq.get(tok).copied().unwrap_or(0.0);
        ((n + 1.0) / (df + 1.0)).ln() + 1.0
    };

    let mut scored: Vec<(f32, &crate::lang::IndexedFunction)> = bake
        .functions
        .iter()
        .filter(|f| {
            file_filter
                .as_deref()
                .map_or(true, |ff| f.file.to_lowercase().contains(ff))
        })
        .filter_map(|f| {
            let s = score_fn(f, &query_tokens, &idf);
            if s > 0.0 { Some((s, f)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    let results: Vec<SemanticMatch> = scored
        .into_iter()
        .map(|(score, f)| SemanticMatch {
            name: f.name.clone(),
            file: f.file.clone(),
            start_line: f.start_line,
            score,
            parent_type: f.parent_type.clone(),
            kind: "function",
        })
        .collect();

    let payload = SemanticSearchPayload {
        tool: "semantic_search",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        query,
        results,
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
            summary: None,
            parent_type: f.parent_type.clone(),
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
