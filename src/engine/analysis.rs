use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;

use super::types::{
    DeadFunction, DocMatch, DuplicateEntry, DuplicateGroup, FeatureEnvy, FindDocsPayload,
    GraphDeletePayload, HealthPayload, InsiderTrading, LargeFunction, LongMethod, ShotgunSurgery,
};
use super::util::{load_bake_index, reindex_files, resolve_project_root};


/// Public entrypoint for the `blast_radius` tool.
pub fn blast_radius(path: Option<String>, symbol: String, depth: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow::anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let max_depth = depth.unwrap_or(2);

    // Build complexity lookup and reverse call index: callee_name → vec of (caller_name, caller_file)
    let complexity_map: std::collections::HashMap<String, u32> = bake
        .functions
        .iter()
        .map(|f| (f.name.clone(), f.complexity))
        .collect();

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

    // BFS pass 1: depth-limited — builds the callers list for display.
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
                    let complexity = complexity_map.get(caller_name).copied().unwrap_or(0);
                    callers.push(serde_json::json!({
                        "caller": caller_name,
                        "file": caller_file,
                        "depth": d + 1,
                        "complexity": complexity,
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

    // BFS pass 2: unlimited — compute the true transitive caller count.
    let total_callers = {
        let mut all_visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut all_seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        let mut q: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        q.push_back(symbol.clone());
        all_visited.insert(symbol.clone());
        while let Some(sym) = q.pop_front() {
            if let Some(entries) = called_by.get(&sym) {
                for (caller_name, caller_file) in entries {
                    all_seen.insert((caller_name.clone(), caller_file.clone()));
                    if !all_visited.contains(caller_name) {
                        all_visited.insert(caller_name.clone());
                        q.push_back(caller_name.clone());
                    }
                }
            }
        }
        all_seen.len()
    };

    // Sort: depth ascending (closest callers first), then complexity descending (highest impact first)
    callers.sort_by(|a, b| {
        let da = a["depth"].as_u64().unwrap_or(0);
        let db = b["depth"].as_u64().unwrap_or(0);
        da.cmp(&db).then_with(|| {
            let ca = b["complexity"].as_u64().unwrap_or(0);
            let cb = a["complexity"].as_u64().unwrap_or(0);
            ca.cmp(&cb)
        })
    });

    // Import-graph expansion: add files that import the target symbol's defining file
    // or any already-affected file.  Catches file-level deps the call graph misses.
    {
        let target_file = bake.functions.iter()
            .find(|f| f.name == symbol)
            .map(|f| f.file.clone());

        let mut seeds: Vec<String> = affected_files.iter().cloned().collect();
        if let Some(tf) = target_file {
            seeds.push(tf);
        }

        for seed in &seeds {
            let seed_stem = std::path::Path::new(seed)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if seed_stem.is_empty() { continue; }

            for bake_file in &bake.files {
                let path_str = bake_file.path.to_string_lossy().to_string();
                if affected_files.contains(&path_str) { continue; }
                if bake_file.imports.iter().any(|imp| imp.contains(&seed_stem)) {
                    affected_files.insert(path_str);
                }
            }
        }
    }

    let affected_files: Vec<String> = affected_files.into_iter().collect();

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

    // ── shared indexes ────────────────────────────────────────────────────────

    // callee_name (lowercased) → set of caller files
    let mut callee_to_caller_files: HashMap<String, HashSet<String>> = HashMap::new();
    // name → file (for feature envy cross-file resolution)
    let name_to_file: HashMap<String, &str> = bake
        .functions
        .iter()
        .map(|f| (f.name.to_lowercase(), f.file.as_str()))
        .collect();

    for f in &bake.functions {
        for c in &f.calls {
            callee_to_caller_files
                .entry(c.callee.to_lowercase())
                .or_default()
                .insert(f.file.clone());
        }
    }

    let all_callees: HashSet<String> = callee_to_caller_files.keys().cloned().collect();

    // ── 1. Dead Code (Fowler: Dead Code) ─────────────────────────────────────
    // Threshold: never called, non-public, non-test, non-handler, name > 2 chars.
    // Public functions are externally reachable; HTTP handlers use dynamic dispatch.
    let mut dead_code: Vec<DeadFunction> = bake
        .functions
        .iter()
        .filter(|f| {
            let lc = f.name.to_lowercase();
            let file_lc = f.file.to_lowercase();
            !all_callees.contains(&lc)
                && lc != "main"
                && !lc.starts_with("test")
                && !lc.ends_with("_test")
                && !f.file.contains("test")
                && !file_lc.contains("example")
                && !file_lc.contains("/bench")
                && !lc.starts_with("handle_")
                && !file_lc.contains("handler")
                && f.visibility != crate::lang::Visibility::Public
                && f.name.len() > 2
        })
        .map(|f| DeadFunction {
            name: f.name.clone(),
            file: f.file.clone(),
            start_line: f.start_line,
            end_line: f.end_line,
            lines: f.end_line.saturating_sub(f.start_line) + 1,
            smell: "Dead Code",
            refactoring: "Delete Dead Code",
        })
        .collect();
    dead_code.sort_by(|a, b| b.lines.cmp(&a.lines));

    // ── 2. Large Function (Fowler: Long Method — complexity × fan-out) ────────
    // Threshold: complexity > 10 (McCabe high-risk) AND fan_out > 5.
    // Score = complexity × fan_out; ranked descending, capped at top_n.
    let mut large_functions: Vec<LargeFunction> = bake
        .functions
        .iter()
        .map(|f| {
            let fan_out = f.calls.iter().map(|c| c.callee.as_str()).collect::<HashSet<_>>().len();
            let score = f.complexity.saturating_mul(fan_out as u32);
            (f, fan_out, score)
        })
        .filter(|(f, fan_out, _)| f.complexity > 10 && *fan_out > 5)
        .map(|(f, fan_out, score)| LargeFunction {
            name: f.name.clone(),
            file: f.file.clone(),
            start_line: f.start_line,
            complexity: f.complexity,
            fan_out,
            score,
            smell: "Large Function",
            refactoring: "Extract Function",
            why: format!(
                "complexity={}, fan_out={}; exceeds thresholds (complexity>10, fan_out>5)",
                f.complexity, fan_out
            ),
        })
        .collect();
    large_functions.sort_by(|a, b| b.score.cmp(&a.score));
    large_functions.truncate(top_n);

    // ── 3. Long Method (Fowler: Long Method — lines) ─────────────────────────
    // Threshold: > 30 lines. Fowler's rule: if you can't read it on one screen, extract it.
    // Skip functions already in large_functions to avoid double-reporting.
    let large_names: HashSet<(&str, u32)> = large_functions
        .iter()
        .map(|g| (g.file.as_str(), g.start_line))
        .collect();
    let mut long_methods: Vec<LongMethod> = bake
        .functions
        .iter()
        .filter(|f| {
            let lines = f.end_line.saturating_sub(f.start_line) + 1;
            lines > 30 && !large_names.contains(&(f.file.as_str(), f.start_line))
        })
        .map(|f| {
            let lines = f.end_line.saturating_sub(f.start_line) + 1;
            LongMethod {
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                end_line: f.end_line,
                lines,
                smell: "Long Method",
                refactoring: "Extract Function",
                why: format!("{} lines; threshold: >30 (Fowler: fits on one screen)", lines),
            }
        })
        .collect();
    long_methods.sort_by(|a, b| b.lines.cmp(&a.lines));
    long_methods.truncate(top_n);

    // ── 4. Feature Envy (Fowler: Feature Envy) ────────────────────────────────
    // Threshold: cross-file calls > same-file calls AND cross-file calls >= 3.
    // "A function that seems more interested in a class other than the one it's in."
    let mut feature_envy: Vec<FeatureEnvy> = bake
        .functions
        .iter()
        .filter_map(|f| {
            let mut cross_file_by_target: HashMap<&str, usize> = HashMap::new();
            let mut same_file = 0usize;
            for c in &f.calls {
                match name_to_file.get(&c.callee.to_lowercase()) {
                    Some(&target_file) if target_file == f.file => same_file += 1,
                    Some(&target_file) => {
                        *cross_file_by_target.entry(target_file).or_default() += 1;
                    }
                    None => {}
                }
            }
            let cross_file: usize = cross_file_by_target.values().sum();
            if cross_file <= same_file || cross_file < 3 {
                return None;
            }
            let envies = cross_file_by_target
                .into_iter()
                .max_by_key(|(_, n)| *n)
                .map(|(file, _)| file.to_string())
                .unwrap_or_default();
            Some(FeatureEnvy {
                why: format!(
                    "{} cross-file calls vs {} same-file calls; most calls go to {}",
                    cross_file, same_file, envies
                ),
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                envies,
                cross_file_calls: cross_file,
                same_file_calls: same_file,
                smell: "Feature Envy",
                refactoring: "Move Method",
            })
        })
        .collect();
    feature_envy.sort_by(|a, b| b.cross_file_calls.cmp(&a.cross_file_calls));
    feature_envy.truncate(top_n);

    // ── 5. Shotgun Surgery (Fowler: Shotgun Surgery) ──────────────────────────
    // Threshold: called from >= 4 different files.
    // "Every time you make a change you have to make a lot of little changes in many classes."
    let mut shotgun_surgery: Vec<ShotgunSurgery> = bake
        .functions
        .iter()
        .filter_map(|f| {
            let caller_files = callee_to_caller_files
                .get(&f.name.to_lowercase())
                .map(|s| s.len())
                .unwrap_or(0);
            if caller_files < 4 {
                return None;
            }
            Some(ShotgunSurgery {
                why: format!(
                    "called from {} different files; threshold: >=4",
                    caller_files
                ),
                name: f.name.clone(),
                file: f.file.clone(),
                start_line: f.start_line,
                caller_files,
                smell: "Shotgun Surgery",
                refactoring: "Move Method / Extract Class",
            })
        })
        .collect();
    shotgun_surgery.sort_by(|a, b| b.caller_files.cmp(&a.caller_files));
    shotgun_surgery.truncate(top_n);

    // ── 6. Insider Trading (Fowler: Insider Trading) ──────────────────────────
    // Threshold: file A calls into file B AND file B calls into file A, each >= 2 times.
    // "Classes that trade data between themselves too much."
    // Build file → set of files it calls into.
    let mut file_to_called_files: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for f in &bake.functions {
        for c in &f.calls {
            if let Some(&target_file) = name_to_file.get(&c.callee.to_lowercase()) {
                if target_file != f.file.as_str() {
                    *file_to_called_files
                        .entry(f.file.as_str())
                        .or_default()
                        .entry(target_file)
                        .or_default() += 1;
                }
            }
        }
    }
    let mut insider_trading: Vec<InsiderTrading> = Vec::new();
    let files: Vec<&str> = file_to_called_files.keys().copied().collect();
    let mut seen_pairs: HashSet<(&str, &str)> = HashSet::new();
    for &file_a in &files {
        if let Some(a_calls) = file_to_called_files.get(file_a) {
            for (&file_b, &a_calls_b) in a_calls {
                let key = if file_a < file_b { (file_a, file_b) } else { (file_b, file_a) };
                if seen_pairs.contains(&key) {
                    continue;
                }
                let b_calls_a = file_to_called_files
                    .get(file_b)
                    .and_then(|m| m.get(file_a))
                    .copied()
                    .unwrap_or(0);
                if a_calls_b >= 2 && b_calls_a >= 2 {
                    seen_pairs.insert(key);
                    insider_trading.push(InsiderTrading {
                        why: format!(
                            "{} calls {} functions in {}; {} calls {} functions back",
                            file_a, a_calls_b, file_b, file_b, b_calls_a
                        ),
                        file_a: file_a.to_string(),
                        file_b: file_b.to_string(),
                        a_calls_b,
                        b_calls_a,
                        smell: "Insider Trading",
                        refactoring: "Hide Delegate / Move Method",
                    });
                }
            }
        }
    }
    insider_trading.sort_by(|a, b| (b.a_calls_b + b.b_calls_a).cmp(&(a.a_calls_b + a.b_calls_a)));
    insider_trading.truncate(top_n);

    // ── 7. Duplicate Code (Fowler: Duplicate Code) ────────────────────────────
    // Group by name stem (strip common verb prefixes). Flag stems appearing in >= 2 files.
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
    let mut duplicate_code: Vec<DuplicateGroup> = by_stem
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
            smell: "Duplicate Code",
            refactoring: "Extract Function",
        })
        .collect();
    duplicate_code.sort_by(|a, b| a.stem.cmp(&b.stem));
    duplicate_code.truncate(top_n);

    let payload = HealthPayload {
        tool: "health",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        dead_code,
        large_functions,
        long_methods,
        feature_envy,
        shotgun_surgery,
        insider_trading,
        duplicate_code,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── graph_delete ──────────────────────────────────────────────────────────────

/// Collapse runs of 3+ consecutive newlines to 2, and trim trailing blank lines.
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0u32;
    for line in s.split('\n') {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    // Ensure exactly one trailing newline.
    let trimmed = out.trim_end_matches('\n');
    let mut result = trimmed.to_string();
    result.push('\n');
    result
}

/// Remove a function from a file by name. Requires a prior bake.
/// Pre-flight: refuses to delete if active callers exist, unless `force` is true.
pub fn graph_delete(path: Option<String>, name: String, file: Option<String>, force: bool) -> Result<String> {
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

    // Pre-flight: find any callers of this symbol in the index.
    let callers: Vec<String> = bake
        .functions
        .iter()
        .filter(|f| f.name.to_lowercase() != name_lc)
        .filter(|f| f.calls.iter().any(|c| c.callee.to_lowercase() == name_lc))
        .map(|f| format!("{} ({})", f.name, f.file))
        .collect();

    if !callers.is_empty() && !force {
        return Err(anyhow::anyhow!(
            "Symbol {:?} has {} active caller(s): {}. \
             Run blast_radius to investigate, or pass force=true to delete anyway.",
            name, callers.len(), callers.join(", ")
        ));
    }

    let warnings: Vec<String> = if !callers.is_empty() {
        vec![format!("Deleted with {} active caller(s): {}", callers.len(), callers.join(", "))]
    } else {
        vec![]
    };

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

    // Collapse orphan blank lines left at the deletion site: any run of 3+
    // consecutive newlines → 2 (one blank line between neighbours).
    // Also trim trailing whitespace-only lines at EOF down to a single newline.
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let cleaned = collapse_blank_lines(&content);
    let bytes = cleaned.into_bytes();

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
        warnings,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(root: &TempDir, rel: &str, content: &str) {
        let p = root.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    fn bake_dir(root: &TempDir) {
        crate::engine::bake(Some(root.path().to_string_lossy().into_owned())).unwrap();
    }

    // ── graph_delete ──────────────────────────────────────────────────────────

    #[test]
    fn delete_removes_function_body_from_file() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/lib.rs",
            "fn keep_me() {\n    let x = 1;\n}\n\nfn remove_me() {\n    let y = 2;\n}\n",
        );
        bake_dir(&dir);

        let result = graph_delete(
            Some(dir.path().to_string_lossy().into_owned()),
            "remove_me".into(),
            None,
            false,
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "graph_delete");
        assert!(v["bytes_removed"].as_u64().unwrap() > 0);

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(!content.contains("remove_me"));
        assert!(content.contains("fn keep_me"));
    }

    #[test]
    fn delete_returns_error_when_symbol_not_in_index() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn foo() {}\n");
        bake_dir(&dir);

        let err = graph_delete(
            Some(dir.path().to_string_lossy().into_owned()),
            "no_such_fn".into(),
            None,
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
