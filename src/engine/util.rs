use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use super::types::{BakeFile, BakeIndex, ScopeDependency, ScopeSummary};

pub(crate) struct Snapshot {
    pub(crate) languages: BTreeSet<String>,
    pub(crate) files_indexed: usize,
    pub(crate) scopes: Vec<ScopeSummary>,
    pub(crate) scope_dependencies: Vec<ScopeDependency>,
    pub(crate) scoping_hints: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScopeProfile {
    pub(crate) name: String,
    pub(crate) tags: BTreeSet<String>,
}

const BAKE_DIRNAME: &str = ".bakes";
const LEGACY_BAKE_DIRNAME: &str = "bakes";

fn requested_root(path: Option<String>) -> Result<PathBuf> {
    if let Some(p) = path {
        let pb = PathBuf::from(p);
        let meta =
            fs::metadata(&pb).with_context(|| format!("Failed to stat path: {}", pb.display()))?;
        if !meta.is_dir() {
            anyhow::bail!("Provided path is not a directory: {}", pb.display());
        }
        return Ok(pb);
    }

    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    Ok(cwd)
}

pub(crate) fn bake_artifacts_dir(root: &Path) -> PathBuf {
    root.join(BAKE_DIRNAME).join("latest")
}

fn legacy_bake_artifacts_dir(root: &Path) -> PathBuf {
    root.join(LEGACY_BAKE_DIRNAME).join("latest")
}

fn bake_index_path(root: &Path) -> PathBuf {
    bake_artifacts_dir(root).join("bake.db")
}

fn legacy_bake_index_path(root: &Path) -> PathBuf {
    legacy_bake_artifacts_dir(root).join("bake.db")
}

fn existing_bake_index_path(root: &Path) -> Option<PathBuf> {
    let bake_path = bake_index_path(root);
    if bake_path.exists() {
        return Some(bake_path);
    }

    let legacy_path = legacy_bake_index_path(root);
    legacy_path.exists().then_some(legacy_path)
}

fn is_bake_artifact_dir_name(name: &str) -> bool {
    matches!(name, BAKE_DIRNAME | LEGACY_BAKE_DIRNAME)
}

fn git_dir(root: &Path) -> Result<Option<PathBuf>> {
    let git_path = root.join(".git");
    let Ok(meta) = fs::metadata(&git_path) else {
        return Ok(None);
    };

    if meta.is_dir() {
        return Ok(Some(git_path));
    }

    if meta.is_file() {
        let content = fs::read_to_string(&git_path)
            .with_context(|| format!("Failed to read {}", git_path.display()))?;
        let Some(raw) = content.strip_prefix("gitdir:") else {
            return Ok(None);
        };
        let git_dir = PathBuf::from(raw.trim());
        let resolved = if git_dir.is_absolute() {
            git_dir
        } else {
            root.join(git_dir)
        };
        return Ok(Some(resolved));
    }

    Ok(None)
}

pub(crate) fn ensure_bake_gitignored(root: &Path) -> Result<()> {
    let Some(git_dir) = git_dir(root)? else {
        return Ok(());
    };

    let info_dir = git_dir.join("info");
    fs::create_dir_all(&info_dir)
        .with_context(|| format!("Failed to create {}", info_dir.display()))?;
    let exclude_path = info_dir.join("exclude");
    let mut exclude = if exclude_path.exists() {
        fs::read_to_string(&exclude_path)
            .with_context(|| format!("Failed to read {}", exclude_path.display()))?
    } else {
        String::new()
    };

    if exclude.lines().any(|line| line.trim() == ".bakes/") {
        return Ok(());
    }

    if !exclude.is_empty() && !exclude.ends_with('\n') {
        exclude.push('\n');
    }
    exclude.push_str(".bakes/\n");
    fs::write(&exclude_path, exclude)
        .with_context(|| format!("Failed to write {}", exclude_path.display()))?;
    Ok(())
}

pub(crate) fn prepare_bake_artifacts_dir(root: &Path) -> Result<PathBuf> {
    let legacy_root = root.join(LEGACY_BAKE_DIRNAME);
    let current_root = root.join(BAKE_DIRNAME);
    if legacy_root.exists() && !current_root.exists() {
        fs::rename(&legacy_root, &current_root).with_context(|| {
            format!(
                "Failed to move legacy bake dir from {} to {}",
                legacy_root.display(),
                current_root.display()
            )
        })?;
    }

    ensure_bake_gitignored(root)?;

    let bake_dir = bake_artifacts_dir(root);
    fs::create_dir_all(&bake_dir)
        .with_context(|| format!("Failed to create {}", bake_dir.display()))?;
    Ok(bake_dir)
}

fn find_bake_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|ancestor| existing_bake_index_path(ancestor).is_some())
        .map(Path::to_path_buf)
}

