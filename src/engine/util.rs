use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::types::{BakeFile, BakeIndex};

pub(crate) struct Snapshot {
    pub(crate) languages: BTreeSet<String>,
    pub(crate) files_indexed: usize,
}

fn requested_root(path: Option<String>) -> Result<PathBuf> {
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

fn bake_index_path(root: &Path) -> PathBuf {
    root.join("bakes").join("latest").join("bake.db")
}

fn find_bake_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|ancestor| bake_index_path(ancestor).exists())
        .map(Path::to_path_buf)
}

fn find_project_root_hint(start: &Path) -> Option<PathBuf> {
    const MARKERS: &[&str] = &[".git", "Cargo.toml", "go.mod", "package.json", "pyproject.toml", "Gemfile"];

    start
        .ancestors()
        .skip(1)
        .find(|ancestor| MARKERS.iter().any(|marker| ancestor.join(marker).exists()))
        .map(Path::to_path_buf)
}

fn missing_bake_error(root: &Path) -> anyhow::Error {
    if let Some(project_root) = find_project_root_hint(root) {
        return anyhow!(
            "No bake index found under {}. Did you mean to pass the project root {}? Run `bake` there first to build bakes/latest/bake.db.",
            root.display(),
            project_root.display()
        );
    }

    anyhow!(
        "No bake index found under {}. Run `bake` first to build bakes/latest/bake.db.",
        root.display()
    )
}

pub(crate) fn resolve_project_root(path: Option<String>) -> Result<PathBuf> {
    let root = requested_root(path)?;
    Ok(find_bake_root(&root).unwrap_or(root))
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
    let bake_path = bake_index_path(root);
    if !bake_path.exists() {
        return Ok(None);
    }

    let bake = super::db::read_bake_from_db(&bake_path)
        .with_context(|| format!("Failed to read bake index from {}", bake_path.display()))?;

    // Auto-reindex if the running binary is newer than what generated the index.
    let version_stale = parse_semver(env!("CARGO_PKG_VERSION")) > parse_semver(&bake.version);

    // Auto-reindex if any source file is newer than bake.db (or has gone missing).
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
        let bakes_dir = root.join("bakes").join("latest");
        fs::create_dir_all(&bakes_dir)?;
        super::db::write_bake_to_db(&fresh, &bake_path)
            .with_context(|| format!("Failed to write refreshed bake index to {}", bake_path.display()))?;
        return Ok(Some(fresh));
    }

    Ok(Some(bake))
}

pub(crate) fn require_bake_index(root: &PathBuf) -> Result<BakeIndex> {
    load_bake_index(root)?.ok_or_else(|| missing_bake_error(root))
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
    //
    // Scale stack size with repo size. The default rayon stack (2 MB) overflows
    // on large repos where tree-sitter AST recursion depth exceeds the limit.
    // Formula: 8 MB base + 1 KB per file, capped at 128 MB.
    let stack_bytes = (8 * 1024 * 1024 + files.len() * 1024).min(128 * 1024 * 1024);
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(stack_bytes)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    let results: Vec<_> = pool.install(|| {
        files
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
            .collect()
    });

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
    let bake_path = bake_index_path(root);
    if !bake_path.exists() {
        return Ok(());
    }

    let mut bake = match super::db::read_bake_from_db(&bake_path) {
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

    super::db::write_bake_to_db(&bake, &bake_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::engine::types::BakeIndex;

    fn write_bake(root: &TempDir) {
        let bakes_dir = root.path().join("bakes/latest");
        fs::create_dir_all(&bakes_dir).unwrap();
        let bake = BakeIndex {
            version: env!("CARGO_PKG_VERSION").to_string(),
            project_root: root.path().to_path_buf(),
            languages: BTreeSet::new(),
            files: vec![],
            functions: vec![],
            endpoints: vec![],
            types: vec![],
            impls: vec![],
        };
        crate::engine::db::write_bake_to_db(&bake, &bakes_dir.join("bake.db")).unwrap();
    }

    #[test]
    fn resolve_project_root_walks_up_to_existing_bake() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("src/engine");
        fs::create_dir_all(&subdir).unwrap();
        write_bake(&dir);

        let resolved = resolve_project_root(Some(subdir.to_string_lossy().into_owned())).unwrap();
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn require_bake_index_suggests_project_root_for_subdir_paths() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("src/engine");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let err = require_bake_index(&subdir).err().unwrap().to_string();
        assert!(err.contains(&format!("No bake index found under {}", subdir.display())));
        assert!(err.contains(&format!("Did you mean to pass the project root {}", dir.path().display())));
    }
}
