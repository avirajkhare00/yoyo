use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use super::types::{GraphAddPayload, GraphCreatePayload, GraphMovePayload, GraphRenamePayload, TraceDownPayload, TraceNode};
use super::edit::ast_check_str;
use super::util::{detect_language, load_bake_index, reindex_files, require_bake_index, resolve_project_root};
use crate::lang::Visibility;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Find all byte ranges where `name` appears as a whole identifier (word-boundary).
fn find_identifier_occurrences(content: &[u8], name: &[u8]) -> Vec<(usize, usize)> {
    let len = name.len();
    if len == 0 {
        return vec![];
    }
    let mut result = Vec::new();
    let mut i = 0;
    while i + len <= content.len() {
        if &content[i..i + len] == name {
            let before_ok = i == 0 || !is_word_char(content[i - 1]);
            let after_ok = i + len >= content.len() || !is_word_char(content[i + len]);
            if before_ok && after_ok {
                result.push((i, i + len));
            }
        }
        i += 1;
    }
    result
}

/// Walk the project and collect all source files (as absolute paths).
fn collect_source_files(root: &PathBuf) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if matches!(
                            name,
                            ".git"
                                | "node_modules"
                                | "target"
                                | "dist"
                                | "build"
                                | "__pycache__"
                        ) {
                            continue;
                        }
                    }
                    walk(&path, out);
                } else if path.is_file() {
                    out.push(path);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    files
}

/// A typed parameter for scaffold generation.
pub struct Param {
    pub name: String,
    pub type_str: String,
}

/// Generate a function scaffold with typed signature (params / return type / method receiver).
/// Type strings are passed verbatim — callers supply language-appropriate types.
fn generate_typed_scaffold(
    name: &str,
    lang: &str,
    params: &[Param],
    returns: Option<&str>,
    on: Option<&str>,
) -> String {
    let param_str: String = match lang {
        "go" => params.iter().map(|p| format!("{} {}", p.name, p.type_str)).collect::<Vec<_>>().join(", "),
        _ => params.iter().map(|p| format!("{}: {}", p.name, p.type_str)).collect::<Vec<_>>().join(", "),
    };

    match lang {
        "rust" => {
            let ret = returns.map(|r| format!(" -> {}", r)).unwrap_or_default();
            if let Some(struct_name) = on {
                format!("\nimpl {} {{\n    fn {}({}){} {{\n        todo!()\n    }}\n}}\n", struct_name, name, param_str, ret)
            } else {
                format!("\nfn {}({}){} {{\n    todo!()\n}}\n", name, param_str, ret)
            }
        }
        "go" => {
            let ret = returns.map(|r| format!(" {}", r)).unwrap_or_default();
            if let Some(struct_name) = on {
                let recv = struct_name.chars().next().map(|c| c.to_lowercase().to_string()).unwrap_or_else(|| "r".to_string());
                format!("\nfunc ({} *{}) {}({}){} {{\n    // TODO\n}}\n", recv, struct_name, name, param_str, ret)
            } else {
                format!("\nfunc {}({}){} {{\n    // TODO\n}}\n", name, param_str, ret)
            }
        }
        "typescript" | "javascript" => {
            let ret = returns.map(|r| format!(": {}", r)).unwrap_or_default();
            format!("\nfunction {}({}){} {{\n    // TODO\n}}\n", name, param_str, ret)
        }
        "zig" => {
            let ret = returns.unwrap_or("void");
            format!("\nfn {}({}) {} {{\n    // TODO\n}}\n", name, param_str, ret)
        }
        "python" => {
            let ret = returns.map(|r| format!(" -> {}", r)).unwrap_or_default();
            let self_prefix = if on.is_some() { "self, " } else { "" };
            format!("\ndef {}({}{}){} :\n    pass\n", name, self_prefix, param_str, ret)
        }
        _ => format!("\nfn {}({}) {{\n    // TODO\n}}\n", name, param_str),
    }
}

/// Generate a language-idiomatic test function scaffold for `fn_name`.
fn generate_test_scaffold(fn_name: &str, lang: &str) -> String {
    match lang {
        "rust" => format!(
            "\n#[test]\nfn test_{}() {{\n    todo!()\n}}\n",
            fn_name
        ),
        "go" => {
            // Go test functions must be exported: TestFnName
            let mut test_name = String::from("Test");
            let mut chars = fn_name.chars();
            if let Some(first) = chars.next() {
                test_name.extend(first.to_uppercase());
                test_name.push_str(chars.as_str());
            }
            format!(
                "\nfunc {}(t *testing.T) {{\n    // TODO\n}}\n",
                test_name
            )
        }
        "typescript" | "javascript" => format!(
            "\nit(\"{}\", () => {{\n    // TODO\n}});\n",
            fn_name
        ),
        "zig" => format!(
            "\ntest \"{}\" {{\n    // TODO\n}}\n",
            fn_name
        ),
        "python" => format!(
            "\ndef test_{}():\n    pass\n",
            fn_name
        ),
        _ => format!(
            "\n#[test]\nfn test_{}() {{\n    todo!()\n}}\n",
            fn_name
        ),
    }
}

fn generate_scaffold(entity_type: &str, name: &str, lang: &str) -> String {
    match entity_type {
        "fn" => format!("\nfn {}() {{\n    todo!()\n}}\n", name),
        "function" => format!("\nfunction {}() {{\n    // TODO\n}}\n", name),
        "def" => format!("\ndef {}():\n    pass\n", name),
        "func" => format!("\nfunc {}() {{\n    // TODO\n}}\n", name),
        _ => match lang {
            "rust" => format!("\nfn {}() {{\n    todo!()\n}}\n", name),
            "typescript" | "javascript" => {
                format!("\nfunction {}() {{\n    // TODO\n}}\n", name)
            }
            "python" => format!("\ndef {}():\n    pass\n", name),
            "go" => format!("\nfunc {}() {{\n    // TODO\n}}\n", name),
            _ => format!("\nfn {}() {{\n    todo!()\n}}\n", name),
        },
    }
}

// ── graph_rename ─────────────────────────────────────────────────────────────