fn find_project_root_hint(start: &Path) -> Option<PathBuf> {
    const MARKERS: &[&str] = &[
        ".git",
        "Cargo.toml",
        "go.mod",
        "package.json",
        "pyproject.toml",
        "Gemfile",
    ];

    start
        .ancestors()
        .skip(1)
        .find(|ancestor| MARKERS.iter().any(|marker| ancestor.join(marker).exists()))
        .map(Path::to_path_buf)
}

fn missing_bake_error(root: &Path) -> anyhow::Error {
    if let Some(project_root) = find_project_root_hint(root) {
        return anyhow!(
            "No bake index found under {}. Did you mean to pass the project root {}? Run `bake` there first to build .bakes/latest/bake.db.",
            root.display(),
            project_root.display()
        );
    }

    anyhow!(
        "No bake index found under {}. Run `bake` first to build .bakes/latest/bake.db.",
        root.display()
    )
}

pub(crate) fn resolve_project_root(path: Option<String>) -> Result<PathBuf> {
    let root = requested_root(path)?;
    Ok(find_bake_root(&root).unwrap_or(root))
}

pub(crate) fn scope_profile(path: &str, language: &str) -> ScopeProfile {
    let normalized = path.replace('\\', "/").to_lowercase();
    let mut parts = normalized.split('/').filter(|part| !part.is_empty());
    let first = parts.next().unwrap_or(".");
    let name = if matches!(first, "src" | "lib" | "app" | "internal" | "pkg" | "cmd")
        && normalized.contains('/')
    {
        first.to_string()
    } else {
        first.to_string()
    };

    let mut tags = BTreeSet::new();
    let lower_lang = language.to_lowercase();
    let is_js_family = matches!(lower_lang.as_str(), "typescript" | "javascript");
    let has_any = |needles: &[&str]| needles.iter().any(|needle| normalized.contains(needle));

    if has_any(&[
        "/test",
        "/tests",
        "/spec",
        "__tests__",
        ".test.",
        ".spec.",
        "cypress",
        "playwright",
        "/e2e",
        "/integration",
    ]) {
        tags.insert("test".to_string());
    }
    if has_any(&["cypress", "playwright", "/e2e"]) {
        tags.insert("e2e".to_string());
    }
    if has_any(&[
        "generated",
        "openapi",
        "swagger",
        "/gen/",
        "/gen.",
        "/generated/",
    ]) {
        tags.insert("generated".to_string());
    }
    if has_any(&[
        "/backend",
        "/server",
        "/servers",
        "/handler",
        "/handlers",
        "/controller",
        "/controllers",
        "/service",
        "/services",
        "/api",
        "/internal",
        "/cmd",
        "/pkg",
    ]) || name == "backend"
    {
        tags.insert("backend".to_string());
    }
    if has_any(&[
        "/frontend",
        "/web",
        "/client",
        "/ui",
        "/components",
        "/hooks",
        "/pages",
        "/tui",
    ]) || name == "web"
        || name == "frontend"
    {
        tags.insert("frontend".to_string());
    }
    if is_js_family && !tags.contains("backend") && !tags.contains("test") {
        tags.insert("frontend".to_string());
    }

    ScopeProfile { name, tags }
}

