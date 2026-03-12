use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use std::collections::HashMap;

use super::types::{MultiPatchPayload, PatchBytesPayload, PatchPayload, SlicePayload, SyntaxError};
use super::util::{reindex_files, require_bake_index, resolve_project_root};

// ── Pre-write in-memory AST validation ────────────────────────────────────────

/// Parse `source` with tree-sitter (no file I/O) and return any ERROR/MISSING nodes.
/// Returns an empty vec if the extension is unsupported.
pub(super) fn ast_check_str(source: &str, ext: &str) -> Vec<SyntaxError> {
    use ast_grep_language::{LanguageExt, SupportLang};
    // Zig: tree-sitter-zig grammar lags behind valid constructs (error unions, nested
    // slice types, builtins). Delegate to `zig fmt --stdin` which is the canonical
    // validator. Falls back to tree-sitter only when `zig` is not on PATH.
    if ext == "zig" {
        return zig_ast_check(source);
    }
    let mut parser = tree_sitter::Parser::new();
    let ok = match ext {
        "rs"                        => parser.set_language(&SupportLang::Rust.get_ts_language()),
        "go"                        => parser.set_language(&SupportLang::Go.get_ts_language()),
        "py"                        => parser.set_language(&SupportLang::Python.get_ts_language()),
        "ts" | "tsx" | "js" | "jsx" => parser.set_language(&SupportLang::TypeScript.get_ts_language()),
        _                           => return vec![],
    };
    if ok.is_err() { return vec![]; }
    let Some(tree) = parser.parse(source, None) else { return vec![] };
    let mut errors = vec![];
    collect_errors(tree.root_node(), source, &mut errors);
    errors
}

/// Validate Zig source by piping it through `zig fmt --stdin`.
/// Returns errors parsed from stderr. Returns empty vec if `zig` is not on PATH.
fn zig_ast_check(source: &str) -> Vec<SyntaxError> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = match Command::new("zig")
        .args(["fmt", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return vec![], // zig not on PATH — skip check
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(source.as_bytes());
    }
    let Ok(output) = child.wait_with_output() else {
        return vec![];
    };
    if output.status.success() {
        return vec![];
    }
    // stderr format: <stdin>:LINE:COL: error: MESSAGE
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.lines().filter_map(|line| {
        let after = line.strip_prefix("<stdin>:")?;
        let mut parts = after.splitn(3, ':');
        let line_num: u32 = parts.next()?.parse().ok()?;
        let _col = parts.next();
        let rest = parts.next()?.trim();
        let text = rest.strip_prefix("error: ").unwrap_or(rest)
            .chars().take(80).collect();
        Some(SyntaxError { line: line_num, kind: "error".to_string(), text })
    }).collect()
}

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

// zig ast-check performs full semantic analysis (type checking, undefined symbols)
// on a single file without requiring a complete build system setup.
fn zig_check_errors(file: &std::path::Path) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("zig")
        .args(["ast-check", file.to_str().unwrap_or("")])
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() { return vec![]; }

    // zig ast-check writes to stderr: path:line:col: error: message
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut errors = vec![];
    for line in stderr.lines() {
        if !line.contains("error:") { continue; }
        let parts: Vec<&str> = line.splitn(5, ':').collect();
        if parts.len() < 5 { continue; }
        let line_num = parts[1].trim().parse::<u32>().unwrap_or(0);
        let text: String = parts[4].trim().chars().take(120).collect();
        errors.push(SyntaxError { line: line_num, kind: "zig".to_string(), text });
    }
    errors
}