/// Rename a symbol everywhere — definition + all call sites — atomically.
/// Scope is determined by the symbol's visibility in the bake index:
///   Private  → rename only within the defining file (safe, no external callers)
///   Module   → rename within all files in the same directory (same package)
///   Public   → rename project-wide + emit a warning (external callers may exist)
pub fn graph_rename(
    path: Option<String>,
    name: String,
    new_name: String,
) -> Result<String> {
    if name == new_name {
        return Err(anyhow!("old_name and new_name are identical: {:?}", name));
    }
    let root = resolve_project_root(path)?;
    let name_bytes = name.as_bytes().to_vec();
    let name_lc = name.to_lowercase();

    // Determine rename scope from bake index visibility.
    let bake = load_bake_index(&root)?;
    let (source_files, scope_label, warnings) = if let Some(ref bake) = bake {
        if let Some(func) = bake.functions.iter().find(|f| f.name.to_lowercase() == name_lc) {
            match func.visibility {
                Visibility::Private => {
                    // Private: safe to rename only the defining file.
                    let scoped = vec![root.join(&func.file)];
                    (scoped, "file".to_string(), vec![])
                }
                Visibility::Module => {
                    // Module-visible (pub(crate) / Go package): scope to files in the same dir.
                    let def_dir = std::path::Path::new(&func.file)
                        .parent()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let scoped: Vec<PathBuf> = bake.files.iter()
                        .filter(|f| {
                            let fdir = std::path::Path::new(&f.path)
                                .parent()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            fdir == def_dir
                        })
                        .map(|f| root.join(&f.path))
                        .collect();
                    (scoped, "package".to_string(), vec![])
                }
                Visibility::Public => {
                    // Public: rename project-wide but warn that callers outside the
                    // call graph (dynamic dispatch, reflection) may also need updating.
                    (collect_source_files(&root), "project".to_string(), vec![
                        format!("'{}' is public — callers outside the call graph (dynamic dispatch, FFI) may need manual review after rename", name),
                    ])
                }
            }
        } else {
            // Symbol not in index (unbaked or unsupported language) — fall back to project-wide.
            (collect_source_files(&root), "project".to_string(), vec![
                format!("'{}' not in bake index — run `bake` first for visibility-scoped rename; falling back to project-wide search", name),
            ])
        }
    } else {
        // No bake index at all — project-wide fallback.
        (collect_source_files(&root), "project".to_string(), vec![
            "No bake index — run `bake` first for visibility-scoped rename; falling back to project-wide search".to_string(),
        ])
    };

    // Collect (rel_path, occurrences) for each file that contains the identifier.
    let mut edits_by_file: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    let mut total_occurrences = 0usize;

    for full_path in &source_files {
        let lang = detect_language(full_path);
        if lang == "other" {
            continue;
        }
        let Ok(content) = fs::read(full_path) else {
            continue;
        };
        let occurrences = find_identifier_occurrences(&content, &name_bytes);
        if !occurrences.is_empty() {
            let rel = full_path
                .strip_prefix(&root)
                .unwrap_or(full_path)
                .to_string_lossy()
                .into_owned();
            total_occurrences += occurrences.len();
            edits_by_file.push((rel, occurrences));
        }
    }

    if total_occurrences == 0 {
        return Err(anyhow!(
            "No occurrences of identifier {:?} found in source files.",
            name
        ));
    }

    let files_changed = edits_by_file.len();
    let mut all_changed_files: Vec<String> = Vec::new();

    for (rel, mut occs) in edits_by_file {
        let full_path = root.join(&rel);
        let mut bytes = fs::read(&full_path)
            .with_context(|| format!("Failed to read {}", rel))?;

        // Apply bottom-up so earlier offsets stay valid.
        occs.sort_by(|a, b| b.0.cmp(&a.0));
        for (start, end) in &occs {
            bytes.splice(start..end, new_name.as_bytes().iter().copied());
        }

        // Pre-write AST check — reject before touching disk.
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match std::str::from_utf8(&bytes) {
            Err(_) => return Err(anyhow!(
                "graph_rename rejected: result is invalid UTF-8 in {} (file not modified)",
                rel
            )),
            Ok(patched_str) => {
                let pre_errors = ast_check_str(patched_str, ext);
                if !pre_errors.is_empty() {
                    let summary: Vec<String> = pre_errors.iter()
                        .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                        .collect();
                    return Err(anyhow!(
                        "graph_rename rejected: syntax errors in {} (file not modified):\n{}",
                        rel, summary.join("\n")
                    ));
                }
            }
        }

        fs::write(&full_path, &bytes)
            .with_context(|| format!("Failed to write {}", rel))?;
        all_changed_files.push(rel);
    }

    let refs: Vec<&str> = all_changed_files.iter().map(|s| s.as_str()).collect();
    let _ = reindex_files(&root, &refs);

    let payload = GraphRenamePayload {
        tool: "graph_rename",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        old_name: name,
        new_name,
        scope: scope_label,
        files_changed,
        occurrences_renamed: total_occurrences,
        warnings,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── graph_add ────────────────────────────────────────────────────────────────

/// Insert a new function scaffold at a specified location.
pub fn graph_add(
    path: Option<String>,
    entity_type: String,
    name: String,
    file: String,
    after_symbol: Option<String>,
    language: Option<String>,
    params: Option<Vec<Param>>,
    returns: Option<String>,
    on: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let full_path = root.join(&file);

    let lang_owned;
    let lang: &str = if let Some(ref l) = language {
        l.as_str()
    } else {
        lang_owned = detect_language(&full_path).to_string();
        &lang_owned
    };

    // Find insertion byte offset.
    let insert_at = if let Some(sym) = after_symbol {
        let bake = require_bake_index(&root)?;
        let file_lc = file.to_lowercase();
        let sym_lc = sym.to_lowercase();
        bake.functions
            .iter()
            .find(|f| {
                f.file.to_lowercase().ends_with(&file_lc)
                    && (f.name.to_lowercase() == sym_lc
                        || f.name.to_lowercase().contains(&sym_lc))
            })
            .map(|f| f.byte_end)
            .ok_or_else(|| anyhow!("Symbol {:?} not found in {:?}", sym, file))?
    } else {
        // Append to end of file.
        if full_path.exists() {
            fs::metadata(&full_path)?.len() as usize
        } else {
            0
        }
    };

    let scaffold = if entity_type == "test" {
        generate_test_scaffold(&name, lang)
    } else if let Some(ref p) = params {
        generate_typed_scaffold(&name, lang, p, returns.as_deref(), on.as_deref())
    } else {
        generate_scaffold(&entity_type, &name, lang)
    };
    let scaffold_bytes = scaffold.as_bytes();

    let mut bytes = if full_path.exists() {
        fs::read(&full_path).with_context(|| format!("Failed to read {}", file))?
    } else {
        Vec::new()
    };
    let insert_at = insert_at.min(bytes.len());
    bytes.splice(insert_at..insert_at, scaffold_bytes.iter().copied());

    // Pre-write AST check — reject before touching disk.
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if let Ok(patched_str) = std::str::from_utf8(&bytes) {
        let pre_errors = ast_check_str(patched_str, ext);
        if !pre_errors.is_empty() {
            let summary: Vec<String> = pre_errors.iter()
                .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                .collect();
            return Err(anyhow!(
                "graph_add rejected: syntax errors in {} (file not modified):\n{}",
                file, summary.join("\n")
            ));
        }
    }

    fs::write(&full_path, &bytes)
        .with_context(|| format!("Failed to write {}", file))?;

    let _ = reindex_files(&root, &[file.as_str()]);

    let payload = GraphAddPayload {
        tool: "graph_add",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        entity_type,
        name,
        file,
        inserted_at_byte: insert_at,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── graph_create ─────────────────────────────────────────────────────────────

/// Create a new file with an initial function scaffold and reindex it.
/// Errors if the file already exists or if the parent directory does not exist.
pub fn graph_create(
    path: Option<String>,
    file: String,
    function_name: String,
    language: Option<String>,
    params: Option<Vec<Param>>,
    returns: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let full_path = root.join(&file);

    if full_path.exists() {
        return Err(anyhow!(
            "File {:?} already exists. Use graph_add to insert a function into an existing file.",
            file
        ));
    }

    if let Some(parent) = full_path.parent() {
        if !parent.exists() {
            return Err(anyhow!(
                "Parent directory {:?} does not exist. Create it first.",
                parent.display()
            ));
        }
    }

    let lang_owned;
    let lang: &str = if let Some(ref l) = language {
        l.as_str()
    } else {
        lang_owned = detect_language(&full_path).to_string();
        &lang_owned
    };

    let entity_type = match lang {
        "rust" => "fn",
        "python" => "def",
        "go" => "func",
        _ => "function",
    };

    let scaffold = if let Some(ref p) = params {
        generate_typed_scaffold(&function_name, lang, p, returns.as_deref(), None)
    } else {
        generate_scaffold(entity_type, &function_name, lang)
    };
    let scaffold_content = scaffold.trim_start().to_string();

    // Pre-write AST check — reject before touching disk.
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let pre_errors = ast_check_str(&scaffold_content, ext);
    if !pre_errors.is_empty() {
        let summary: Vec<String> = pre_errors.iter()
            .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
            .collect();
        return Err(anyhow!(
            "graph_create rejected: syntax errors in scaffold for {} (file not modified):\n{}",
            file, summary.join("\n")
        ));
    }

    fs::write(&full_path, &scaffold_content)
        .with_context(|| format!("Failed to create file {}", file))?;

    let _ = reindex_files(&root, &[file.as_str()]);

    let payload = GraphCreatePayload {
        tool: "graph_create",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        function_name,
        language: lang.to_string(),
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── graph_move ───────────────────────────────────────────────────────────────

/// Scan the moved function body for identifiers that match `use` statements in
/// the source file, and return the ones not already present in the destination.
///
/// Strategy: extract word tokens from the function body, then for each `use`
/// statement in the source file extract the "exposed" identifier (last path
/// segment, alias, or brace-list members) and check if it appears in the body.
fn inject_needed_imports(body: &str, src_content: &str, dst_content: &str) -> Vec<String> {
    // Word tokens present in the function body.
    let body_words: std::collections::HashSet<&str> = body
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .collect();

    let mut needed = Vec::new();
    for line in src_content.lines() {
        let t = line.trim();
        if !t.starts_with("use ") && !t.starts_with("import ") && !t.starts_with("from ") {
            continue;
        }
        // Skip relative imports — `use self::`, `use super::` are module-local
        // and won't be valid in the destination file.
        if t.starts_with("use self::") || t.starts_with("use super::") {
            continue;
        }
        // Skip if the destination already has this import line.
        if dst_content.contains(t) {
            continue;
        }
        let exposed = exposed_idents_from_import(t);
        // Wildcard imports are always included if source has them.
        let matches = exposed.iter().any(|id| id == "*" || body_words.contains(id.as_str()));
        if matches {
            needed.push(t.to_string());
        }
    }
    needed
}

/// Extract the set of identifiers a single import statement exposes.
/// Works for Rust (`use`), TypeScript/JS (`import`), Python (`from`/`import`).
fn exposed_idents_from_import(stmt: &str) -> Vec<String> {
    let t = stmt.trim();

    // Rust: `use path::to::{A, B as C};` or `use path::to::D;`
    if t.starts_with("use ") {
        let inner = t.trim_start_matches("use ").trim_end_matches(';').trim();
        if let Some(brace) = inner.find('{') {
            let rest = &inner[brace + 1..];
            let close = rest.find('}').unwrap_or(rest.len());
            return rest[..close]
                .split(',')
                .map(|s| {
                    let s = s.trim();
                    // handle `Name as Alias`
                    s.split(" as ").last().unwrap_or(s)
                        .split("::")
                        .last()
                        .unwrap_or(s)
                        .trim()
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(as_pos) = inner.rfind(" as ") {
            return vec![inner[as_pos + 4..].trim().to_string()];
        }
        if inner == "*" || inner.ends_with("::*") {
            return vec!["*".to_string()];
        }
        if let Some(last) = inner.split("::").last() {
            return vec![last.trim_end_matches(';').trim().to_string()];
        }
    }

    // TypeScript/JS: `import { A, B } from '...'` or `import X from '...'`
    if t.starts_with("import ") {
        if let Some(brace) = t.find('{') {
            if let Some(close) = t.find('}') {
                return t[brace + 1..close]
                    .split(',')
                    .map(|s| s.trim().split(" as ").last().unwrap_or("").trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        // `import X from ...` — X is the default import identifier
        let after_import = t.trim_start_matches("import ").trim();
        if let Some(from_pos) = after_import.find(" from ") {
            let name = after_import[..from_pos].trim();
            if !name.is_empty() {
                return vec![name.to_string()];
            }
        }
    }

    // Python: `from module import A, B` or `import module`
    if t.starts_with("from ") {
        if let Some(import_pos) = t.find(" import ") {
            return t[import_pos + 8..]
                .split(',')
                .map(|s| s.trim().split(" as ").last().unwrap_or("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    if t.starts_with("import ") {
        return t[7..]
            .split(',')
            .map(|s| s.trim().split(" as ").last().unwrap_or("").trim().split('.').last().unwrap_or("").to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    vec![]
}

/// Move a function from one file to another.
pub fn graph_move(
    path: Option<String>,
    name: String,
    to_file: String,
) -> Result<String> {
    let root = resolve_project_root(path)?;

    let bake = require_bake_index(&root)?;

    let sym_lc = name.to_lowercase();
    let func = bake
        .functions
        .iter()
        .find(|f| f.name.to_lowercase() == sym_lc)
        .ok_or_else(|| anyhow!("Symbol {:?} not found in bake index.", name))?;

    let from_file = func.file.clone();
    let byte_start = func.byte_start;
    let byte_end = func.byte_end;

    if from_file == to_file {
        return Err(anyhow!(
            "Source and destination files are the same: {:?}",
            from_file
        ));
    }

    let src_full = root.join(&from_file);
    let mut src_bytes = fs::read(&src_full)
        .with_context(|| format!("Failed to read source {}", from_file))?;

    if byte_end > src_bytes.len() || byte_start > byte_end {
        return Err(anyhow!(
            "Invalid byte range [{}, {}) for {} (file len {})",
            byte_start,
            byte_end,
            from_file,
            src_bytes.len()
        ));
    }

    let func_body: Vec<u8> = src_bytes[byte_start..byte_end].to_vec();

    // Remove from source (in memory only — no disk write yet).
    src_bytes.drain(byte_start..byte_end);

    // Build destination (in memory only — no disk write yet).
    let dst_full = root.join(&to_file);
    let mut dst_bytes = if dst_full.exists() {
        fs::read(&dst_full).with_context(|| format!("Failed to read dest {}", to_file))?
    } else {
        Vec::new()
    };

    // Ensure a blank line separator before the moved function.
    if !dst_bytes.is_empty() && *dst_bytes.last().unwrap() != b'\n' {
        dst_bytes.push(b'\n');
    }
    dst_bytes.push(b'\n');
    dst_bytes.extend_from_slice(&func_body);
    if dst_bytes.last().copied().unwrap_or(b'\n') != b'\n' {
        dst_bytes.push(b'\n');
    }

    // Import fixup: inject any `use` statements the moved function needs into the destination.
    let src_content = String::from_utf8_lossy(&src_bytes).into_owned();
    let dst_content = String::from_utf8_lossy(&dst_bytes).into_owned();
    let func_body_str = String::from_utf8_lossy(&func_body).into_owned();
    let imports_added = inject_needed_imports(&func_body_str, &src_content, &dst_content);

    let final_dst = if imports_added.is_empty() {
        dst_bytes
    } else {
        let import_block = imports_added.iter()
            .map(|s| format!("{}\n", s))
            .collect::<String>();
        let mut out = import_block.into_bytes();
        out.extend_from_slice(&dst_bytes);
        out
    };

    // Pre-write AST checks — both files checked before either is written to disk.
    let src_ext = src_full.extension().and_then(|e| e.to_str()).unwrap_or("");
    match std::str::from_utf8(&src_bytes) {
        Err(_) => return Err(anyhow!(
            "graph_move rejected: invalid UTF-8 in source {} after removal (no files modified)",
            from_file
        )),
        Ok(src_str) => {
            let pre_errors = ast_check_str(src_str, src_ext);
            if !pre_errors.is_empty() {
                let summary: Vec<String> = pre_errors.iter()
                    .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                    .collect();
                return Err(anyhow!(
                    "graph_move rejected: syntax errors in source {} after removal (no files modified):\n{}",
                    from_file, summary.join("\n")
                ));
            }
        }
    }

    let dst_ext = dst_full.extension().and_then(|e| e.to_str()).unwrap_or("");
    match std::str::from_utf8(&final_dst) {
        Err(_) => return Err(anyhow!(
            "graph_move rejected: invalid UTF-8 in dest {} (no files modified)",
            to_file
        )),
        Ok(dst_str) => {
            let pre_errors = ast_check_str(dst_str, dst_ext);
            if !pre_errors.is_empty() {
                let summary: Vec<String> = pre_errors.iter()
                    .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                    .collect();
                return Err(anyhow!(
                    "graph_move rejected: syntax errors in dest {} (no files modified):\n{}",
                    to_file, summary.join("\n")
                ));
            }
        }
    }

    // Both checks passed — safe to write.
    fs::write(&src_full, &src_bytes)
        .with_context(|| format!("Failed to write source {}", from_file))?;
    fs::write(&dst_full, &final_dst)
        .with_context(|| format!("Failed to write dest {}", to_file))?;

    let _ = reindex_files(&root, &[from_file.as_str(), to_file.as_str()]);

    let payload = GraphMovePayload {
        tool: "graph_move",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        name,
        from_file,
        to_file,
        imports_added,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── trace_down ────────────────────────────────────────────────────────────────

/// Known external boundary signals — qualifier substrings → boundary label.
const DB_QUALIFIERS: &[&str] = &["db", "sql", "gorm", "sqlx", "pg", "mysql", "mongo", "redis", "store", "repo", "repository"];
const DB_CALLEES:    &[&str] = &["query", "exec", "find", "findbyid", "insert", "update", "delete", "save", "scan", "get", "create"];
const HTTP_QUALIFIERS: &[&str] = &["client", "http", "fetch", "axios", "reqwest", "request", "httpclient"];
const HTTP_CALLEES:    &[&str] = &["get", "post", "put", "patch", "delete", "do", "send", "request", "call"];
const QUEUE_QUALIFIERS: &[&str] = &["kafka", "iggy", "nats", "rabbit", "queue", "producer", "publisher", "broker"];
/// Receiver names too generic to use as qualifier hints.
const TRIVIAL_RECEIVERS: &[&str] = &["self", "this", "c", "s", "r", "ctx", "app", "e", "g"];

fn boundary_from_call(callee: &str, qualifier: &Option<String>) -> Option<String> {
    let cl = callee.to_lowercase();
    let ql = qualifier.as_deref().unwrap_or("").to_lowercase();

    if DB_QUALIFIERS.iter().any(|q| ql.contains(q)) && DB_CALLEES.iter().any(|c| cl == *c) {
        return Some("database".into());
    }
    if HTTP_QUALIFIERS.iter().any(|q| ql.contains(q)) && HTTP_CALLEES.iter().any(|c| cl == *c) {
        return Some("http_client".into());
    }
    if QUEUE_QUALIFIERS.iter().any(|q| ql.contains(q)) {
        return Some("queue".into());
    }
    None
}

fn resolve_candidate<'a>(
    candidates: &[&'a crate::lang::IndexedFunction],
    caller: &crate::lang::IndexedFunction,
    qualifier: &Option<String>,
) -> Option<&'a crate::lang::IndexedFunction> {
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }

    // All candidates share the same name — use it for qualified_name matching.
    let fn_name = candidates[0].name.to_lowercase();

    if let Some(qual) = qualifier {
        let ql = qual.to_lowercase();

        if !TRIVIAL_RECEIVERS.contains(&ql.as_str()) {
            // 1. Exact qualified_name match (Rust: engine::fn, Go: pkg/fn, Python: mod.fn).
            //    Handles calls like `engine::process(...)` where qualifier == module_path.
            let sep_rust = format!("{}::{}", ql, fn_name);
            let sep_slash = format!("{}/{}", ql, fn_name);
            let sep_dot = format!("{}.{}", ql, fn_name);
            if let Some(m) = candidates.iter().find(|f| {
                let qn = f.qualified_name.to_lowercase();
                qn == sep_rust || qn == sep_slash || qn == sep_dot
            }) {
                return Some(m);
            }

            // 2. Qualifier exactly matches module_path (Go packages, Python modules).
            if let Some(m) = candidates.iter().find(|f| f.module_path.to_lowercase() == ql) {
                return Some(m);
            }

            // 3. Qualifier ends with module_path — handles `crate::engine` matching
            //    module_path `engine` in Rust.
            if let Some(m) = candidates.iter().find(|f| {
                let mp = f.module_path.to_lowercase();
                !mp.is_empty() && ql.ends_with(mp.as_str())
            }) {
                return Some(m);
            }

            // 4. Fallback: qualifier as file path substring (original heuristic).
            if let Some(m) = candidates.iter().find(|f| f.file.to_lowercase().contains(&ql)) {
                return Some(m);
            }
        }
    }

    // Same language, single match.
    let same_lang: Vec<_> = candidates.iter().filter(|f| f.language == caller.language).collect();
    if same_lang.len() == 1 {
        return Some(same_lang[0]);
    }
    // Closest directory.
    if let Some(dir) = caller.file.rsplit('/').skip(1).next() {
        if let Some(m) = candidates.iter().find(|f| f.file.contains(dir)) {
            return Some(m);
        }
    }
    candidates.first().copied()
}

fn is_stdlib_symbol(callee: &str, qualifier: Option<&str>) -> bool {
    // Known Go stdlib package qualifiers
    const GO_PKGS: &[&str] = &[
        "fmt", "log", "time", "errors", "strings", "strconv", "os", "io", "sync",
        "context", "math", "sort", "regexp", "http", "json", "bytes", "bufio",
        "filepath", "path", "runtime", "reflect", "atomic", "rand", "hex", "base64",
        "ioutil", "net", "url", "unicode", "utf8",
    ];
    // Known Go builtins (no qualifier)
    const GO_BUILTINS: &[&str] = &[
        "len", "cap", "make", "append", "delete", "new", "close", "panic", "recover",
        "print", "println", "copy", "complex", "real", "imag", "string", "int",
        "uint", "float64", "float32", "bool", "byte", "rune", "error",
    ];
    // Known Rust builtins / common std items (no qualifier)
    const RUST_BUILTINS: &[&str] = &[
        "println", "eprintln", "print", "eprint", "vec", "format", "assert",
        "assert_eq", "assert_ne", "panic", "todo", "unimplemented", "unreachable",
        "dbg", "write", "writeln", "unwrap", "expect", "clone", "into", "from",
        "len", "push", "pop", "is_empty", "to_string", "to_owned", "as_str",
    ];

    if let Some(q) = qualifier {
        GO_PKGS.contains(&q)
    } else {
        GO_BUILTINS.contains(&callee) || RUST_BUILTINS.contains(&callee)
    }
}

/// Core BFS trace — reusable by both `trace_down` and `flow`.
/// Returns (chain, unresolved) starting from `start`, up to `max_depth`.
pub(crate) fn trace_chain<'a>(
    bake: &'a super::types::BakeIndex,
    start: &'a crate::lang::IndexedFunction,
    max_depth: usize,
) -> (Vec<TraceNode>, Vec<String>) {
    let mut by_name: HashMap<String, Vec<&'a crate::lang::IndexedFunction>> = HashMap::new();
    for f in &bake.functions {
        by_name.entry(f.name.to_lowercase()).or_default().push(f);
    }

    let mut chain: Vec<TraceNode> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(&crate::lang::IndexedFunction, usize)> = VecDeque::new();

    visited.insert(format!("{}:{}", start.file, start.name));
    queue.push_back((start, 0));

    while let Some((func, d)) = queue.pop_front() {
        chain.push(TraceNode {
            name: func.name.clone(),
            file: func.file.clone(),
            start_line: func.start_line,
            depth: d,
            qualifier: None,
            boundary: None,
            resolved: true,
        });

        if d >= max_depth {
            continue;
        }

        for call in &func.calls {
            let cl = call.callee.to_lowercase();

            if let Some(b) = boundary_from_call(&call.callee, &call.qualifier) {
                let key = format!("boundary:{}:{}", b, call.callee);
                if !visited.contains(&key) {
                    visited.insert(key);
                    chain.push(TraceNode {
                        name: call.callee.clone(),
                        file: String::new(),
                        start_line: call.line,
                        depth: d + 1,
                        qualifier: call.qualifier.clone(),
                        boundary: Some(b),
                        resolved: false,
                    });
                }
                continue;
            }

            if let Some(candidates) = by_name.get(&cl) {
                if let Some(callee_fn) = resolve_candidate(candidates, func, &call.qualifier) {
                    let key = format!("{}:{}", callee_fn.file, callee_fn.name);
                    if !visited.contains(&key) {
                        visited.insert(key);
                        queue.push_back((callee_fn, d + 1));
                    }
                }
            } else {
                let label = match &call.qualifier {
                    Some(q) => format!("{}.{}", q, call.callee),
                    None => call.callee.clone(),
                };
                if !unresolved.contains(&label) && !is_stdlib_symbol(&call.callee, call.qualifier.as_deref()) {
                    unresolved.push(label);
                }
            }
        }
    }

    chain.sort_by_key(|n| n.depth);
    (chain, unresolved)
}

pub fn trace_down(
    path: Option<String>,
    symbol: String,
    depth: Option<usize>,
    file: Option<String>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = require_bake_index(&root)?;

    let max_depth = depth.unwrap_or(5);
    let file_filter = file.as_deref().map(str::to_lowercase);
    let needle = symbol.to_lowercase();

    let start = bake
        .functions
        .iter()
        .find(|f| {
            f.name.to_lowercase() == needle
                && file_filter
                    .as_ref()
                    .map(|ff| f.file.to_lowercase().contains(ff.as_str()))
                    .unwrap_or(true)
        })
        .ok_or_else(|| anyhow!("Symbol '{}' not found. Run `bake` first or check the name.", symbol))?;

    let lang = start.language.to_lowercase();
    if lang != "rust" && lang != "go" {
        return Ok(serde_json::to_string_pretty(&serde_json::json!({
            "tool": "trace_down",
            "supported": false,
            "language": start.language,
            "reason": "trace_down call-chain tracing is only supported for Rust and Go. Other languages do not emit enough call-graph data during bake.",
            "alternatives": [
                "supersearch with context=identifiers and pattern=call to find call sites manually",
                "symbol+include_source to read the function body and trace manually",
                "flow for endpoint-rooted tracing — handler is still returned even without chain"
            ]
        }))?);
    }

    let (chain, unresolved) = trace_chain(&bake, start, max_depth);

    let payload = TraceDownPayload {
        tool: "trace_down",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        symbol,
        chain,
        unresolved,
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

    // ── graph_rename ──────────────────────────────────────────────────────────

    #[test]
    fn rename_renames_identifier_in_source_file() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/lib.rs",
            "fn old_name() {}\nfn caller() { old_name(); }\n",
        );

        let result = graph_rename(
            Some(dir.path().to_string_lossy().into_owned()),
            "old_name".into(),
            "new_name".into(),
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["occurrences_renamed"], 2);

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn new_name()"));
        assert!(content.contains("new_name()"));
        assert!(!content.contains("old_name"));
    }

    #[test]
    fn rename_returns_error_for_identical_names() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn foo() {}\n");

        let err = graph_rename(
            Some(dir.path().to_string_lossy().into_owned()),
            "foo".into(),
            "foo".into(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("identical"));
    }

    #[test]
    fn rename_returns_error_when_symbol_not_found() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn foo() {}\n");

        let err = graph_rename(
            Some(dir.path().to_string_lossy().into_owned()),
            "nonexistent".into(),
            "whatever".into(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("No occurrences"));
    }

    // ── graph_add ─────────────────────────────────────────────────────────────

    #[test]
    fn add_appends_function_scaffold_to_file() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn existing() {}\n");
        bake_dir(&dir);

        let result = graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "fn".into(),
            "new_helper".into(),
            "src/lib.rs".into(),
            None,
            Some("rust".into()),
            None, None, None,
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "graph_add");

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("new_helper"));
    }

    // ── graph_create ──────────────────────────────────────────────────────────

    #[test]
    fn create_makes_new_file_with_scaffold() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        let result = graph_create(
            Some(dir.path().to_string_lossy().into_owned()),
            "src/new_module.rs".into(),
            "process_event".into(),
            Some("rust".into()),
            None, None,
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "graph_create");
        assert_eq!(v["function_name"], "process_event");
        assert_eq!(v["language"], "rust");

        let content = fs::read_to_string(dir.path().join("src/new_module.rs")).unwrap();
        assert!(content.contains("fn process_event"));
        assert!(content.contains("todo!()"));
    }

    #[test]
    fn create_errors_if_file_already_exists() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/existing.rs", "fn old() {}\n");

        let err = graph_create(
            Some(dir.path().to_string_lossy().into_owned()),
            "src/existing.rs".into(),
            "new_fn".into(),
            None, None, None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn create_errors_if_parent_dir_missing() {
        let dir = TempDir::new().unwrap();

        let err = graph_create(
            Some(dir.path().to_string_lossy().into_owned()),
            "nonexistent/dir/foo.rs".into(),
            "my_fn".into(),
            None, None, None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn create_python_scaffold() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        let result = graph_create(
            Some(dir.path().to_string_lossy().into_owned()),
            "src/handler.py".into(),
            "handle_request".into(),
            Some("python".into()),
            None, None,
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["language"], "python");

        let content = fs::read_to_string(dir.path().join("src/handler.py")).unwrap();
        assert!(content.contains("def handle_request"));
        assert!(content.contains("pass"));
    }

    #[test]
    fn create_typescript_scaffold() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        let result = graph_create(
            Some(dir.path().to_string_lossy().into_owned()),
            "src/service.ts".into(),
            "fetchUser".into(),
            Some("typescript".into()),
            None, None,
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["language"], "typescript");

        let content = fs::read_to_string(dir.path().join("src/service.ts")).unwrap();
        assert!(content.contains("function fetchUser"));
    }

    #[test]
    fn create_detects_language_from_extension() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        graph_create(
            Some(dir.path().to_string_lossy().into_owned()),
            "src/utils.go".into(),
            "parseArgs".into(),
            None, None, None,
        )
        .unwrap();

        let content = fs::read_to_string(dir.path().join("src/utils.go")).unwrap();
        assert!(content.contains("func parseArgs"));
    }

    // ── test scaffold ─────────────────────────────────────────────────────────

    #[test]
    fn test_scaffold_rust_generates_test_fn() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "test".into(), "add".into(), "src/lib.rs".into(),
            None, Some("rust".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("#[test]"), "#[test] missing:\n{}", content);
        assert!(content.contains("fn test_add()"), "test fn name missing:\n{}", content);
        assert!(content.contains("todo!()"), "todo!() missing:\n{}", content);
    }

    #[test]
    fn test_scaffold_go_generates_test_fn() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.go", "package lib\nfunc Add(a, b int) int { return a + b }\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "test".into(), "Add".into(), "src/lib.go".into(),
            None, Some("go".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.go")).unwrap();
        assert!(content.contains("func TestAdd(t *testing.T)"), "Go test fn missing:\n{}", content);
    }

    #[test]
    fn test_scaffold_typescript_generates_it_block() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/math.ts", "export function add(a: number, b: number): number { return a + b; }\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "test".into(), "add".into(), "src/math.ts".into(),
            None, Some("typescript".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/math.ts")).unwrap();
        assert!(content.contains("it(\"add\""), "it() block missing:\n{}", content);
    }

    #[test]
    fn test_scaffold_zig_generates_test_block() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.zig", "pub fn add(a: i32, b: i32) i32 { return a + b; }\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "test".into(), "add".into(), "src/lib.zig".into(),
            None, Some("zig".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.zig")).unwrap();
        assert!(content.contains("test \"add\""), "Zig test block missing:\n{}", content);
    }

    #[test]
    fn test_scaffold_python_generates_test_fn() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/math.py", "def add(a, b):\n    return a + b\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "test".into(), "add".into(), "src/math.py".into(),
            None, Some("python".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/math.py")).unwrap();
        assert!(content.contains("def test_add()"), "Python test fn missing:\n{}", content);
        assert!(content.contains("pass"), "pass missing:\n{}", content);
    }

    // ── typed scaffold ────────────────────────────────────────────────────────

    fn params(list: &[(&str, &str)]) -> Vec<Param> {
        list.iter().map(|(n, t)| Param { name: n.to_string(), type_str: t.to_string() }).collect()
    }

    #[test]
    fn typed_scaffold_rust_function_with_params_and_return() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn existing() {}\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "fn".into(), "parse_config".into(), "src/lib.rs".into(),
            None, Some("rust".into()),
            Some(params(&[("input", "&str"), ("timeout", "u32")])),
            Some("Result<Config, Error>".into()),
            None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn parse_config(input: &str, timeout: u32)"), "params missing: {}", content);
        assert!(content.contains("-> Result<Config, Error>"), "return type missing: {}", content);
        assert!(content.contains("todo!()"), "body missing: {}", content);
    }

    #[test]
    fn typed_scaffold_rust_method_wraps_in_impl_block() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "struct Config;\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "fn".into(), "load".into(), "src/lib.rs".into(),
            None, Some("rust".into()),
            Some(params(&[("path", "&str")])),
            Some("Result<Self, Error>".into()),
            Some("Config".into()),
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("impl Config"), "impl block missing: {}", content);
        assert!(content.contains("fn load(path: &str)"), "method sig missing: {}", content);
        assert!(content.contains("-> Result<Self, Error>"), "return type missing: {}", content);
    }

    #[test]
    fn typed_scaffold_go_function_with_params_and_return() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.go", "package lib\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "func".into(), "ParseConfig".into(), "src/lib.go".into(),
            None, Some("go".into()),
            Some(params(&[("input", "string"), ("timeout", "int")])),
            Some("(Config, error)".into()),
            None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.go")).unwrap();
        assert!(content.contains("func ParseConfig(input string, timeout int)"), "sig missing: {}", content);
        assert!(content.contains("(Config, error)"), "return type missing: {}", content);
    }

    #[test]
    fn typed_scaffold_typescript_function_with_params_and_return() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/service.ts", "export {};\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "function".into(), "fetchUser".into(), "src/service.ts".into(),
            None, Some("typescript".into()),
            Some(params(&[("id", "number"), ("opts", "RequestOptions")])),
            Some("Promise<User>".into()),
            None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/service.ts")).unwrap();
        assert!(content.contains("function fetchUser(id: number, opts: RequestOptions)"), "sig missing: {}", content);
        assert!(content.contains(": Promise<User>"), "return type missing: {}", content);
    }

    #[test]
    fn typed_scaffold_no_spec_behaves_as_before() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn existing() {}\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "fn".into(), "simple".into(), "src/lib.rs".into(),
            None, Some("rust".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn simple()"), "bare scaffold missing: {}", content);
    }

    // ── graph_move ────────────────────────────────────────────────────────────

    #[test]
    fn move_relocates_function_body_to_destination() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/a.rs",
            "fn helper() {\n    let x = 1;\n}\n",
        );
        write_file(&dir, "src/b.rs", "fn other() {}\n");
        bake_dir(&dir);

        let result = graph_move(
            Some(dir.path().to_string_lossy().into_owned()),
            "helper".into(),
            "src/b.rs".into(),
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "graph_move");
        assert_eq!(v["from_file"], "src/a.rs");
        assert_eq!(v["to_file"], "src/b.rs");

        let src_content = fs::read_to_string(dir.path().join("src/a.rs")).unwrap();
        assert!(!src_content.contains("fn helper"));

        let dst_content = fs::read_to_string(dir.path().join("src/b.rs")).unwrap();
        assert!(dst_content.contains("fn helper"));
    }

    #[test]
    fn move_errors_when_source_and_dest_are_same() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/a.rs", "fn foo() {}\n");
        bake_dir(&dir);

        let err = graph_move(
            Some(dir.path().to_string_lossy().into_owned()),
            "foo".into(),
            "src/a.rs".into(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("same"));
    }

    // ── resolve_candidate ─────────────────────────────────────────────────────

    fn make_fn(name: &str, file: &str, lang: &str, module_path: &str) -> crate::lang::IndexedFunction {
        crate::lang::IndexedFunction {
            name: name.to_string(),
            file: file.to_string(),
            language: lang.to_string(),
            start_line: 1,
            end_line: 5,
            complexity: 1,
            calls: vec![],
            byte_start: 0,
            byte_end: 0,
            module_path: module_path.to_string(),
            qualified_name: if module_path.is_empty() {
                name.to_string()
            } else {
                let sep = if lang == "rust" { "::" } else if lang == "python" { "." } else { "/" };
                format!("{}{}{}", module_path, sep, name)
            },
            visibility: crate::lang::Visibility::Public,
            parent_type: None,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_candidate_exact_qualified_name_rust() {
        let a = make_fn("process", "src/engine/core.rs", "rust", "engine");
        let b = make_fn("process", "src/api/handler.rs", "rust", "api");
        let caller = make_fn("run", "src/main.rs", "rust", "");
        let candidates = vec![&a, &b];
        // qualifier "engine" matches module_path "engine" exactly
        let result = resolve_candidate(&candidates, &caller, &Some("engine".to_string()));
        assert_eq!(result.unwrap().file, "src/engine/core.rs");
    }

    #[test]
    fn resolve_candidate_crate_prefix_rust() {
        let a = make_fn("process", "src/engine/core.rs", "rust", "engine");
        let b = make_fn("process", "src/api/handler.rs", "rust", "api");
        let caller = make_fn("run", "src/main.rs", "rust", "");
        let candidates = vec![&a, &b];
        // qualifier "crate::engine" ends with module_path "engine"
        let result = resolve_candidate(&candidates, &caller, &Some("crate::engine".to_string()));
        assert_eq!(result.unwrap().file, "src/engine/core.rs");
    }

    #[test]
    fn resolve_candidate_go_package_exact() {
        let a = make_fn("Handle", "cmd/server/handler.go", "go", "server");
        let b = make_fn("Handle", "internal/client/handler.go", "go", "client");
        let caller = make_fn("main", "cmd/main.go", "go", "main");
        let candidates = vec![&a, &b];
        // qualifier "server" matches module_path "server" exactly
        let result = resolve_candidate(&candidates, &caller, &Some("server".to_string()));
        assert_eq!(result.unwrap().file, "cmd/server/handler.go");
    }

    #[test]
    fn resolve_candidate_sep_qualified_name_rust() {
        let a = make_fn("parse", "src/engine/parse.rs", "rust", "engine");
        let b = make_fn("parse", "src/cli/parse.rs", "rust", "cli");
        let caller = make_fn("run", "src/main.rs", "rust", "");
        let candidates = vec![&a, &b];
        // qualifier "engine" + "::" + "parse" == qualified_name "engine::parse"
        let result = resolve_candidate(&candidates, &caller, &Some("engine".to_string()));
        assert_eq!(result.unwrap().file, "src/engine/parse.rs");
    }

    #[test]
    fn resolve_candidate_trivial_receiver_falls_through() {
        let a = make_fn("get", "src/engine/store.rs", "rust", "engine");
        let b = make_fn("get", "src/api/store.rs", "rust", "api");
        let caller = make_fn("run", "src/engine/core.rs", "rust", "engine");
        let candidates = vec![&a, &b];
        // "self" is trivial — skip qualifier matching, fall to same-directory
        let result = resolve_candidate(&candidates, &caller, &Some("self".to_string()));
        assert_eq!(result.unwrap().file, "src/engine/store.rs");
    }

    // ── compiler guardrail rejection tests ───────────────────────────────────

    #[test]
    fn graph_rename_succeeds_and_updates_all_occurrences() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn foo() {}\nfn caller() { foo(); }\n");
        bake_dir(&dir);

        graph_rename(
            Some(dir.path().to_string_lossy().into_owned()),
            "foo".into(),
            "bar".into(),
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn bar()"));
        assert!(content.contains("bar()"));
        assert!(!content.contains("foo"));
    }

    #[test]
    fn graph_add_rejects_when_scaffold_is_invalid_for_language() {
        let dir = TempDir::new().unwrap();
        // entity_type "function" generates JS syntax — invalid in a .rs file.
        write_file(&dir, "src/lib.rs", "fn existing() {}\n");
        bake_dir(&dir);

        let err = graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "function".into(), // JS entity_type in a Rust file
            "new_fn".into(),
            "src/lib.rs".into(),
            None,
            Some("rust".into()),
            None, None, None,
        ).unwrap_err();

        assert!(err.to_string().contains("graph_add rejected"), "got: {}", err);
        // File must be untouched.
        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(!content.contains("new_fn"), "file was modified despite rejection");
    }

    #[test]
    fn graph_add_allows_valid_rust_fn_scaffold() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn existing() {}\n");
        bake_dir(&dir);

        graph_add(
            Some(dir.path().to_string_lossy().into_owned()),
            "fn".into(),
            "new_helper".into(),
            "src/lib.rs".into(),
            None,
            Some("rust".into()),
            None, None, None,
        ).unwrap();

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn new_helper()"));
    }

    #[test]
    fn graph_move_rejects_when_dest_has_invalid_syntax_and_leaves_both_files_untouched() {
        let dir = TempDir::new().unwrap();
        // Destination already has broken syntax — moving into it should be rejected.
        write_file(&dir, "src/a.rs", "fn foo() {}\n");
        write_file(&dir, "src/b.rs", "fn bar( {}\n"); // pre-broken
        bake_dir(&dir);

        let err = graph_move(
            Some(dir.path().to_string_lossy().into_owned()),
            "foo".into(),
            "src/b.rs".into(),
        ).unwrap_err();

        assert!(err.to_string().contains("graph_move rejected"), "got: {}", err);
        // Both files must be untouched — checks happen before any write.
        let src = fs::read_to_string(dir.path().join("src/a.rs")).unwrap();
        let dst = fs::read_to_string(dir.path().join("src/b.rs")).unwrap();
        assert!(src.contains("fn foo()"), "source was modified despite rejection");
        assert_eq!(dst, "fn bar( {}\n", "dest was modified despite rejection");
    }
}
