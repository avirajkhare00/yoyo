use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use super::types::{PatchPayload, SlicePayload};
use super::util::{load_bake_index, resolve_project_root};

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