// Write new_text to full_path, run the compiler, restore original if errors.
// Guarantees: if this returns Ok, the file on disk compiles cleanly.
fn write_with_compiler_guard(
    root: &PathBuf,
    full_path: &std::path::Path,
    file: &str,
    new_text: &str,
    ext: &str,
) -> Result<()> {
    let original = fs::read_to_string(full_path).unwrap_or_default();

    fs::write(full_path, new_text)
        .with_context(|| format!("Failed to write {}", file))?;

    let errors: Vec<SyntaxError> = match ext {
        "rs"  => cargo_check_errors(root, file),
        "go"  => go_build_errors(root, file),
        "zig" => zig_check_errors(full_path),
        _     => vec![],
    };

    if !errors.is_empty() {
        // Restore the original — file must not be left corrupted.
        let _ = fs::write(full_path, original);
        let summary: Vec<String> = errors.iter()
            .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
            .collect();
        return Err(anyhow!(
            "patch rejected: compiler errors (file restored to original):\n{}",
            summary.join("\n")
        ));
    }

    Ok(())
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
        patched_source: Some(new_content),
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

    // Pre-write AST check.
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let pre_errors = ast_check_str(&new_content, ext);
    if !pre_errors.is_empty() {
        let summary: Vec<String> = pre_errors.iter()
            .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
            .collect();
        return Err(anyhow!(
            "patch rejected: syntax errors in new content (file not modified):\n{}",
            summary.join("\n")
        ));
    }

    write_with_compiler_guard(&root, &full_path, &file, &new_content, ext)?;

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
        patched_source: Some(new_string),
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
    let bake = require_bake_index(&root)?;

    let needle = name.to_lowercase();

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
        patched_source: Some(new_content),
        syntax_errors,
    };
    let json = serde_json::to_string_pretty(&payload)?;
    Ok(json)
}

/// Public entrypoint for the `change` tool: one write surface that routes to existing
/// structural write primitives.
#[allow(clippy::too_many_arguments)]
pub fn change(
    path: Option<String>,
    action: String,
    name: Option<String>,
    file: Option<String>,
    start_line: Option<u32>,
    end_line: Option<u32>,
    new_content: Option<String>,
    old_string: Option<String>,
    new_string: Option<String>,
    match_index: Option<usize>,
    edits: Option<Vec<PatchEdit>>,
    new_name: Option<String>,
    to_file: Option<String>,
    force: Option<bool>,
    function_name: Option<String>,
    entity_type: Option<String>,
    after_symbol: Option<String>,
    language: Option<String>,
    params: Option<Vec<crate::engine::Param>>,
    returns: Option<String>,
    on: Option<String>,
) -> Result<String> {
    let action = action.trim().to_ascii_lowercase();
    let result = match action.as_str() {
        "edit" => {
            if let (Some(name), Some(new_content)) = (name.clone(), new_content.clone()) {
                crate::engine::patch_by_symbol(path, name, new_content, match_index)?
            } else if let (Some(file), Some(old_string), Some(new_string)) =
                (file.clone(), old_string, new_string)
            {
                crate::engine::patch_string(path, file, old_string, new_string)?
            } else if let (Some(file), Some(start_line), Some(end_line), Some(new_content)) =
                (file.clone(), start_line, end_line, new_content.clone())
            {
                crate::engine::patch(path, file, start_line, end_line, new_content)?
            } else {
                return Err(anyhow!(
                    "change action=edit requires either name+new_content, file+old_string+new_string, or file+start_line+end_line+new_content"
                ));
            }
        }
        "bulk_edit" => {
            let edits = edits.ok_or_else(|| anyhow!("change action=bulk_edit requires edits"))?;
            crate::engine::multi_patch(path, edits)?
        }
        "rename" => {
            let name = name.ok_or_else(|| anyhow!("change action=rename requires name"))?;
            let new_name =
                new_name.ok_or_else(|| anyhow!("change action=rename requires new_name"))?;
            crate::engine::graph_rename(path, name, new_name)?
        }
        "move" => {
            let name = name.ok_or_else(|| anyhow!("change action=move requires name"))?;
            let to_file =
                to_file.ok_or_else(|| anyhow!("change action=move requires to_file"))?;
            crate::engine::graph_move(path, name, to_file)?
        }
        "delete" => {
            let name = name.ok_or_else(|| anyhow!("change action=delete requires name"))?;
            crate::engine::graph_delete(path, name, file, force.unwrap_or(false))?
        }
        "create" => {
            let file = file.ok_or_else(|| anyhow!("change action=create requires file"))?;
            let function_name = function_name
                .ok_or_else(|| anyhow!("change action=create requires function_name"))?;
            crate::engine::graph_create(path, file, function_name, language, params, returns)?
        }
        "add" => {
            let entity_type =
                entity_type.ok_or_else(|| anyhow!("change action=add requires entity_type"))?;
            let name = name.ok_or_else(|| anyhow!("change action=add requires name"))?;
            let file = file.ok_or_else(|| anyhow!("change action=add requires file"))?;
            crate::engine::graph_add(path, entity_type, name, file, after_symbol, language, params, returns, on)?
        }
        _ => {
            return Err(anyhow!(
                "Unknown change action '{}'. Available: edit, bulk_edit, rename, move, delete, create, add",
                action
            ))
        }
    };

    let next_hint = match action.as_str() {
        "edit" | "bulk_edit" => {
            "Use inspect(file=...) or inspect(file,start_line,end_line) to verify the written code."
        }
        "rename" | "move" => {
            "Use impact(symbol=...) or inspect(name=...) to verify the updated call sites and destination."
        }
        "delete" => "Use impact(symbol=...) before forced deletion, or inspect(name=...) to review the target again.",
        "create" | "add" => "Use inspect(name=...) to review the new scaffold and change(action=edit) to refine it.",
        _ => "Use inspect(...) to verify the result of this change.",
    };

    let mut parsed = serde_json::from_str::<serde_json::Value>(&result)?
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("change expected object payload from delegated write tool"))?;
    parsed.remove("tool");
    parsed.remove("version");
    parsed.remove("project_root");

    let mut payload = serde_json::Map::new();
    payload.insert("tool".to_string(), serde_json::json!("change"));
    payload.insert("version".to_string(), serde_json::json!(env!("CARGO_PKG_VERSION")));
    payload.insert("action".to_string(), serde_json::json!(action));
    payload.insert("next_hint".to_string(), serde_json::json!(next_hint));
    payload.extend(parsed);

    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(payload))?)
}
// ── Byte-level patch ─────────────────────────────────────────────────────────

