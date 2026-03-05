use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use super::types::{GraphAddPayload, GraphMovePayload, GraphRenamePayload};
use super::util::{detect_language, load_bake_index, reindex_files, resolve_project_root};

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

    let source_files = collect_source_files(&root);

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
        files_changed,
        occurrences_renamed: total_occurrences,
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
        let bake = load_bake_index(&root)?
            .ok_or_else(|| anyhow!("No bake index. Run `bake` first."))?;
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

    let scaffold = generate_scaffold(&entity_type, &name, lang);
    let scaffold_bytes = scaffold.as_bytes();

    let mut bytes = if full_path.exists() {
        fs::read(&full_path).with_context(|| format!("Failed to read {}", file))?
    } else {
        Vec::new()
    };
    let insert_at = insert_at.min(bytes.len());
    bytes.splice(insert_at..insert_at, scaffold_bytes.iter().copied());
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

// ── graph_move ───────────────────────────────────────────────────────────────

/// Move a function from one file to another.
pub fn graph_move(
    path: Option<String>,
    name: String,
    to_file: String,
) -> Result<String> {
    let root = resolve_project_root(path)?;

    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index. Run `bake` first."))?;

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

    // Remove from source.
    src_bytes.drain(byte_start..byte_end);
    fs::write(&src_full, &src_bytes)
        .with_context(|| format!("Failed to write source {}", from_file))?;

    // Append to destination.
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

    fs::write(&dst_full, &dst_bytes)
        .with_context(|| format!("Failed to write dest {}", to_file))?;

    let _ = reindex_files(&root, &[from_file.as_str(), to_file.as_str()]);

    let payload = GraphMovePayload {
        tool: "graph_move",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        name,
        from_file,
        to_file,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}
