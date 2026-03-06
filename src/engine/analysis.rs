use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;

use super::types::{
    DeadFunction, DocMatch, DuplicateEntry, DuplicateGroup, FindDocsPayload, GodFunction,
    GraphDeletePayload, HealthPayload,
};
use super::util::{load_bake_index, reindex_files, resolve_project_root};


/// Public entrypoint for the `blast_radius` tool.
pub fn blast_radius(path: Option<String>, symbol: String, depth: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow::anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let max_depth = depth.unwrap_or(2);

    // Build reverse call index: callee_name → vec of (caller_name, caller_file)
    let mut called_by: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for f in &bake.functions {
        for callee in &f.calls {
            called_by
                .entry(callee.callee.clone())
                .or_default()
                .push((f.name.clone(), f.file.clone()));
        }
    }

    // BFS from target symbol outward through callers
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_callers: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut affected_files: BTreeSet<String> = BTreeSet::new();
    let mut callers: Vec<serde_json::Value> = Vec::new();
    let mut queue: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::new();

    queue.push_back((symbol.clone(), 0));
    visited.insert(symbol.clone());

    while let Some((sym, d)) = queue.pop_front() {
        if d >= max_depth {
            continue;
        }
        if let Some(entries) = called_by.get(&sym) {
            for (caller_name, caller_file) in entries {
                let key = (caller_name.clone(), caller_file.clone());
                if seen_callers.insert(key) {
                    callers.push(serde_json::json!({
                        "caller": caller_name,
                        "file": caller_file,
                        "depth": d + 1,
                    }));
                    affected_files.insert(caller_file.clone());
                }
                if !visited.contains(caller_name) {
                    visited.insert(caller_name.clone());
                    queue.push_back((caller_name.clone(), d + 1));
                }
            }
        }
    }

    let affected_files: Vec<String> = affected_files.into_iter().collect();
    let total_callers = callers.len();

    let payload = serde_json::json!({
        "tool": "blast_radius",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "symbol": symbol,
        "depth": max_depth,
        "callers": callers,
        "affected_files": affected_files,
        "total_callers": total_callers,
    });

    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Public entrypoint for the `find_docs` tool.
pub fn find_docs(path: Option<String>, doc_type: String, limit: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let limit = limit.unwrap_or(50);

    let mut matches = Vec::new();

    fn walk_docs(dir: &Path, root: &Path, doc_type: &str, limit: usize, out: &mut Vec<DocMatch>) -> Result<()> {
        if out.len() >= limit {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            if out.len() >= limit {
                break;
            }
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if matches!(name, ".git" | "node_modules" | "dist" | "build" | "target") {
                        continue;
                    }
                }
                walk_docs(&path, root, doc_type, limit, out)?;
            } else if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();

                let is_match = match doc_type {
                    "readme" => name.to_lowercase().starts_with("readme"),
                    "env" => name.starts_with(".env") || name.to_lowercase() == "env",
                    "config" => {
                        let lc = name.to_lowercase();
                        lc.contains("config") || lc.ends_with(".toml") || lc.ends_with(".yaml") || lc.ends_with(".yml")
                    }
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

    walk_docs(&root, &root, &doc_type, limit, &mut matches)?;
    let truncated = matches.len() >= limit;

    let payload = FindDocsPayload {
        tool: "find_docs",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        doc_type,
        truncated,
        matches,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}


// ── health ────────────────────────────────────────────────────────────────────

/// Diagnose a codebase: dead code, god functions, duplicate hints.
pub fn health(path: Option<String>, top: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow::anyhow!("No bake index found. Run `bake` first."))?;

    let top_n = top.unwrap_or(10);

    // Set of every callee name ever called (lowercased).
    let all_callees: HashSet<String> = bake
        .functions
        .iter()
        .flat_map(|f| f.calls.iter().map(|c| c.callee.to_lowercase()))
        .collect();

    // Dead code: indexed but never called; skip main, tests, very short names.
    let mut dead_code: Vec<DeadFunction> = bake
        .functions
        .iter()
        .filter(|f| {
            let lc = f.name.to_lowercase();
            !all_callees.contains(&lc)
                && lc != "main"
                && !lc.starts_with("test")
                && !lc.ends_with("_test")
                && !f.file.contains("test")
                && f.name.len() > 2
        })
        .map(|f| DeadFunction {
            name: f.name.clone(),
            file: f.file.clone(),
            start_line: f.start_line,
            end_line: f.end_line,
            lines: f.end_line.saturating_sub(f.start_line) + 1,
        })
        .collect();
    dead_code.sort_by(|a, b| b.lines.cmp(&a.lines));

    // God functions: ranked by complexity × unique fan-out.
    let mut god_functions: Vec<GodFunction> = bake
        .functions
        .iter()
        .map(|f| {
            let fan_out = f.calls.iter().map(|c| c.callee.as_str()).collect::<HashSet<_>>().len();
            let score = f.complexity.saturating_mul(fan_out as u32);
            GodFunction {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                complexity: f.complexity,
                fan_out,
                score,
            }
        })
        .filter(|g| g.score > 0)
        .collect();
    god_functions.sort_by(|a, b| b.score.cmp(&a.score));
    god_functions.truncate(top_n);

    // Duplicate hints: group by stem (name with common verb prefix stripped).
    const PREFIXES: &[&str] = &[
        "get_", "set_", "create_", "update_", "delete_", "handle_", "run_",
        "fetch_", "load_", "save_", "parse_", "build_", "make_", "init_",
        "process_", "validate_", "check_",
    ];
    let stem = |name: &str| -> String {
        let lc = name.to_lowercase();
        for p in PREFIXES {
            if lc.starts_with(p) {
                return lc[p.len()..].to_string();
            }
        }
        lc
    };

    let mut by_stem: HashMap<String, Vec<&crate::lang::IndexedFunction>> = HashMap::new();
    for f in &bake.functions {
        let s = stem(&f.name);
        if s.len() > 2 {
            by_stem.entry(s).or_default().push(f);
        }
    }

    let mut duplicate_hints: Vec<DuplicateGroup> = by_stem
        .into_iter()
        .filter(|(_, funcs)| {
            funcs.len() >= 2
                && funcs.iter().map(|f| f.file.as_str()).collect::<HashSet<_>>().len() >= 2
        })
        .map(|(s, funcs)| DuplicateGroup {
            stem: s,
            functions: funcs
                .iter()
                .map(|f| DuplicateEntry {
                    name: f.name.clone(),
                    file: f.file.clone(),
                    start_line: f.start_line,
                })
                .collect(),
        })
        .collect();
    duplicate_hints.sort_by(|a, b| a.stem.cmp(&b.stem));
    duplicate_hints.truncate(top_n);

    let payload = HealthPayload {
        tool: "health",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        dead_code,
        god_functions,
        duplicate_hints,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── graph_delete ──────────────────────────────────────────────────────────────

/// Remove a function from a file by name. Requires a prior bake.
pub fn graph_delete(path: Option<String>, name: String, file: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow::anyhow!("No bake index. Run `bake` first."))?;

    let name_lc = name.to_lowercase();
    let file_lc = file.as_deref().map(|s| s.to_lowercase());

    let func = bake
        .functions
        .iter()
        .find(|f| {
            f.name.to_lowercase() == name_lc
                && file_lc
                    .as_deref()
                    .map(|ff| f.file.to_lowercase().ends_with(ff))
                    .unwrap_or(true)
        })
        .ok_or_else(|| anyhow::anyhow!("Symbol {:?} not found in bake index.", name))?;

    let rel_file = func.file.clone();
    let byte_start = func.byte_start;
    let byte_end = func.byte_end;

    let full_path = root.join(&rel_file);
    let mut bytes = std::fs::read(&full_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", rel_file, e))?;

    if byte_end > bytes.len() || byte_start > byte_end {
        return Err(anyhow::anyhow!(
            "Invalid byte range [{}, {}) for {} (file len {})",
            byte_start, byte_end, rel_file, bytes.len()
        ));
    }

    let bytes_removed = byte_end - byte_start;
    bytes.drain(byte_start..byte_end);

    std::fs::write(&full_path, &bytes)
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", rel_file, e))?;

    let _ = reindex_files(&root, &[rel_file.as_str()]);

    let payload = GraphDeletePayload {
        tool: "graph_delete",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        name,
        file: rel_file,
        bytes_removed,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}