/// Public entrypoint for `patch_bytes`: splice at exact byte offsets.
#[allow(dead_code)]
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

    // Pre-write AST check — reject before touching disk.
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match std::str::from_utf8(&bytes) {
        Err(_) => return Err(anyhow!(
            "patch_bytes rejected: result is invalid UTF-8 in {} (file not modified) — \
             byte offsets likely split a multi-byte character",
            file
        )),
        Ok(patched_str) => {
            let pre_errors = ast_check_str(patched_str, ext);
            if !pre_errors.is_empty() {
                let summary: Vec<String> = pre_errors.iter()
                    .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                    .collect();
                return Err(anyhow!(
                    "patch_bytes rejected: syntax errors in {} (file not modified):\n{}",
                    file, summary.join("\n")
                ));
            }
        }
    }

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

#[derive(Debug, Clone, serde::Deserialize)]
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

        // Pre-write AST check — reject before touching disk.
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match std::str::from_utf8(&bytes) {
            Err(_) => return Err(anyhow!(
                "multi_patch rejected: result is invalid UTF-8 in {} (file not modified) — \
                 byte offsets likely split a multi-byte character",
                file
            )),
            Ok(patched_str) => {
                let pre_errors = ast_check_str(patched_str, ext);
                if !pre_errors.is_empty() {
                    let summary: Vec<String> = pre_errors.iter()
                        .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                        .collect();
                    return Err(anyhow!(
                        "multi_patch rejected: syntax errors in {} (file not modified):\n{}",
                        file, summary.join("\n")
                    ));
                }
            }
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

    // Pre-write AST check — reject before touching disk.
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let pre_errors = ast_check_str(&new_text, ext);
    if !pre_errors.is_empty() {
        let summary: Vec<String> = pre_errors.iter()
            .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
            .collect();
        return Err(anyhow!(
            "patch rejected: syntax errors in new content (file not modified):\n{}",
            summary.join("\n")
        ));
    }

    write_with_compiler_guard(&root, &full_path, file, &new_text, ext)?;

    Ok((file.to_string(), start, end, total_lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, rel: &str, content: &str) {
        let p = dir.path().join(rel);
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent).unwrap(); }
        std::fs::write(p, content).unwrap();
    }

    // ── adversarial / puncture tests ─────────────────────────────────────────

    #[test]
    fn multi_patch_rejects_when_offsets_split_multibyte_utf8_char() {
        // Hole: byte offsets that split a multi-byte UTF-8 char produce invalid UTF-8.
        // Old guard: `if let Ok(...) = from_utf8` silently skipped the check and wrote the file.
        // Fix: treat UTF-8 decode failure as a hard rejection.
        let dir = TempDir::new().unwrap();
        // "é" (U+00E9) encodes as 2 bytes: 0xC3 0xA9.
        // "fn é() {}\n" — é starts at byte 3.
        let content = "fn é() {}\n";
        write_file(&dir, "lib.rs", content);

        // Patch bytes 3..4 — splits 0xC3 | 0xA9, leaving 0xA9 as a lone continuation byte.
        let edits = vec![PatchEdit {
            file: "lib.rs".to_string(),
            byte_start: 3,
            byte_end: 4,
            new_content: "x".to_string(),
        }];

        // Verify the test precondition: result bytes are invalid UTF-8.
        let mut check = content.as_bytes().to_vec();
        check.splice(3..4, b"x".iter().copied());
        assert!(std::str::from_utf8(&check).is_err(), "precondition: result must be invalid UTF-8");

        let err = multi_patch(
            Some(dir.path().to_string_lossy().into_owned()),
            edits,
        ).unwrap_err();

        assert!(err.to_string().contains("invalid UTF-8") || err.to_string().contains("multi_patch rejected"),
            "expected UTF-8 rejection, got: {}", err);
        assert_eq!(std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(), content,
            "file must be untouched on rejection");
    }

    #[test]
    fn ast_check_str_catches_invalid_zig_syntax() {
        // Uses zig fmt --stdin when available, tree-sitter-zig as fallback.
        // Either way, obviously broken Zig must produce errors.
        let broken_zig = "fn broken syntax {{{{ totally invalid";
        let errors = ast_check_str(broken_zig, "zig");
        assert!(!errors.is_empty(), "zig syntax errors must be detected");
    }

    #[test]
    fn ast_check_str_passes_valid_zig() {
        // Exercises constructs that confused tree-sitter-zig:
        // error unions (![]u64), nested const slices ([]const []const T), builtins (@memset).
        let valid_zig = concat!(
            "const std = @import(\"std\");\n",
            "fn work(alloc: std.mem.Allocator, data: []const []const u8) ![]u64 {\n",
            "    const out = try alloc.alloc(u64, data.len);\n",
            "    @memset(out, 0);\n",
            "    return out;\n",
            "}\n",
        );
        let errors = ast_check_str(valid_zig, "zig");
        assert!(errors.is_empty(), "valid zig should have no errors, got {} error(s): {:?}", errors.len(), errors);
    }

    #[test]
    fn multi_patch_rejects_invalid_syntax() {
        let dir = TempDir::new().unwrap();
        // "fn foo() {}\n" — "{}" is at bytes 9..11
        write_file(&dir, "lib.rs", "fn foo() {}\n");

        let edits = vec![PatchEdit {
            file: "lib.rs".to_string(),
            byte_start: 9,
            byte_end: 11,
            new_content: "{{{".to_string(), // break the body
        }];
        let err = multi_patch(
            Some(dir.path().to_string_lossy().into_owned()),
            edits,
        ).unwrap_err();

        assert!(err.to_string().contains("multi_patch rejected"), "got: {}", err);
        // File must be untouched.
        assert_eq!(std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(), "fn foo() {}\n");
    }

    #[test]
    fn multi_patch_allows_valid_rust_edit() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "lib.rs", "fn foo() {}\n");

        // Replace "foo" (bytes 3..6) with "bar" — valid rename.
        let edits = vec![PatchEdit {
            file: "lib.rs".to_string(),
            byte_start: 3,
            byte_end: 6,
            new_content: "bar".to_string(),
        }];
        multi_patch(
            Some(dir.path().to_string_lossy().into_owned()),
            edits,
        ).unwrap();

        assert_eq!(std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(), "fn bar() {}\n");
    }

    #[test]
    fn change_edit_by_symbol_wraps_patch_payload() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn greet() {\n    println!(\"hi\");\n}\n");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "edit".into(),
            Some("greet".into()),
            None,
            None,
            None,
            Some("fn greet() {\n    println!(\"bye\");\n}".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "change");
        assert_eq!(v["action"], "edit");
        assert_eq!(
            v["next_hint"],
            "Use inspect(file=...) or inspect(file,start_line,end_line) to verify the written code."
        );
        assert_eq!(v["file"], "src/lib.rs");
        assert_eq!(v["patched_source"], "fn greet() {\n    println!(\"bye\");\n}");
    }

    #[test]
    fn change_rename_wraps_graph_payload() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "pub fn greet() {}\nfn call() { greet(); }\n");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "rename".into(),
            Some("greet".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("salute".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "change");
        assert_eq!(v["action"], "rename");
        assert_eq!(
            v["next_hint"],
            "Use impact(symbol=...) or inspect(name=...) to verify the updated call sites and destination."
        );
        assert_eq!(v["old_name"], "greet");
        assert_eq!(v["new_name"], "salute");
        assert!(std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap().contains("salute"));
    }

    #[test]
    fn change_bulk_edit_wraps_multi_patch_payload() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn alpha() {}\nfn beta() {}\n");

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "bulk_edit".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![
                PatchEdit {
                    file: "src/lib.rs".into(),
                    byte_start: 3,
                    byte_end: 8,
                    new_content: "omega".into(),
                },
                PatchEdit {
                    file: "src/lib.rs".into(),
                    byte_start: 17,
                    byte_end: 21,
                    new_content: "zeta".into(),
                },
            ]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "change");
        assert_eq!(v["action"], "bulk_edit");
        assert_eq!(v["edits_applied"], 2);
        assert_eq!(v["files_written"], 1);
        let content = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn omega() {}"));
        assert!(content.contains("fn zeta() {}"));
    }

    #[test]
    fn change_move_wraps_graph_move_payload() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/from.rs", "pub fn greet() {}\n");
        write_file(&dir, "src/to.rs", "pub fn keep() {}\n");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "move".into(),
            Some("greet".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("src/to.rs".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "change");
        assert_eq!(v["action"], "move");
        assert_eq!(v["from_file"], "src/from.rs");
        assert_eq!(v["to_file"], "src/to.rs");
        assert!(!std::fs::read_to_string(dir.path().join("src/from.rs")).unwrap().contains("greet"));
        assert!(std::fs::read_to_string(dir.path().join("src/to.rs")).unwrap().contains("greet"));
    }

    #[test]
    fn change_delete_wraps_graph_delete_payload() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "fn target() {}\nfn caller() { target(); }\n");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "delete".into(),
            Some("target".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(true),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "change");
        assert_eq!(v["action"], "delete");
        assert_eq!(v["name"], "target");
        assert!(!std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap().contains("fn target"));
    }

    #[test]
    fn change_create_preserves_typed_params_and_return() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "create".into(),
            None,
            Some("src/new.rs".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("compute".into()),
            None,
            None,
            Some("rust".into()),
            Some(vec![crate::engine::Param { name: "value".into(), type_str: "i32".into() }]),
            Some("i32".into()),
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["action"], "create");
        let content = std::fs::read_to_string(dir.path().join("src/new.rs")).unwrap();
        assert!(content.contains("fn compute(value: i32) -> i32"));
    }

    #[test]
    fn change_add_preserves_typed_params_return_and_receiver() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "src/lib.rs", "struct Greeter;\n");
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        let out = change(
            Some(dir.path().to_string_lossy().into_owned()),
            "add".into(),
            Some("wave".into()),
            Some("src/lib.rs".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("fn".into()),
            None,
            Some("rust".into()),
            Some(vec![crate::engine::Param { name: "value".into(), type_str: "i32".into() }]),
            Some("i32".into()),
            Some("Greeter".into()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["action"], "add");
        let content = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("impl Greeter"));
        assert!(content.contains("fn wave(value: i32) -> i32"));
    }
}
