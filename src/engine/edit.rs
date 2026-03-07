use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use std::collections::HashMap;

use super::types::{MultiPatchPayload, PatchBytesPayload, PatchPayload, SlicePayload, SyntaxError};
use super::util::{load_bake_index, reindex_files, resolve_project_root};

// ── Post-patch syntax validation ──────────────────────────────────────────────

/// Parse `file` with tree-sitter and return any ERROR/MISSING nodes.
/// Returns an empty vec if the language is unsupported or the file can't be read.
fn syntax_check(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    let full_path = root.join(file);
    let Ok(source) = fs::read_to_string(&full_path) else { return vec![] };

    use ast_grep_language::{LanguageExt, SupportLang};
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut parser = tree_sitter::Parser::new();
    let ok = match ext {
        "rs"                        => parser.set_language(&SupportLang::Rust.get_ts_language()),
        "go"                        => parser.set_language(&SupportLang::Go.get_ts_language()),
        "py"                        => parser.set_language(&SupportLang::Python.get_ts_language()),
        "ts" | "tsx" | "js" | "jsx" => parser.set_language(&SupportLang::TypeScript.get_ts_language()),
        _                           => return vec![],
    };
    if ok.is_err() { return vec![]; }

    let Some(tree) = parser.parse(&source, None) else { return vec![] };
    let mut errors = vec![];
    collect_errors(tree.root_node(), &source, &mut errors);
    // For each supported language, run the compiler/checker to catch errors
    // that tree-sitter cannot see (macros, type errors, import issues, etc.).
    match ext {
        "rs"                        => errors.extend(cargo_check_errors(root, file)),
        "go"                        => errors.extend(go_build_errors(root, file)),
        "py"                        => errors.extend(python_compile_errors(root, file)),
        "ts" | "tsx" | "js" | "jsx" => errors.extend(tsc_errors(root, file)),
        _ => {}
    }

    errors
}


/// Run `cargo check --message-format=json` in `root` and return compiler errors
/// that mention `file`. Best-effort: returns empty vec on any failure.
fn cargo_check_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("cargo")
        .args(["check", "--message-format=json", "--quiet"])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors = vec![];

    // Normalise the target file path for comparison.
    let file_norm = file.replace('\\', "/");

    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") { continue; }

        let message = &msg["message"];
        if message.get("level").and_then(|l| l.as_str()) != Some("error") { continue; }

        let Some(spans) = message.get("spans").and_then(|s| s.as_array()) else { continue };

        for span in spans {
            let span_file = span.get("file_name").and_then(|f| f.as_str()).unwrap_or("");
            let span_norm = span_file.replace('\\', "/");
            // Match if either path ends with the other (handles relative vs absolute).
            if !span_norm.ends_with(&file_norm) && !file_norm.ends_with(&span_norm) { continue; }

            let line_num = span.get("line_start").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
            let raw = message.get("message").and_then(|m| m.as_str()).unwrap_or("");
            let text: String = raw.chars().take(120).collect();
            errors.push(SyntaxError { line: line_num, kind: "cargo".to_string(), text });
        }
    }

    errors
}

/// Run `go build ./...` and return errors mentioning `file`. Best-effort.
fn go_build_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("go")
        .args(["build", "./..."])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };

    // go build writes errors to stderr in the form: file.go:line:col: message
    let stderr = String::from_utf8_lossy(&output.stderr);
    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file);

    let mut errors = vec![];
    for line in stderr.lines() {
        if !line.contains(file_name) { continue; }
        // Format: path/to/file.go:LINE:COL: message
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() < 4 { continue; }
        let line_num = parts[1].trim().parse::<u32>().unwrap_or(0);
        let text: String = parts[3].trim().chars().take(120).collect();
        errors.push(SyntaxError { line: line_num, kind: "go".to_string(), text });
    }
    errors
}

/// Run `python -m py_compile <file>` and return syntax errors. Best-effort.
fn python_compile_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("python3")
        .args(["-m", "py_compile", file])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() { return vec![]; }

    // py_compile writes to stderr: File "path", line N\n  SyntaxError: msg
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut line_num = 0u32;
    let mut errors = vec![];

    for ln in stderr.lines() {
        if let Some(rest) = ln.trim().strip_prefix("File ") {
            // File "path", line N
            if let Some(idx) = rest.rfind(", line ") {
                line_num = rest[idx + 7..].trim().parse().unwrap_or(0);
            }
        } else if ln.trim().starts_with("SyntaxError:") || ln.trim().starts_with("IndentationError:") {
            let text: String = ln.trim().chars().take(120).collect();
            errors.push(SyntaxError { line: line_num, kind: "python".to_string(), text });
        }
    }
    errors
}