fn scope_profile_for_bake_file(file: &BakeFile) -> ScopeProfile {
    if !file.scope_name.is_empty() {
        return ScopeProfile {
            name: file.scope_name.clone(),
            tags: file.scope_tags.iter().cloned().collect(),
        };
    }
    scope_profile(&file.path.to_string_lossy(), &file.language)
}

pub(crate) fn matches_scope_filter(path: &str, language: &str, scope_filter: Option<&str>) -> bool {
    let Some(scope_filter) = scope_filter else {
        return true;
    };
    let needle = scope_filter.to_lowercase();
    let profile = scope_profile(path, language);
    profile.name == needle
        || profile.tags.iter().any(|tag| tag == &needle)
        || path.to_lowercase().contains(&needle)
}

pub(crate) fn is_backend_endpoint_task(
    query: Option<&str>,
    endpoint: Option<&str>,
    symbol: Option<&str>,
    file_hint: Option<&str>,
) -> bool {
    let combined = [query, endpoint, symbol, file_hint]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    combined.contains("/api")
        || combined.contains("endpoint")
        || combined.contains("route")
        || combined.contains("handler")
        || combined.contains("backend")
        || combined.contains("http")
        || combined.contains("workflow")
}

pub(crate) fn summarize_scopes(files: &[BakeFile]) -> Vec<ScopeSummary> {
    let mut grouped: std::collections::BTreeMap<
        String,
        (usize, BTreeSet<String>, BTreeSet<String>),
    > = std::collections::BTreeMap::new();

    for file in files {
        let profile = scope_profile_for_bake_file(file);
        let entry = grouped
            .entry(profile.name)
            .or_insert_with(|| (0, BTreeSet::new(), BTreeSet::new()));
        entry.0 += 1;
        entry.1.insert(file.language.clone());
        entry.2.extend(profile.tags);
    }

    grouped
        .into_iter()
        .map(|(name, (file_count, languages, tags))| ScopeSummary {
            name,
            file_count,
            languages: languages.into_iter().collect(),
            tags: tags.into_iter().collect(),
        })
        .collect()
}

