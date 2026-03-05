use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::Result;

use super::types::{DocMatch, FindDocsPayload};
use super::util::{load_bake_index, resolve_project_root};

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
                callers.push(serde_json::json!({
                    "caller": caller_name,
                    "file": caller_file,
                    "depth": d + 1,
                }));
                affected_files.insert(caller_file.clone());
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
