use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use std::collections::HashMap;

use super::types::{MultiPatchPayload, PatchBytesPayload, PatchPayload, SlicePayload};
use super::util::{load_bake_index, reindex_files, resolve_project_root};

/// Public entrypoint for the `slice` tool: read a specific line range of a file.
pub fn slice(
    path: Option<String>,
    file: String,
    start: u32,
    end: u32,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    if start == 0 || end == 0 || end < start {
        return Err(anyhow!(
            "Invalid range: start and end must be >= 1 and end >= start (got start={}, end={})",
            start,
            end
        ));
    }

    let full_path = root.join(&file);
    let content = fs::read_to_string(&full_path).with_context(|| {
        format!(
            "Failed to read file {} (resolved to {})",
            file,
            full_path.display()
        )
    })?;

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len() as u32;

    let s = start.saturating_sub(1) as usize;
    let e = end.min(total_lines).saturating_sub(1) as usize;

    if s >= all_lines.len() {
        return Err(anyhow!(
            "Start line {} is beyond end of file (total_lines={})",
            start,
            total_lines
        ));
    }

    let mut lines = Vec::new();
    for i in s..=e {
        lines.push(all_lines[i].to_string());
    }

    let payload = SlicePayload {
        tool: "slice",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end: end.min(total_lines),
        total_lines,
        lines,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `patch` tool (by file and line range).
pub fn patch(
    path: Option<String>,
    file: String,
    start: u32,
    end: u32,
    new_content: String,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let (file, start, end, total_lines) =
        apply_patch_to_range(&root, &file, start, end, &new_content)?;
    let _ = reindex_files(&root, &[file.as_str()]);
    let payload = PatchPayload {
        tool: "patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end,
        total_lines,
    };
    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `patch` tool (by symbol name). Resolves the symbol from the bake
/// index, then replaces its line range with `new_content`. Use `match_index` (0-based) when
/// multiple symbols match the name; default 0.
pub fn patch_by_symbol(
    path: Option<String>,
    name: String,
    new_content: String,
    match_index: Option<usize>,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake = load_bake_index(&root)?
        .ok_or_else(|| anyhow!("No bake index found. Run `bake` first to build bakes/latest/bake.json."))?;

    let needle = name.to_lowercase();

    // Collect matching functions as (file, start_line, end_line, exact_match, complexity).
    let mut matches: Vec<(String, u32, u32, bool, u32)> = bake
        .functions
        .iter()
        .filter_map(|f| {
            let fname = f.name.to_lowercase();
            if fname == needle || fname.contains(&needle) {
                Some((f.file.clone(), f.start_line, f.end_line, fname == needle, f.complexity))
            } else {
                None
            }
        })
        .collect();

    // Same order as symbol: exact match first, then higher complexity, then file path.
    matches.sort_by(|a, b| {
        (b.3 as i32)
            .cmp(&(a.3 as i32))
            .then_with(|| b.4.cmp(&a.4))
            .then(a.0.cmp(&b.0))
    });

    if matches.is_empty() {
        return Err(anyhow!("No symbol match for name {:?}. Run `bake` and ensure the symbol exists.", name));
    }

    let idx = match_index.unwrap_or(0);
    if idx >= matches.len() {
        return Err(anyhow!(
            "match_index {} out of range ({} match(es) for {:?})",
            idx,
            matches.len(),
            name
        ));
    }

    let (file, start, end, _, _) = &matches[idx];
    let (file, start, end, total_lines) =
        apply_patch_to_range(&root, file.as_str(), *start, *end, &new_content)?;
    let _ = reindex_files(&root, &[file.as_str()]);
    let payload = PatchPayload {
        tool: "patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end,
        total_lines,
    };
    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

// ── Byte-level patch ─────────────────────────────────────────────────────────

/// Public entrypoint for `patch_bytes`: splice at exact byte offsets.
pub fn patch_bytes(
    path: Option<String>,
    file: String,
    byte_start: usize,
    byte_end: usize,
    new_content: String,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let full_path = root.join(&file);
    let mut bytes = fs::read(&full_path).with_context(|| {
        format!("Failed to read file {} (resolved to {})", file, full_path.display())
    })?;
    let file_len = bytes.len();
    if byte_start > byte_end || byte_end > file_len {
        return Err(anyhow!(
            "Invalid byte range: byte_start={} byte_end={} file_len={}",
            byte_start,
            byte_end,
            file_len
        ));
    }
    let new_bytes = new_content.as_bytes();
    let new_byte_count = new_bytes.len();
    bytes.splice(byte_start..byte_end, new_bytes.iter().copied());
    fs::write(&full_path, &bytes).with_context(|| {
        format!("Failed to write patched file {} (resolved to {})", file, full_path.display())
    })?;
    let _ = reindex_files(&root, &[file.as_str()]);
    let payload = PatchBytesPayload {
        tool: "patch_bytes",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        byte_start,
        byte_end,
        new_bytes: new_byte_count,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

// ── Multi-patch ───────────────────────────────────────────────────────────────

pub struct PatchEdit {
    pub file: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub new_content: String,
}

/// Public entrypoint for `multi_patch`: apply N byte-level edits across M files atomically.
/// Edits within each file are applied bottom-up (descending byte_start) so earlier offsets
/// are not shifted by later replacements. Each file is written exactly once.
pub fn multi_patch(path: Option<String>, edits: Vec<PatchEdit>) -> Result<String> {
    let root = resolve_project_root(path)?;

    // Group edits by file.
    let mut by_file: HashMap<String, Vec<PatchEdit>> = HashMap::new();
    let total_edits = edits.len();
    for edit in edits {
        by_file.entry(edit.file.clone()).or_default().push(edit);
    }

    let files_written = by_file.len();
    let files_for_reindex: Vec<String> = by_file.keys().cloned().collect();

    for (file, mut file_edits) in by_file {
        let full_path = root.join(&file);
        let mut bytes = fs::read(&full_path).with_context(|| {
            format!("Failed to read file {} (resolved to {})", file, full_path.display())
        })?;
        let file_len = bytes.len();

        // Validate ranges.
        for e in &file_edits {
            if e.byte_start > e.byte_end || e.byte_end > file_len {
                return Err(anyhow!(
                    "Invalid byte range in {}: byte_start={} byte_end={} file_len={}",
                    file,
                    e.byte_start,
                    e.byte_end,
                    file_len
                ));
            }
        }

        // Sort descending by byte_start (bottom-up) to preserve offsets.
        file_edits.sort_by(|a, b| b.byte_start.cmp(&a.byte_start));

        // Check for overlaps (after sorting).
        for i in 1..file_edits.len() {
            if file_edits[i - 1].byte_start < file_edits[i].byte_end {
                return Err(anyhow!(
                    "Overlapping edits in {}: [{}, {}) overlaps [{}, {})",
                    file,
                    file_edits[i].byte_start,
                    file_edits[i].byte_end,
                    file_edits[i - 1].byte_start,
                    file_edits[i - 1].byte_end
                ));
            }
        }

        // Apply edits bottom-up.
        for edit in &file_edits {
            bytes.splice(edit.byte_start..edit.byte_end, edit.new_content.as_bytes().iter().copied());
        }

        fs::write(&full_path, &bytes).with_context(|| {
            format!("Failed to write patched file {} (resolved to {})", file, full_path.display())
        })?;
    }

    let refs: Vec<&str> = files_for_reindex.iter().map(|s| s.as_str()).collect();
    let _ = reindex_files(&root, &refs);

    let payload = MultiPatchPayload {
        tool: "multi_patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        files_written,
        edits_applied: total_edits,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Apply a line-range replacement in a file. Returns (file, start, end, total_lines) for the payload.
fn apply_patch_to_range(
    root: &PathBuf,
    file: &str,
    start: u32,
    end: u32,
    new_content: &str,
) -> Result<(String, u32, u32, u32)> {
    if start == 0 || end == 0 || end < start {
        return Err(anyhow!(
            "Invalid range: start and end must be >= 1 and end >= start (got start={}, end={})",
            start,
            end
        ));
    }

    let full_path = root.join(file);
    let content = fs::read_to_string(&full_path).with_context(|| {
        format!(
            "Failed to read file {} (resolved to {})",
            file,
            full_path.display()
        )
    })?;

    let had_trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let total_lines = lines.len() as u32;

    let s = start.saturating_sub(1) as usize;
    let e = end.min(total_lines).saturating_sub(1) as usize;

    if s >= lines.len() {
        return Err(anyhow!(
            "Start line {} is beyond end of file (total_lines={})",
            start,
            total_lines
        ));
    }

    let replacement_lines: Vec<String> = new_content.lines().map(|s| s.to_string()).collect();
    lines.splice(s..=e, replacement_lines.into_iter());

    let mut new_text = lines.join("\n");
    if had_trailing_newline {
        new_text.push('\n');
    }
    fs::write(&full_path, new_text).with_context(|| {
        format!(
            "Failed to write patched file {} (resolved to {})",
            file,
            full_path.display()
        )
    })?;

    Ok((file.to_string(), start, end, total_lines))
}