fn import_tokens(import: &str) -> BTreeSet<String> {
    import
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

pub(crate) fn summarize_scope_dependencies(files: &[BakeFile]) -> Vec<ScopeDependency> {
    let scope_names: BTreeSet<String> = files
        .iter()
        .map(|file| scope_profile_for_bake_file(file).name)
        .filter(|name| !name.is_empty())
        .collect();
    let mut dependencies = BTreeSet::new();

    for file in files {
        let source_scope = scope_profile_for_bake_file(file).name;
        if source_scope.is_empty() {
            continue;
        }
        for import in &file.imports {
            let tokens = import_tokens(import);
            for target_scope in &scope_names {
                if target_scope == &source_scope || !tokens.contains(target_scope) {
                    continue;
                }
                dependencies.insert(ScopeDependency {
                    scope: source_scope.clone(),
                    depends_on: target_scope.clone(),
                });
            }
        }
    }

    dependencies.into_iter().collect()
}

pub(crate) fn bake_scopes(bake: &BakeIndex) -> Vec<ScopeSummary> {
    if bake.scopes.is_empty() {
        summarize_scopes(&bake.files)
    } else {
        bake.scopes.clone()
    }
}

pub(crate) fn bake_scope_dependencies(bake: &BakeIndex) -> Vec<ScopeDependency> {
    if bake.scope_dependencies.is_empty() {
        summarize_scope_dependencies(&bake.files)
    } else {
        bake.scope_dependencies.clone()
    }
}

pub(crate) fn scope_hints(
    scopes: &[ScopeSummary],
    dependencies: &[ScopeDependency],
) -> Vec<String> {
    let has_backend = scopes
        .iter()
        .any(|scope| scope.tags.iter().any(|tag| tag == "backend"));
    let has_frontend = scopes
        .iter()
        .any(|scope| scope.tags.iter().any(|tag| tag == "frontend"));
    let has_tests = scopes
        .iter()
        .any(|scope| scope.tags.iter().any(|tag| tag == "test" || tag == "e2e"));
    let has_generated = scopes
        .iter()
        .any(|scope| scope.tags.iter().any(|tag| tag == "generated"));

    let mut hints = Vec::new();
    if has_backend && (has_frontend || has_tests) {
        hints.push(
            "Mixed backend/frontend/test repo detected. For backend API work, scope reads to the backend slice first."
                .to_string(),
        );
    }
    if has_tests {
        hints.push(
            "Test and e2e files are indexed too. They can pollute endpoint discovery unless you scope reads."
                .to_string(),
        );
    }
    if has_generated {
        hints.push(
            "Generated/OpenAPI code detected. If endpoint tracing misses, fall back to symbol-first search and inspect."
                .to_string(),
        );
    }
    if dependencies.iter().any(|dep| dep.scope != dep.depends_on) {
        hints.push(
            "Cross-scope imports detected. Changing shared/generated code may require refreshing dependent scopes too."
                .to_string(),
        );
    }
    hints
}

fn fingerprints_from_bake(bake: &BakeIndex) -> std::collections::HashMap<String, (i64, i64)> {
    bake.files
        .iter()
        .map(|file| {
            (
                file.path.to_string_lossy().into_owned(),
                (file.mtime_ns, file.bytes as i64),
            )
        })
        .collect()
}

fn expand_to_scope_files(
    bake: &BakeIndex,
    touched_files: &[String],
) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let reverse_dependencies = bake_scope_dependencies(bake).into_iter().fold(
        std::collections::HashMap::<String, Vec<String>>::new(),
        |mut acc, dep| {
            acc.entry(dep.depends_on).or_default().push(dep.scope);
            acc
        },
    );
    let mut affected_scopes = HashSet::new();
    let mut queue = Vec::new();
    let mut expanded = HashSet::new();

    for file in touched_files {
        let scope_name = bake
            .files
            .iter()
            .find(|indexed| indexed.path.to_string_lossy() == file.as_str())
            .map(|indexed| {
                if indexed.scope_name.is_empty() {
                    scope_profile(file, &indexed.language).name
                } else {
                    indexed.scope_name.clone()
                }
            })
            .unwrap_or_else(|| scope_profile(file, detect_language(Path::new(file))).name);
        if affected_scopes.insert(scope_name.clone()) {
            queue.push(scope_name);
        }
        expanded.insert(file.clone());
    }

    while let Some(scope_name) = queue.pop() {
        if let Some(dependents) = reverse_dependencies.get(&scope_name) {
            for dependent in dependents {
                if affected_scopes.insert(dependent.clone()) {
                    queue.push(dependent.clone());
                }
            }
        }
    }

    for file in &bake.files {
        let path = file.path.to_string_lossy().into_owned();
        let profile = scope_profile_for_bake_file(file);
        if affected_scopes.contains(&profile.name) {
            expanded.insert(path);
        }
    }

    expanded
}

fn refresh_scoped_paths(
    root: &PathBuf,
    bake: &BakeIndex,
    touched_files: &[String],
) -> Result<BakeIndex> {
    let bake_path = existing_bake_index_path(root).unwrap_or_else(|| bake_index_path(root));
    let affected = expand_to_scope_files(bake, touched_files);
    let mut fingerprints = fingerprints_from_bake(bake);
    for path in &affected {
        fingerprints.remove(path);
    }

    let (delta, removed, _) = build_bake_index(root, &fingerprints)?;
    super::db::write_bake_incremental(&delta, &removed, &bake_path).with_context(|| {
        format!(
            "Failed to write scoped bake index update to {}",
            bake_path.display()
        )
    })?;
    super::db::read_bake_from_db(&bake_path).with_context(|| {
        format!(
            "Failed to read refreshed bake index from {}",
            bake_path.display()
        )
    })
}

