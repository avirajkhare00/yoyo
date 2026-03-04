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

pub(crate) fn build_bake_index(root: &PathBuf) -> Result<BakeIndex> {
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
                    match analyzer.analyze_file(root, &path) {
                        Ok((funcs, eps)) => {
                            functions.extend(funcs);
                            endpoints.extend(eps);
                        }
                        Err(_) => {} // skip files that fail to parse
                    }
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


pub(crate) fn detect_language(path: &Path) -> &'static str {
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

pub(crate) fn module_from_path(path: &str) -> String {
    // Heuristic: use directory portion of the path as the "module".
    if let Some((dir, _file)) = path.rsplit_once('/') {
        dir.to_string()
    } else {
        ".".to_string()
    }
}

pub(crate) fn infer_entity_from_path(path: &str) -> String {
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