/// Run `tsc --noEmit` and return errors mentioning `file`. Best-effort.
/// Requires `tsc` to be available (via npx or global install).
fn tsc_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    // Try npx tsc first, fall back to tsc directly.
    let output = Command::new("npx")
        .args(["--no-install", "tsc", "--noEmit", "--pretty", "false"])
        .current_dir(root)
        .output()
        .or_else(|_| {
            Command::new("tsc")
                .args(["--noEmit", "--pretty", "false"])
                .current_dir(root)
                .output()
        });

    let Ok(output) = output else { return vec![] };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file);

    let mut errors = vec![];
    for ln in combined.lines() {
        if !ln.contains(file_name) { continue; }
        // Format: path/file.ts(LINE,COL): error TS####: message
        if let Some(paren) = ln.find('(') {
            let rest = &ln[paren + 1..];
            if let Some(comma) = rest.find(',') {
                let line_num = rest[..comma].parse::<u32>().unwrap_or(0);
                let text = ln.split(": ").skip(2).collect::<Vec<_>>().join(": ");
                let text: String = text.chars().take(120).collect();
                errors.push(SyntaxError { line: line_num, kind: "tsc".to_string(), text });
            }
        }
    }
    errors
}

fn collect_errors(node: tree_sitter::Node, source: &str, errors: &mut Vec<SyntaxError>) {
    if node.is_error() || node.is_missing() {
        let line = node.start_position().row as u32 + 1;
        let raw  = node.utf8_text(source.as_bytes()).unwrap_or("").trim();
        let text: String = raw.chars().take(80).collect();
        let kind = if node.is_missing() { "missing" } else { "error" }.to_string();
        errors.push(SyntaxError { line, kind, text });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, source, errors);
    }
}

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
    let syntax_errors = syntax_check(&root, &file);
    let payload = PatchPayload {
        tool: "patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end,
        total_lines,
        syntax_errors,
    };
    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Content-match patch: find `old_string` in `file`, replace with `new_string`.
/// Immune to line number drift — works by content, not position.
pub fn patch_string(
    path: Option<String>,
    file: String,
    old_string: String,
    new_string: String,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let full_path = root.join(&file);
    let content = fs::read_to_string(&full_path)
        .with_context(|| format!("Failed to read {}", file))?;

    let pos = content
        .find(&old_string)
        .ok_or_else(|| anyhow!("old_string not found in {}. Check exact whitespace and content.", file))?;

    let new_content = format!(
        "{}{}{}",
        &content[..pos],
        new_string,
        &content[pos + old_string.len()..]
    );

    let start_line = (content[..pos].lines().count() + 1) as u32;
    let end_line = start_line + old_string.lines().count().saturating_sub(1) as u32;
    let total_lines = new_content.lines().count() as u32;

    fs::write(&full_path, &new_content)
        .with_context(|| format!("Failed to write {}", file))?;

    let _ = reindex_files(&root, &[file.as_str()]);
    let syntax_errors = syntax_check(&root, &file);

    let payload = PatchPayload {
        tool: "patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start: start_line,
        end: end_line,
        total_lines,
        syntax_errors,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
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
    let syntax_errors = syntax_check(&root, &file);
    let payload = PatchPayload {
        tool: "patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        start,
        end,
        total_lines,
        syntax_errors,
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
    let syntax_errors = syntax_check(&root, &file);
    let payload = PatchBytesPayload {
        tool: "patch_bytes",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file,
        byte_start,
        byte_end,
        new_bytes: new_byte_count,
        syntax_errors,
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
    let syntax_errors: Vec<SyntaxError> = files_for_reindex
        .iter()
        .flat_map(|f| syntax_check(&root, f))
        .collect();
    let payload = MultiPatchPayload {
        tool: "multi_patch",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        files_written,
        edits_applied: total_edits,
        syntax_errors,
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