pub(crate) fn effective_scope(
    files: &[BakeFile],
    explicit_scope: Option<&str>,
    query: Option<&str>,
    endpoint: Option<&str>,
    symbol: Option<&str>,
    file_hint: Option<&str>,
) -> Option<String> {
    if let Some(scope) = explicit_scope {
        return Some(scope.to_lowercase());
    }

    if let Some(file_hint) = file_hint {
        return Some(scope_profile(file_hint, detect_language(Path::new(file_hint))).name);
    }

    if !is_backend_endpoint_task(query, endpoint, symbol, file_hint) {
        return None;
    }

    summarize_scopes(files)
        .into_iter()
        .filter(|scope| {
            scope.tags.iter().any(|tag| tag == "backend")
                && !scope
                    .tags
                    .iter()
                    .any(|tag| tag == "frontend" || tag == "test" || tag == "e2e")
        })
        .max_by(|left, right| left.file_count.cmp(&right.file_count))
        .map(|scope| scope.name)
}

pub(crate) fn backend_scope_boost(path: &str, language: &str) -> i32 {
    let profile = scope_profile(path, language);
    let mut score = 0i32;
    if profile.tags.iter().any(|tag| tag == "backend") {
        score += 25;
    }
    if profile.tags.iter().any(|tag| tag == "frontend") {
        score -= 20;
    }
    if profile.tags.iter().any(|tag| tag == "test" || tag == "e2e") {
        score -= 35;
    }
    score
}

pub(crate) fn project_snapshot(root: &PathBuf) -> Result<Snapshot> {
    let mut languages = BTreeSet::new();
    let mut files_indexed = 0usize;
    let mut files = Vec::new();

    fn walk(dir: &Path, languages: &mut BTreeSet<String>, count: &mut usize) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip common heavy/irrelevant directories for a quick snapshot.
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if is_bake_artifact_dir_name(name) {
                        continue;
                    }
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

    fn collect_files(root: &Path, files: &mut Vec<BakeFile>) -> Result<()> {
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if is_bake_artifact_dir_name(name) {
                        continue;
                    }
                    if matches!(
                        name,
                        ".git" | "node_modules" | "target" | "dist" | "build" | "__pycache__"
                    ) {
                        continue;
                    }
                }
                collect_files(&path, files)?;
            } else if path.is_file() {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                files.push(BakeFile {
                    path: rel,
                    language: detect_language(&path).to_string(),
                    scope_name: String::new(),
                    scope_tags: vec![],
                    bytes: 0,
                    mtime_ns: 0,
                    imports: vec![],
                    origin: "user".to_string(),
                });
            }
        }
        Ok(())
    }

    walk(root, &mut languages, &mut files_indexed)?;
    collect_files(root, &mut files)?;
    let scopes = summarize_scopes(&files);
    let scope_dependencies = summarize_scope_dependencies(&files);
    let scoping_hints = scope_hints(&scopes, &scope_dependencies);

    Ok(Snapshot {
        languages,
        files_indexed,
        scopes,
        scope_dependencies,
        scoping_hints,
    })
}

