use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::types::{BakeFile, BakeIndex};

pub(crate) struct Snapshot {
    pub(crate) languages: BTreeSet<String>,
    pub(crate) files_indexed: usize,
}

pub(crate) fn resolve_project_root(path: Option<String>) -> Result<PathBuf> {
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

pub(crate) fn project_snapshot(root: &PathBuf) -> Result<Snapshot> {
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

pub(crate) fn load_bake_index(root: &PathBuf) -> Result<Option<BakeIndex>> {
    let bake_path = root.join("bakes").join("latest").join("bake.json");
    if !bake_path.exists() {
        return Ok(None);
    }

    let data =
        fs::read_to_string(&bake_path).with_context(|| format!("Failed to read {}", bake_path.display()))?;
    let bake: BakeIndex = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse bake index from {}", bake_path.display()))?;

    // Auto-reindex if the running binary is newer than what generated the index.
    let version_stale = parse_semver(env!("CARGO_PKG_VERSION")) > parse_semver(&bake.version);

    // Auto-reindex if any source file is newer than bake.json (or has gone missing).
    let bake_mtime = fs::metadata(&bake_path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let content_stale = bake.files.iter().any(|f| {
        fs::metadata(root.join(&f.path))
            .and_then(|m| m.modified())
            .map(|mtime| mtime > bake_mtime)
            .unwrap_or(true) // missing file → reindex
    });

    if version_stale || content_stale {
        let fresh = build_bake_index(root)?;
        let json = serde_json::to_string_pretty(&fresh)?;
        fs::write(&bake_path, &json)
            .with_context(|| format!("Failed to write refreshed bake index to {}", bake_path.display()))?;
        return Ok(Some(fresh));
    }

    Ok(Some(bake))
}

/// Parse a "MAJOR.MINOR.PATCH" version string into a comparable tuple.
fn parse_semver(v: &str) -> (u32, u32, u32) {
    let mut parts = v.split('.').filter_map(|s| s.parse::<u32>().ok());
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// Detect stdlib root paths for installed toolchains.
/// Returns (language, path) for each stdlib found. Best-effort — missing toolchains are skipped.
pub(crate) fn detect_stdlib_paths() -> Vec<(String, PathBuf)> {
    let mut paths = Vec::new();

    // Zig: zig env --json → lib_dir/std
    if let Ok(out) = std::process::Command::new("zig").args(["env", "--json"]).output() {
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            if let Some(lib_dir) = json.get("lib_dir").and_then(|v| v.as_str()) {
                let p = PathBuf::from(lib_dir).join("std");
                if p.is_dir() {
                    paths.push(("zig".to_string(), p));
                }
            }
        }
    }

    // Go: go env GOROOT → GOROOT/src
    if let Ok(out) = std::process::Command::new("go").args(["env", "GOROOT"]).output() {
        if let Ok(s) = std::str::from_utf8(&out.stdout) {
            let p = PathBuf::from(s.trim()).join("src");
            if p.is_dir() {
                paths.push(("go".to_string(), p));
            }
        }
    }

    // Rust: rustc --print sysroot → .../lib/rustlib/src/rust/library
    if let Ok(out) = std::process::Command::new("rustc").args(["--print", "sysroot"]).output() {
        if let Ok(s) = std::str::from_utf8(&out.stdout) {
            let p = PathBuf::from(s.trim())
                .join("lib").join("rustlib").join("src").join("rust").join("library");
            if p.is_dir() {
                paths.push(("rust".to_string(), p));
            }
        }
    }

    // TypeScript: try npm, pnpm, yarn in order — first valid dir wins
    let ts_root = [
        ("npm",  vec!["root", "-g"]),
        ("pnpm", vec!["root", "-g"]),
        ("yarn", vec!["global", "dir"]),
    ]
    .iter()
    .find_map(|(cmd, args)| {
        std::process::Command::new(cmd).args(args.as_slice()).output().ok()
            .and_then(|out| std::str::from_utf8(&out.stdout).ok().map(|s| s.trim().to_string()))
            .map(|s| PathBuf::from(s).join("typescript").join("lib"))
            .filter(|p| p.is_dir())
    });
    if let Some(p) = ts_root {
        paths.push(("typescript".to_string(), p));
    }

    paths
}

pub(crate) fn build_bake_index(root: &PathBuf) -> Result<BakeIndex> {
    use ignore::WalkBuilder;
    use rayon::prelude::*;

    // Phase 1: collect file metadata using .gitignore-aware walker.
    let mut languages = BTreeSet::new();
    let mut files: Vec<BakeFile> = Vec::new();

    for result in WalkBuilder::new(root)
        .hidden(false)       // don't skip hidden files — .gitignore handles exclusions
        .git_ignore(true)    // respect .gitignore (nested, global, .git/info/exclude)
        .require_git(false)  // apply .gitignore rules even outside a git repo
        .filter_entry(|e| e.file_name() != ".git")  // never descend into .git/
        .build()
    {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path().to_path_buf();
        if !path.is_file() {
            continue;
        }
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let lang = detect_language(&path);
        if lang != "other" {
            languages.insert(lang.to_string());
        }
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        files.push(BakeFile {
            path: rel,
            language: lang.to_string(),
            bytes,
            imports: vec![],
            origin: "user".to_string(),
        });
    }

    // Phase 2: parse files in parallel (CPU-bound tree-sitter work).
    // Stdlib is NOT pre-indexed here — it is too large (Go stdlib = 3000+ files).
    // The router pulls specific stdlib signatures on demand via detect_stdlib_paths() + symbol().
    let results: Vec<_> = files
        .par_iter()
        .enumerate()
        .filter_map(|(idx, file)| {
            let lang = file.language.as_str();
            let analyzer = crate::lang::find_analyzer(lang)?;
            let full_path = root.join(&file.path);
            let source = std::fs::read_to_string(&full_path).ok()?;
            let imports = analyzer.extract_imports(&source);
            let (funcs, eps, typs, imps) = analyzer.analyze_file(root, &full_path).ok()?;
            Some((idx, funcs, eps, typs, imps, imports))
        })
        .collect();

    let mut functions = Vec::new();
    let mut endpoints = Vec::new();
    let mut types = Vec::new();
    let mut impls = Vec::new();
    for (idx, funcs, eps, typs, imps, imports) in results {
        functions.extend(funcs);
        endpoints.extend(eps);
        types.extend(typs);
        impls.extend(imps);
        files[idx].imports = imports;
    }

    Ok(BakeIndex {
        version: env!("CARGO_PKG_VERSION").to_string(),
        project_root: root.clone(),
        languages,
        files,
        functions,
        endpoints,
        types,
        impls,
    })
}

pub(crate) fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("kt") | Some("kts") => "kotlin",
        Some("php") => "php",
        Some("rb") => "ruby",
        Some("swift") => "swift",
        Some("c") | Some("h") => "c",
        Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") | Some("hh") => "cpp",
        Some("cs") => "csharp",
        Some("sh") | Some("bash") => "bash",
        Some("zig") => "zig",
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

/// Incrementally re-index a set of changed files in the bake index.
/// If no bake index exists this is a no-op. Errors are silently swallowed so
/// callers can use `let _ = reindex_files(...)`.
pub(crate) fn reindex_files(root: &PathBuf, changed_files: &[&str]) -> Result<()> {
    let bake_path = root.join("bakes").join("latest").join("bake.json");
    if !bake_path.exists() {
        return Ok(());
    }

    let data = match fs::read_to_string(&bake_path) {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };
    let mut bake: super::types::BakeIndex = match serde_json::from_str(&data) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };

    for file in changed_files {
        // Remove stale entries for this file.
        bake.functions.retain(|f| f.file.as_str() != *file);
        bake.endpoints.retain(|e| e.file.as_str() != *file);
        bake.types.retain(|t| t.file.as_str() != *file);

        // Re-analyze the file.
        let full_path = root.join(file);
        if !full_path.exists() {
            continue;
        }
        let lang = detect_language(&full_path);
        if let Some(analyzer) = crate::lang::find_analyzer(lang) {
            if let Ok((funcs, eps, typs, imps)) = analyzer.analyze_file(root, &full_path) {
                bake.functions.extend(funcs);
                bake.endpoints.extend(eps);
                bake.types.extend(typs);
                bake.impls.extend(imps);
            }
        }
    }

    let json = serde_json::to_string_pretty(&bake)?;
    fs::write(&bake_path, json)?;
    Ok(())
}