pub(crate) fn load_bake_index(root: &PathBuf) -> Result<Option<BakeIndex>> {
    let Some(bake_path) = existing_bake_index_path(root) else {
        return Ok(None);
    };

    let bake = super::db::read_bake_from_db(&bake_path)
        .with_context(|| format!("Failed to read bake index from {}", bake_path.display()))?;

    // Auto-reindex if the running binary is newer than what generated the index.
    let version_stale = parse_semver(env!("CARGO_PKG_VERSION")) > parse_semver(&bake.version);

    // Auto-reindex if any source file is newer than bake.db (or has gone missing).
    let bake_mtime = fs::metadata(&bake_path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let stale_files: Vec<String> = bake
        .files
        .iter()
        .filter_map(|file| {
            let path = file.path.to_string_lossy().into_owned();
            let stale = fs::metadata(root.join(&file.path))
                .and_then(|metadata| metadata.modified())
                .map(|mtime| mtime > bake_mtime)
                .unwrap_or(true);
            stale.then_some(path)
        })
        .collect();

    if version_stale {
        let (fresh, _, _) = build_bake_index(root, &std::collections::HashMap::new())?;
        prepare_bake_artifacts_dir(root)?;
        let fresh_path = bake_index_path(root);
        super::db::write_bake_to_db(&fresh, &fresh_path).with_context(|| {
            format!(
                "Failed to write refreshed bake index to {}",
                fresh_path.display()
            )
        })?;
        return Ok(Some(fresh));
    }

    if !stale_files.is_empty() {
        return Ok(Some(refresh_scoped_paths(root, &bake, &stale_files)?));
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
    if let Ok(out) = std::process::Command::new("zig")
        .args(["env", "--json"])
        .output()
    {
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
    if let Ok(out) = std::process::Command::new("go")
        .args(["env", "GOROOT"])
        .output()
    {
        if let Ok(s) = std::str::from_utf8(&out.stdout) {
            let p = PathBuf::from(s.trim()).join("src");
            if p.is_dir() {
                paths.push(("go".to_string(), p));
            }
        }
    }

    // Rust: rustc --print sysroot → .../lib/rustlib/src/rust/library
    if let Ok(out) = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
    {
        if let Ok(s) = std::str::from_utf8(&out.stdout) {
            let p = PathBuf::from(s.trim())
                .join("lib")
                .join("rustlib")
                .join("src")
                .join("rust")
                .join("library");
            if p.is_dir() {
                paths.push(("rust".to_string(), p));
            }
        }
    }

    // TypeScript: try npm, pnpm, yarn in order — first valid dir wins
    let ts_root = [
        ("npm", vec!["root", "-g"]),
        ("pnpm", vec!["root", "-g"]),
        ("yarn", vec!["global", "dir"]),
    ]
    .iter()
    .find_map(|(cmd, args)| {
        std::process::Command::new(cmd)
            .args(args.as_slice())
            .output()
            .ok()
            .and_then(|out| {
                std::str::from_utf8(&out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            })
            .map(|s| PathBuf::from(s).join("typescript").join("lib"))
            .filter(|p| p.is_dir())
    });
    if let Some(p) = ts_root {
        paths.push(("typescript".to_string(), p));
    }

    paths
}

/// Build or incrementally update the bake index.
///
/// `fingerprints` maps relative file path → (mtime_ns, bytes) from the existing DB.
/// Pass an empty map for a fresh full build.
///
/// Returns `(index, removed_paths, skipped_count)`:
/// - `index` contains only changed/new files and their parsed data
/// - `removed_paths` are files present in `fingerprints` but no longer on disk
/// - `skipped_count` is the number of files skipped due to matching fingerprint
pub(crate) fn build_bake_index(
    root: &PathBuf,
    fingerprints: &std::collections::HashMap<String, (i64, i64)>,
) -> Result<(BakeIndex, Vec<String>, usize)> {
    use ignore::WalkBuilder;
    use rayon::prelude::*;
    use std::collections::HashSet;

    // Phase 1: walk files, stat for mtime+size, skip unchanged ones.
    let mut languages = BTreeSet::new();
    let mut files: Vec<BakeFile> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::with_capacity(fingerprints.len());
    let mut skipped = 0usize;

    for result in WalkBuilder::new(root)
        .hidden(false) // don't skip hidden files — .gitignore handles exclusions
        .git_ignore(true) // respect .gitignore (nested, global, .git/info/exclude)
        .require_git(false) // apply .gitignore rules even outside a git repo
        .filter_entry(|e| {
            e.file_name() != ".git"
                && e.file_name() != BAKE_DIRNAME
                && e.file_name() != LEGACY_BAKE_DIRNAME
        }) // never descend into repo metadata or bake artifacts
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
        let meta = entry.metadata().ok();
        let bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime_ns = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

        let lang = detect_language(&path);
        if lang != "other" {
            languages.insert(lang.to_string());
        }
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let scope = scope_profile(&rel.to_string_lossy(), lang);
        let path_str = rel.to_string_lossy().into_owned();

        // Skip if mtime_ns+bytes match the stored fingerprint (and fingerprint is non-zero).
        let is_unchanged = fingerprints
            .get(&path_str)
            .map(|&(stored_mtime, stored_bytes)| {
                stored_mtime != 0 && stored_mtime == mtime_ns && stored_bytes == bytes as i64
            })
            .unwrap_or(false);

        seen_paths.insert(path_str);

        if is_unchanged {
            skipped += 1;
            continue;
        }

        files.push(BakeFile {
            path: rel,
            language: lang.to_string(),
            scope_name: scope.name,
            scope_tags: scope.tags.into_iter().collect(),
            bytes,
            mtime_ns,
            imports: vec![],
            origin: "user".to_string(),
        });
    }

    // Files in fingerprints that were not seen on disk have been removed.
    let removed: Vec<String> = fingerprints
        .keys()
        .filter(|p| !seen_paths.contains(*p))
        .cloned()
        .collect();

    // Phase 2: parse only changed/new files in parallel (CPU-bound tree-sitter work).
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
        let scope_name = files[idx].scope_name.clone();
        functions.extend(funcs.into_iter().map(|mut func| {
            func.scope_name = scope_name.clone();
            func
        }));
        endpoints.extend(eps.into_iter().map(|mut endpoint| {
            endpoint.scope_name = scope_name.clone();
            endpoint
        }));
        types.extend(typs.into_iter().map(|mut typ| {
            typ.scope_name = scope_name.clone();
            typ
        }));
        impls.extend(imps);
        files[idx].imports = imports;
    }

    let scopes = summarize_scopes(&files);
    let scope_dependencies = summarize_scope_dependencies(&files);

    Ok((
        BakeIndex {
            version: env!("CARGO_PKG_VERSION").to_string(),
            project_root: root.clone(),
            languages,
            files,
            scopes,
            scope_dependencies,
            functions,
            endpoints,
            types,
            impls,
        },
        removed,
        skipped,
    ))
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
        Some("clj") | Some("cljs") | Some("cljc") => "clojure",
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

    let bake = match super::db::read_bake_from_db(&bake_path) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };
    let touched: Vec<String> = changed_files
        .iter()
        .map(|file| (*file).to_string())
        .collect();
    let _ = refresh_scoped_paths(root, &bake, &touched)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::engine::types::BakeIndex;

    fn write_bake(root: &TempDir) {
        let bakes_dir = bake_artifacts_dir(root.path());
        fs::create_dir_all(&bakes_dir).unwrap();
        let bake = BakeIndex {
            version: env!("CARGO_PKG_VERSION").to_string(),
            project_root: root.path().to_path_buf(),
            languages: BTreeSet::new(),
            files: vec![],
            scopes: vec![],
            scope_dependencies: vec![],
            functions: vec![],
            endpoints: vec![],
            types: vec![],
            impls: vec![],
        };
        crate::engine::db::write_bake_to_db(&bake, &bakes_dir.join("bake.db")).unwrap();
    }

    #[test]
    fn ensure_bake_gitignored_writes_info_exclude() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();

        ensure_bake_gitignored(dir.path()).unwrap();
        let exclude = fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
        assert!(exclude.contains(".bakes/"));

        ensure_bake_gitignored(dir.path()).unwrap();
        let exclude = fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
        assert_eq!(
            exclude
                .lines()
                .filter(|line| line.trim() == ".bakes/")
                .count(),
            1
        );
    }

    #[test]
    fn ensure_bake_gitignored_supports_gitdir_file() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git-worktree");
        fs::create_dir_all(git_dir.join("info")).unwrap();
        fs::write(dir.path().join(".git"), "gitdir: .git-worktree\n").unwrap();

        ensure_bake_gitignored(dir.path()).unwrap();

        let exclude = fs::read_to_string(git_dir.join("info/exclude")).unwrap();
        assert!(exclude.contains(".bakes/"));
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
        assert!(err.contains(&format!(
            "Did you mean to pass the project root {}",
            dir.path().display()
        )));
    }

    #[test]
    fn detect_language_supports_clojure_extensions() {
        assert_eq!(detect_language(Path::new("src/core.clj")), "clojure");
        assert_eq!(detect_language(Path::new("src/core.cljs")), "clojure");
        assert_eq!(detect_language(Path::new("src/core.cljc")), "clojure");
    }

    fn write_mixed_repo(root: &TempDir) {
        fs::create_dir_all(root.path().join("backend")).unwrap();
        fs::create_dir_all(root.path().join("web")).unwrap();
        fs::write(
            root.path().join("backend/a.ts"),
            "function alpha() { return beta(); }\nfunction beta() { return 1; }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("backend/b.ts"),
            "function gamma() { return 2; }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("web/ui.ts"),
            "function renderUi() { return 3; }\n",
        )
        .unwrap();
    }

    fn write_dependency_repo(root: &TempDir) {
        fs::create_dir_all(root.path().join("backend")).unwrap();
        fs::create_dir_all(root.path().join("generated")).unwrap();
        fs::create_dir_all(root.path().join("web")).unwrap();
        fs::write(
            root.path().join("backend/server.ts"),
            "import { client } from \"../generated/client\";\nfunction handler() { return client(); }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("generated/client.ts"),
            "export function client() { return 1; }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("web/app.ts"),
            "function renderUi() { return 3; }\n",
        )
        .unwrap();
    }

    #[test]
    fn reindex_files_expands_to_changed_scope() {
        let dir = TempDir::new().unwrap();
        write_mixed_repo(&dir);
        std::env::set_var("YOYO_SKIP_EMBED", "1");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        std::env::remove_var("YOYO_SKIP_EMBED");

        fs::write(
            dir.path().join("backend/b.ts"),
            "function gammaUpdated() { return 4; }\n",
        )
        .unwrap();

        reindex_files(&dir.path().to_path_buf(), &["backend/a.ts"]).unwrap();
        let bake = require_bake_index(&dir.path().to_path_buf()).unwrap();

        assert!(bake.functions.iter().any(|f| f.name == "gammaUpdated"));
        assert!(!bake.functions.iter().any(|f| f.name == "gamma"));
        assert!(bake.functions.iter().any(|f| f.name == "renderUi"));
    }

    #[test]
    fn load_bake_index_refreshes_stale_scope_only() {
        let dir = TempDir::new().unwrap();
        write_mixed_repo(&dir);
        std::env::set_var("YOYO_SKIP_EMBED", "1");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        std::env::remove_var("YOYO_SKIP_EMBED");

        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            dir.path().join("backend/b.ts"),
            "function delta() { return 5; }\n",
        )
        .unwrap();

        let bake = load_bake_index(&dir.path().to_path_buf()).unwrap().unwrap();
        assert!(bake.functions.iter().any(|f| f.name == "delta"));
        assert!(!bake.functions.iter().any(|f| f.name == "gamma"));
        assert!(bake.functions.iter().any(|f| f.name == "renderUi"));
    }

    #[test]
    fn dependency_refresh_expands_to_dependent_scopes() {
        let dir = TempDir::new().unwrap();
        write_dependency_repo(&dir);
        std::env::set_var("YOYO_SKIP_EMBED", "1");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        std::env::remove_var("YOYO_SKIP_EMBED");

        let bake = require_bake_index(&dir.path().to_path_buf()).unwrap();
        let expanded = expand_to_scope_files(&bake, &["generated/client.ts".to_string()]);

        assert!(expanded.contains("generated/client.ts"));
        assert!(expanded.contains("backend/server.ts"));
        assert!(!expanded.contains("web/app.ts"));
    }
}
