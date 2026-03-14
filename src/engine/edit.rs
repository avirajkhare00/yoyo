use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use std::collections::HashMap;

use super::types::{MultiPatchPayload, PatchBytesPayload, PatchPayload, SlicePayload, SyntaxError};
use super::util::{detect_language, reindex_files, require_bake_index, resolve_project_root};

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
        "rs" => parser.set_language(&SupportLang::Rust.get_ts_language()),
        "go" => parser.set_language(&SupportLang::Go.get_ts_language()),
        "py" => parser.set_language(&SupportLang::Python.get_ts_language()),
        "ts" | "tsx" | "js" | "jsx" => {
            parser.set_language(&SupportLang::TypeScript.get_ts_language())
        }
        _ => return vec![],
    };
    if ok.is_err() {
        return vec![];
    }
    let Some(tree) = parser.parse(source, None) else {
        return vec![];
    };
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
    stderr
        .lines()
        .filter_map(|line| {
            let after = line.strip_prefix("<stdin>:")?;
            let mut parts = after.splitn(3, ':');
            let line_num: u32 = parts.next()?.parse().ok()?;
            let _col = parts.next();
            let rest = parts.next()?.trim();
            let text = rest
                .strip_prefix("error: ")
                .unwrap_or(rest)
                .chars()
                .take(80)
                .collect();
            Some(SyntaxError {
                line: line_num,
                kind: "error".to_string(),
                text,
            })
        })
        .collect()
}

// ── Post-patch syntax validation ──────────────────────────────────────────────

/// Parse `file` with tree-sitter and return any ERROR/MISSING nodes.
/// Returns an empty vec if the language is unsupported or the file can't be read.
fn syntax_check(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    let full_path = root.join(file);
    let Ok(source) = fs::read_to_string(&full_path) else {
        return vec![];
    };

    use ast_grep_language::{LanguageExt, SupportLang};
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut parser = tree_sitter::Parser::new();
    let ok = match ext {
        "rs" => parser.set_language(&SupportLang::Rust.get_ts_language()),
        "go" => parser.set_language(&SupportLang::Go.get_ts_language()),
        "py" => parser.set_language(&SupportLang::Python.get_ts_language()),
        "ts" | "tsx" | "js" | "jsx" => {
            parser.set_language(&SupportLang::TypeScript.get_ts_language())
        }
        _ => return vec![],
    };
    if ok.is_err() {
        return vec![];
    }

    let Some(tree) = parser.parse(&source, None) else {
        return vec![];
    };
    let mut errors = vec![];
    collect_errors(tree.root_node(), &source, &mut errors);
    // For each supported language, run the compiler/interpreter checker to catch
    // errors that tree-sitter cannot see (macros, type errors, import issues, etc.).
    errors.extend(semantic_check_errors(root, &full_path, file));

    errors
}

fn cargo_check_errors_for_files(
    root: &PathBuf,
    files: &[&str],
) -> HashMap<String, Vec<SyntaxError>> {
    filter_error_map(cargo_all_check_errors(root), files)
}

fn cargo_all_check_errors(root: &PathBuf) -> HashMap<String, Vec<SyntaxError>> {
    use std::process::Command;

    let output = Command::new("cargo")
        .args(["check", "--message-format=json", "--quiet"])
        .current_dir(root)
        .output();

    let Ok(output) = output else {
        return HashMap::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors_by_file: HashMap<String, Vec<SyntaxError>> = HashMap::new();

    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }

        let message = &msg["message"];
        if message.get("level").and_then(|l| l.as_str()) != Some("error") {
            continue;
        }

        let Some(spans) = message.get("spans").and_then(|s| s.as_array()) else {
            continue;
        };

        for span in spans {
            let span_file = span.get("file_name").and_then(|f| f.as_str()).unwrap_or("");
            let file = normalize_path(span_file);

            let line_num = span.get("line_start").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
            let raw = message
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let text: String = raw.chars().take(120).collect();
            errors_by_file.entry(file).or_default().push(SyntaxError {
                line: line_num,
                kind: "cargo".to_string(),
                text,
            });
        }
    }

    errors_by_file
}

fn go_build_errors_for_files(root: &PathBuf, files: &[&str]) -> HashMap<String, Vec<SyntaxError>> {
    filter_error_map(go_build_all_errors(root), files)
}

fn go_build_all_errors(root: &PathBuf) -> HashMap<String, Vec<SyntaxError>> {
    use std::process::Command;

    let output = Command::new("go")
        .args(["build", "./..."])
        .current_dir(root)
        .output();

    let Ok(output) = output else {
        return HashMap::new();
    };

    // go build writes errors to stderr in the form: file.go:line:col: message
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut errors_by_file: HashMap<String, Vec<SyntaxError>> = HashMap::new();
    for line in stderr.lines() {
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() < 4 {
            continue;
        }
        let file = normalize_path(parts[0]);
        let line_num = parts[1].trim().parse::<u32>().unwrap_or(0);
        let text: String = parts[3].trim().chars().take(120).collect();
        errors_by_file.entry(file).or_default().push(SyntaxError {
            line: line_num,
            kind: "go".to_string(),
            text,
        });
    }
    errors_by_file
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn matching_requested_file<'a>(reported: &str, files: &'a [&str]) -> Option<&'a str> {
    let reported_norm = normalize_path(reported);
    let reported_name = std::path::Path::new(reported)
        .file_name()
        .and_then(|name| name.to_str());

    files.iter().copied().find(|file| {
        let requested_norm = normalize_path(file);
        let requested_name = std::path::Path::new(file)
            .file_name()
            .and_then(|name| name.to_str());

        reported_norm.ends_with(&requested_norm)
            || requested_norm.ends_with(&reported_norm)
            || (reported_name.is_some() && reported_name == requested_name)
    })
}

fn take_file_errors(
    mut errors_by_file: HashMap<String, Vec<SyntaxError>>,
    file: &str,
) -> Vec<SyntaxError> {
    errors_by_file.remove(file).unwrap_or_default()
}

fn filter_error_map(
    errors_by_file: HashMap<String, Vec<SyntaxError>>,
    files: &[&str],
) -> HashMap<String, Vec<SyntaxError>> {
    let mut filtered = HashMap::new();
    for (reported_file, errors) in errors_by_file {
        let Some(file) = matching_requested_file(&reported_file, files) else {
            continue;
        };
        filtered
            .entry(file.to_string())
            .or_insert_with(Vec::new)
            .extend(errors);
    }
    filtered
}

fn merge_error_maps(
    target: &mut HashMap<String, Vec<SyntaxError>>,
    incoming: HashMap<String, Vec<SyntaxError>>,
) {
    for (file, mut errors) in incoming {
        target.entry(file).or_default().append(&mut errors);
    }
}

fn error_fingerprint(file: &str, err: &SyntaxError) -> (String, u32, String, String) {
    (
        normalize_path(file),
        err.line,
        err.kind.clone(),
        err.text.clone(),
    )
}

fn diff_error_maps(
    after: HashMap<String, Vec<SyntaxError>>,
    before: &HashMap<String, Vec<SyntaxError>>,
) -> HashMap<String, Vec<SyntaxError>> {
    let mut before_counts: HashMap<(String, u32, String, String), usize> = HashMap::new();
    for (file, errors) in before {
        for err in errors {
            *before_counts
                .entry(error_fingerprint(file, err))
                .or_insert(0) += 1;
        }
    }

    let mut delta = HashMap::new();
    for (file, errors) in after {
        for err in errors {
            let key = error_fingerprint(&file, &err);
            if let Some(count) = before_counts.get_mut(&key) {
                if *count > 0 {
                    *count -= 1;
                    continue;
                }
            }
            delta.entry(file.clone()).or_insert_with(Vec::new).push(err);
        }
    }
    delta
}

#[derive(Default)]
struct ProjectWideBaselines {
    cargo: Option<HashMap<String, Vec<SyntaxError>>>,
    go: Option<HashMap<String, Vec<SyntaxError>>>,
    tsc: Option<HashMap<String, Vec<SyntaxError>>>,
}

// zig ast-check performs full semantic analysis (type checking, undefined symbols)
// on a single file without requiring a complete build system setup.
fn zig_check_errors(file: &std::path::Path) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("zig")
        .args(["ast-check", file.to_str().unwrap_or("")])
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() {
        return vec![];
    }

    // zig ast-check writes to stderr: path:line:col: error: message
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut errors = vec![];
    for line in stderr.lines() {
        if !line.contains("error:") {
            continue;
        }
        let parts: Vec<&str> = line.splitn(5, ':').collect();
        if parts.len() < 5 {
            continue;
        }
        let line_num = parts[1].trim().parse::<u32>().unwrap_or(0);
        let text: String = parts[4].trim().chars().take(120).collect();
        errors.push(SyntaxError {
            line: line_num,
            kind: "zig".to_string(),
            text,
        });
    }
    errors
}

pub(super) fn semantic_check_errors(
    root: &PathBuf,
    full_path: &std::path::Path,
    file: &str,
) -> Vec<SyntaxError> {
    take_file_errors(
        semantic_check_errors_for_files(root, &[(full_path, file)]),
        file,
    )
}

pub(super) fn semantic_check_errors_for_files(
    root: &PathBuf,
    files: &[(&std::path::Path, &str)],
) -> HashMap<String, Vec<SyntaxError>> {
    let mut errors_by_file: HashMap<String, Vec<SyntaxError>> = HashMap::new();
    let mut cargo_files = Vec::new();
    let mut go_files = Vec::new();
    let mut tsc_files = Vec::new();

    for (full_path, file) in files {
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "rs" => cargo_files.push(*file),
            "go" => go_files.push(*file),
            "ts" | "tsx" | "jsx" => tsc_files.push(*file),
            "zig" => {
                let errors = zig_check_errors(full_path);
                if !errors.is_empty() {
                    errors_by_file.insert((*file).to_string(), errors);
                }
            }
            "py" => {
                let errors = python_compile_errors(root, file);
                if !errors.is_empty() {
                    errors_by_file.insert((*file).to_string(), errors);
                }
            }
            "js" => {
                let errors = node_check_errors(root, file);
                if !errors.is_empty() {
                    errors_by_file.insert((*file).to_string(), errors);
                }
            }
            "rb" => {
                let errors = ruby_check_errors(root, file);
                if !errors.is_empty() {
                    errors_by_file.insert((*file).to_string(), errors);
                }
            }
            "php" => {
                let errors = php_check_errors(root, file);
                if !errors.is_empty() {
                    errors_by_file.insert((*file).to_string(), errors);
                }
            }
            "sh" | "bash" => {
                let errors = bash_check_errors(root, file);
                if !errors.is_empty() {
                    errors_by_file.insert((*file).to_string(), errors);
                }
            }
            _ => {}
        }
    }

    for (file, mut errors) in cargo_check_errors_for_files(root, &cargo_files) {
        errors_by_file.entry(file).or_default().append(&mut errors);
    }
    for (file, mut errors) in go_build_errors_for_files(root, &go_files) {
        errors_by_file.entry(file).or_default().append(&mut errors);
    }
    for (file, mut errors) in tsc_errors_for_files(root, &tsc_files) {
        errors_by_file.entry(file).or_default().append(&mut errors);
    }

    errors_by_file
}

fn collect_project_wide_baselines(
    root: &PathBuf,
    writes: &[PendingWrite<'_>],
) -> ProjectWideBaselines {
    let mut baselines = ProjectWideBaselines::default();
    let mut has_rust = false;
    let mut has_go = false;
    let mut has_tsc = false;

    for write in writes {
        let ext = write
            .full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "rs" => has_rust = true,
            "go" => has_go = true,
            "ts" | "tsx" | "jsx" => has_tsc = true,
            _ => {}
        }
    }

    if has_rust {
        baselines.cargo = Some(cargo_all_check_errors(root));
    }
    if has_go {
        baselines.go = Some(go_build_all_errors(root));
    }
    if has_tsc {
        baselines.tsc = Some(tsc_all_errors(root));
    }

    baselines
}

fn batch_semantic_check_errors(
    root: &PathBuf,
    writes: &[PendingWrite<'_>],
    baselines: &ProjectWideBaselines,
) -> HashMap<String, Vec<SyntaxError>> {
    let mut errors_by_file = HashMap::new();

    for write in writes {
        let ext = write
            .full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "zig" => {
                let errors = zig_check_errors(write.full_path);
                if !errors.is_empty() {
                    errors_by_file.insert(write.file.to_string(), errors);
                }
            }
            "py" => {
                let errors = python_compile_errors(root, write.file);
                if !errors.is_empty() {
                    errors_by_file.insert(write.file.to_string(), errors);
                }
            }
            "js" => {
                let errors = node_check_errors(root, write.file);
                if !errors.is_empty() {
                    errors_by_file.insert(write.file.to_string(), errors);
                }
            }
            "rb" => {
                let errors = ruby_check_errors(root, write.file);
                if !errors.is_empty() {
                    errors_by_file.insert(write.file.to_string(), errors);
                }
            }
            "php" => {
                let errors = php_check_errors(root, write.file);
                if !errors.is_empty() {
                    errors_by_file.insert(write.file.to_string(), errors);
                }
            }
            "sh" | "bash" => {
                let errors = bash_check_errors(root, write.file);
                if !errors.is_empty() {
                    errors_by_file.insert(write.file.to_string(), errors);
                }
            }
            _ => {}
        }
    }

    if let Some(before) = &baselines.cargo {
        merge_error_maps(
            &mut errors_by_file,
            diff_error_maps(cargo_all_check_errors(root), before),
        );
    }
    if let Some(before) = &baselines.go {
        merge_error_maps(
            &mut errors_by_file,
            diff_error_maps(go_build_all_errors(root), before),
        );
    }
    if let Some(before) = &baselines.tsc {
        merge_error_maps(
            &mut errors_by_file,
            diff_error_maps(tsc_all_errors(root), before),
        );
    }

    errors_by_file
}

pub(super) struct PendingWrite<'a> {
    pub(super) full_path: &'a std::path::Path,
    pub(super) file: &'a str,
    pub(super) bytes: &'a [u8],
}

#[derive(Debug, Default, serde::Deserialize)]
struct RuntimeFeedbackConfig {
    #[serde(default)]
    runtime_checks: Vec<RuntimeSmokeCheck>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RuntimeSmokeCheck {
    language: String,
    command: Vec<String>,
    #[serde(default)]
    sandbox_prefix: Vec<String>,
    #[serde(default)]
    allow_unsandboxed: bool,
    kind: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct PreparedRuntimeCheck {
    target_key: String,
    kind: String,
    argv: Vec<String>,
    timeout_ms: u64,
}

struct RuntimeCommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn runtime_config_path(root: &PathBuf) -> Option<PathBuf> {
    root.ancestors()
        .map(|ancestor| ancestor.join(".yoyo").join("runtime.json"))
        .find(|path| path.exists())
}

fn load_runtime_feedback_config(root: &PathBuf) -> Result<Option<RuntimeFeedbackConfig>> {
    let Some(path) = runtime_config_path(root) else {
        return Ok(None);
    };
    let bytes = fs::read(&path)
        .with_context(|| format!("Failed to read runtime config {}", path.display()))?;
    let config = serde_json::from_slice::<RuntimeFeedbackConfig>(&bytes)
        .with_context(|| format!("Failed to parse runtime config {}", path.display()))?;
    Ok(Some(config))
}

fn runtime_timeout_ms(check: &RuntimeSmokeCheck) -> u64 {
    let timeout = check.timeout_ms.unwrap_or(3_000);
    timeout.clamp(100, 10_000)
}

fn replace_runtime_placeholders(value: &str, root: &PathBuf, write: &PendingWrite<'_>) -> String {
    value
        .replace("{{file}}", write.file)
        .replace("{{abs_file}}", &write.full_path.to_string_lossy())
        .replace("{{root}}", &root.to_string_lossy())
}

fn command_basename(command: &str) -> String {
    std::path::Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase()
}

fn has_inline_eval_flag(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-c" | "/c"
                | "-e"
                | "--eval"
                | "-r"
                | "-Command"
                | "-command"
                | "-EncodedCommand"
                | "-encodedCommand"
        )
    })
}

fn command_targets_changed_file(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg.contains("{{file}}") || arg.contains("{{abs_file}}"))
}

fn effective_runtime_command_parts<'a>(parts: &'a [String], context: &str) -> Result<&'a [String]> {
    if parts.is_empty() {
        return Ok(parts);
    }

    if command_basename(&parts[0]) != "env" {
        return Ok(parts);
    }

    let mut idx = 1usize;
    while idx < parts.len() {
        let arg = &parts[idx];
        if arg == "-S" || arg == "--split-string" {
            return Err(anyhow!(
                "runtime feedback config rejected unsafe {} command: env -S is not allowed",
                context
            ));
        }
        if arg.starts_with('-') || arg.contains('=') {
            idx += 1;
            continue;
        }
        break;
    }

    if idx >= parts.len() {
        return Err(anyhow!(
            "runtime feedback config rejected unsafe {} command: missing executable after env",
            context
        ));
    }

    Ok(&parts[idx..])
}

fn validate_runtime_command_parts(parts: &[String], context: &str) -> Result<()> {
    if parts.is_empty() {
        return Err(anyhow!(
            "runtime feedback config has empty {} command",
            context
        ));
    }

    let effective = effective_runtime_command_parts(parts, context)?;
    let exe = command_basename(&effective[0]);
    if matches!(
        exe.as_str(),
        "python"
            | "python3"
            | "node"
            | "ruby"
            | "php"
            | "bash"
            | "sh"
            | "zsh"
            | "fish"
            | "cmd"
            | "cmd.exe"
            | "powershell"
            | "pwsh"
    ) && has_inline_eval_flag(&effective[1..])
    {
        return Err(anyhow!(
            "runtime feedback config rejected unsafe {} command: inline interpreter code is not allowed",
            context
        ));
    }

    Ok(())
}

fn prepare_runtime_checks(
    root: &PathBuf,
    writes: &[PendingWrite<'_>],
) -> Result<Vec<PreparedRuntimeCheck>> {
    let Some(config) = load_runtime_feedback_config(root)? else {
        return Ok(vec![]);
    };

    let mut prepared = Vec::new();
    for write in writes {
        let language = detect_language(write.full_path);
        for check in &config.runtime_checks {
            if check.language != language {
                continue;
            }

            if check.command.is_empty() {
                return Err(anyhow!(
                    "runtime feedback config rejected {} runtime check: command must not be empty",
                    check.language
                ));
            }
            if !command_targets_changed_file(&check.command) {
                return Err(anyhow!(
                    "runtime feedback config rejected {} runtime check: command must target the changed file with {{file}} or {{abs_file}}",
                    check.language
                ));
            }

            let command: Vec<String> = check
                .command
                .iter()
                .map(|part| replace_runtime_placeholders(part, root, write))
                .collect();
            validate_runtime_command_parts(&command, "runtime")?;

            let sandbox_prefix: Vec<String> = check
                .sandbox_prefix
                .iter()
                .map(|part| replace_runtime_placeholders(part, root, write))
                .collect();
            if !sandbox_prefix.is_empty() {
                validate_runtime_command_parts(&sandbox_prefix, "sandbox prefix")?;
            } else if !check.allow_unsandboxed {
                continue;
            }

            let mut argv = sandbox_prefix;
            argv.extend(command);

            prepared.push(PreparedRuntimeCheck {
                target_key: write.file.to_string(),
                kind: check
                    .kind
                    .clone()
                    .unwrap_or_else(|| format!("{}-runtime", language)),
                argv,
                timeout_ms: runtime_timeout_ms(check),
            });
        }
    }

    Ok(prepared)
}

fn temp_capture_path(prefix: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir();
    for attempt in 0..32u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let candidate = base.join(format!(
            "yoyo-runtime-{}-{}-{}-{}.log",
            prefix,
            std::process::id(),
            nanos,
            attempt
        ));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(anyhow!(err).context("Failed to allocate runtime capture file")),
        }
    }

    Err(anyhow!("Failed to allocate runtime capture file"))
}

fn read_and_remove_capture(path: &PathBuf) -> String {
    let content = fs::read_to_string(path).unwrap_or_default();
    let _ = fs::remove_file(path);
    content
}

fn run_bounded_command(
    root: &PathBuf,
    argv: &[String],
    timeout_ms: u64,
) -> Result<Option<RuntimeCommandOutput>> {
    let stdout_path = temp_capture_path("stdout")?;
    let stderr_path = temp_capture_path("stderr")?;

    let stdout_file = match std::fs::OpenOptions::new().write(true).open(&stdout_path) {
        Ok(file) => file,
        Err(err) => {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(anyhow!(err).context("Failed to open runtime stdout capture"));
        }
    };
    let stderr_file = match std::fs::OpenOptions::new().write(true).open(&stderr_path) {
        Ok(file) => file,
        Err(err) => {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(anyhow!(err).context("Failed to open runtime stderr capture"));
        }
    };

    let mut command = std::process::Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.current_dir(root);
    command.stdout(stdout_file);
    command.stderr(stderr_file);

    let spawn_result = command.spawn();
    let mut child = match spawn_result {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Ok(None);
        }
        Err(err) => {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(anyhow!(err).context("Failed to spawn runtime feedback command"));
        }
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let (status, timed_out) = loop {
        if let Some(status) = child
            .try_wait()
            .context("Failed to poll runtime feedback command")?
        {
            break (status, false);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let status = child
                .wait()
                .context("Failed to wait for timed out runtime feedback command")?;
            break (status, true);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };

    let stdout = read_and_remove_capture(&stdout_path);
    let stderr = read_and_remove_capture(&stderr_path);
    Ok(Some(RuntimeCommandOutput {
        status,
        stdout,
        stderr,
        timed_out,
    }))
}

fn runtime_errors_from_output(
    check: &PreparedRuntimeCheck,
    output: RuntimeCommandOutput,
) -> Vec<SyntaxError> {
    if !output.timed_out && output.status.success() {
        return vec![];
    }

    if output.timed_out {
        return vec![SyntaxError {
            line: 0,
            kind: check.kind.clone(),
            text: format!("runtime feedback timed out after {}ms", check.timeout_ms),
        }];
    }

    let combined = format!("{}{}", output.stdout, output.stderr);
    let mut errors: Vec<SyntaxError> = combined
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(5)
        .map(|line| SyntaxError {
            line: 0,
            kind: check.kind.clone(),
            text: line.chars().take(120).collect(),
        })
        .collect();

    if errors.is_empty() {
        errors.push(SyntaxError {
            line: 0,
            kind: check.kind.clone(),
            text: format!(
                "runtime feedback exited with status {}",
                output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "terminated by signal".to_string())
            ),
        });
    }

    errors
}

fn collect_runtime_feedback_errors(
    root: &PathBuf,
    checks: &[PreparedRuntimeCheck],
) -> Result<HashMap<String, Vec<SyntaxError>>> {
    let mut errors_by_file = HashMap::new();
    for check in checks {
        let Some(output) = run_bounded_command(root, &check.argv, check.timeout_ms)? else {
            continue;
        };
        let errors = runtime_errors_from_output(check, output);
        if errors.is_empty() {
            continue;
        }
        errors_by_file.insert(check.target_key.clone(), errors);
    }
    Ok(errors_by_file)
}

fn format_guard_errors(errors: &[SyntaxError]) -> String {
    errors
        .iter()
        .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn restore_batch_originals(
    writes: &[PendingWrite<'_>],
    originals: &[Option<Vec<u8>>],
) -> Result<()> {
    for (write, original) in writes.iter().zip(originals.iter()) {
        match original {
            Some(bytes) => {
                fs::write(write.full_path, bytes)
                    .with_context(|| format!("Failed to restore {}", write.file))?;
            }
            None => {
                if write.full_path.exists() {
                    fs::remove_file(write.full_path)
                        .with_context(|| format!("Failed to remove {}", write.file))?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn write_batch_with_compiler_guard(
    root: &PathBuf,
    writes: &[PendingWrite<'_>],
) -> Result<()> {
    let baselines = collect_project_wide_baselines(root, writes);
    let runtime_checks = prepare_runtime_checks(root, writes)?;
    let runtime_baseline = collect_runtime_feedback_errors(root, &runtime_checks)?;
    let originals: Vec<Option<Vec<u8>>> = writes
        .iter()
        .map(|write| fs::read(write.full_path).ok())
        .collect();
    let mut written = 0usize;

    for write in writes {
        if let Err(err) = fs::write(write.full_path, write.bytes) {
            if written > 0 {
                restore_batch_originals(&writes[..written], &originals[..written])?;
            }
            return Err(anyhow!(err).context(format!("Failed to write {}", write.file)));
        }
        written += 1;
    }

    let mut errors_by_file = batch_semantic_check_errors(root, writes, &baselines);
    if errors_by_file.is_empty() && !runtime_checks.is_empty() {
        merge_error_maps(
            &mut errors_by_file,
            diff_error_maps(
                collect_runtime_feedback_errors(root, &runtime_checks)?,
                &runtime_baseline,
            ),
        );
    }
    if errors_by_file.is_empty() {
        return Ok(());
    }

    restore_batch_originals(writes, &originals)?;

    let mut summaries: Vec<String> = errors_by_file
        .drain()
        .map(|(file, errors)| format!("{}:\n{}", file, format_guard_errors(&errors)))
        .collect();
    summaries.sort();
    Err(anyhow!(
        "patch rejected: compiler/interpreter errors (files restored to original):\n{}",
        summaries.join("\n")
    ))
}

// Write new_text to full_path, run the compiler/interpreter guard, restore original if errors.
// Guarantees: if this returns Ok, the file on disk passed the configured guardrails.
pub(super) fn write_bytes_with_compiler_guard(
    root: &PathBuf,
    full_path: &std::path::Path,
    file: &str,
    new_bytes: &[u8],
) -> Result<()> {
    let writes = [PendingWrite {
        full_path,
        file,
        bytes: new_bytes,
    }];
    write_batch_with_compiler_guard(root, &writes)
}

// Write new_text to full_path, run the compiler/interpreter guard, restore original if errors.
// Guarantees: if this returns Ok, the file on disk passed the configured guardrails.
fn write_with_compiler_guard(
    root: &PathBuf,
    full_path: &std::path::Path,
    file: &str,
    new_text: &str,
    _ext: &str,
) -> Result<()> {
    write_bytes_with_compiler_guard(root, full_path, file, new_text.as_bytes())
}

/// Run `python -m py_compile <file>` and return syntax errors. Best-effort.
fn python_compile_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("python3")
        .args(["-m", "py_compile", file])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() {
        return vec![];
    }

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
        } else if ln.trim().starts_with("SyntaxError:")
            || ln.trim().starts_with("IndentationError:")
        {
            let text: String = ln.trim().chars().take(120).collect();
            errors.push(SyntaxError {
                line: line_num,
                kind: "python".to_string(),
                text,
            });
        }
    }
    errors
}

/// Run `bash -n <file>` and return syntax errors. Best-effort.
fn bash_check_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("bash")
        .args(["-n", file])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() {
        return vec![];
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file);

    let mut errors = vec![];
    for ln in stderr.lines() {
        if !ln.contains(file_name) {
            continue;
        }
        let Some((_, rest)) = ln.split_once(": line ") else {
            continue;
        };
        let Some((line_text, message)) = rest.split_once(':') else {
            continue;
        };
        let line_num = line_text.trim().parse::<u32>().unwrap_or(0);
        let text: String = message.trim().chars().take(120).collect();
        errors.push(SyntaxError {
            line: line_num,
            kind: "bash".to_string(),
            text,
        });
    }
    errors
}

/// Run `node --check <file>` and return syntax errors. Best-effort.
fn node_check_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("node")
        .args(["--check", file])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() {
        return vec![];
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file);

    let mut current_line = 0u32;
    let mut errors = vec![];
    for ln in stderr.lines() {
        if let Some(rest) = ln.trim().strip_prefix(file_name).or_else(|| {
            ln.trim()
                .rsplit_once('/')
                .and_then(|(_, tail)| tail.strip_prefix(file_name))
        }) {
            if let Some(line_text) = rest.strip_prefix(':') {
                current_line = line_text.trim().parse::<u32>().unwrap_or(0);
            }
        } else if ln.trim().starts_with("SyntaxError:") {
            let text: String = ln.trim().chars().take(120).collect();
            errors.push(SyntaxError {
                line: current_line,
                kind: "node".to_string(),
                text,
            });
        }
    }
    if errors.is_empty() {
        if let Some(line) = stderr.lines().find(|line| !line.trim().is_empty()) {
            errors.push(SyntaxError {
                line: current_line,
                kind: "node".to_string(),
                text: line.trim().chars().take(120).collect(),
            });
        }
    }
    errors
}

/// Run `ruby -c <file>` and return syntax errors. Best-effort.
fn ruby_check_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("ruby")
        .args(["-c", file])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() {
        return vec![];
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file);

    let mut errors = vec![];
    for ln in stderr.lines() {
        if !ln.contains(file_name) {
            continue;
        }
        let parts: Vec<&str> = ln.splitn(3, ':').collect();
        if parts.len() < 3 {
            continue;
        }
        let line_num = parts[1].trim().parse::<u32>().unwrap_or(0);
        let text: String = parts[2].trim().chars().take(120).collect();
        errors.push(SyntaxError {
            line: line_num,
            kind: "ruby".to_string(),
            text,
        });
    }
    if errors.is_empty() {
        if let Some(line) = stderr.lines().find(|line| !line.trim().is_empty()) {
            errors.push(SyntaxError {
                line: 0,
                kind: "ruby".to_string(),
                text: line.trim().chars().take(120).collect(),
            });
        }
    }
    errors
}

/// Run `php -l <file>` and return syntax errors. Best-effort.
fn php_check_errors(root: &PathBuf, file: &str) -> Vec<SyntaxError> {
    use std::process::Command;

    let output = Command::new("php")
        .args(["-l", file])
        .current_dir(root)
        .output();

    let Ok(output) = output else { return vec![] };
    if output.status.success() {
        return vec![];
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file);

    let mut errors = vec![];
    for ln in combined.lines() {
        if !ln.contains(file_name) {
            continue;
        }
        let line_num = ln
            .rsplit_once(" on line ")
            .and_then(|(_, n)| n.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let text: String = ln.trim().chars().take(120).collect();
        errors.push(SyntaxError {
            line: line_num,
            kind: "php".to_string(),
            text,
        });
    }
    if errors.is_empty() {
        if let Some(line) = combined.lines().find(|line| !line.trim().is_empty()) {
            errors.push(SyntaxError {
                line: 0,
                kind: "php".to_string(),
                text: line.trim().chars().take(120).collect(),
            });
        }
    }
    errors
}

fn tsc_errors_for_files(root: &PathBuf, files: &[&str]) -> HashMap<String, Vec<SyntaxError>> {
    filter_error_map(tsc_all_errors(root), files)
}

fn tsc_all_errors(root: &PathBuf) -> HashMap<String, Vec<SyntaxError>> {
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

    let Ok(output) = output else {
        return HashMap::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    let mut errors_by_file: HashMap<String, Vec<SyntaxError>> = HashMap::new();
    for ln in combined.lines() {
        // Format: path/file.ts(LINE,COL): error TS####: message
        let Some(paren) = ln.find('(') else { continue };
        let file = normalize_path(&ln[..paren]);
        let rest = &ln[paren + 1..];
        let Some(comma) = rest.find(',') else {
            continue;
        };
        let line_num = rest[..comma].parse::<u32>().unwrap_or(0);
        let text = ln.split(": ").skip(2).collect::<Vec<_>>().join(": ");
        let text: String = text.chars().take(120).collect();
        errors_by_file.entry(file).or_default().push(SyntaxError {
            line: line_num,
            kind: "tsc".to_string(),
            text,
        });
    }
    errors_by_file
}

fn collect_errors(node: tree_sitter::Node, source: &str, errors: &mut Vec<SyntaxError>) {
    if node.is_error() || node.is_missing() {
        let line = node.start_position().row as u32 + 1;
        let raw = node.utf8_text(source.as_bytes()).unwrap_or("").trim();
        let text: String = raw.chars().take(80).collect();
        let kind = if node.is_missing() {
            "missing"
        } else {
            "error"
        }
        .to_string();
        errors.push(SyntaxError { line, kind, text });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, source, errors);
    }
}

/// Public entrypoint for the `slice` tool: read a specific line range of a file.
pub fn slice(path: Option<String>, file: String, start: u32, end: u32) -> Result<String> {
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
    let content =
        fs::read_to_string(&full_path).with_context(|| format!("Failed to read {}", file))?;

    let pos = content.find(&old_string).ok_or_else(|| {
        anyhow!(
            "old_string not found in {}. Check exact whitespace and content.",
            file
        )
    })?;

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
        let summary: Vec<String> = pre_errors
            .iter()
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
                Some((
                    f.file.clone(),
                    f.start_line,
                    f.end_line,
                    fname == needle,
                    f.complexity,
                ))
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
        return Err(anyhow!(
            "No symbol match for name {:?}. Run `bake` and ensure the symbol exists.",
            name
        ));
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
    payload.insert(
        "version".to_string(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    payload.insert("action".to_string(), serde_json::json!(action));
    payload.insert("next_hint".to_string(), serde_json::json!(next_hint));
    payload.extend(parsed);

    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        payload,
    ))?)
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
        format!(
            "Failed to read file {} (resolved to {})",
            file,
            full_path.display()
        )
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
        Err(_) => {
            return Err(anyhow!(
                "patch_bytes rejected: result is invalid UTF-8 in {} (file not modified) — \
             byte offsets likely split a multi-byte character",
                file
            ))
        }
        Ok(patched_str) => {
            let pre_errors = ast_check_str(patched_str, ext);
            if !pre_errors.is_empty() {
                let summary: Vec<String> = pre_errors
                    .iter()
                    .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                    .collect();
                return Err(anyhow!(
                    "patch_bytes rejected: syntax errors in {} (file not modified):\n{}",
                    file,
                    summary.join("\n")
                ));
            }
        }
    }

    write_bytes_with_compiler_guard(&root, &full_path, &file, &bytes)?;
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
    let mut planned_writes: Vec<(String, PathBuf, Vec<u8>)> = Vec::new();

    for (file, mut file_edits) in by_file {
        let full_path = root.join(&file);
        let mut bytes = fs::read(&full_path).with_context(|| {
            format!(
                "Failed to read file {} (resolved to {})",
                file,
                full_path.display()
            )
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
            bytes.splice(
                edit.byte_start..edit.byte_end,
                edit.new_content.as_bytes().iter().copied(),
            );
        }

        // Pre-write AST check — reject before touching disk.
        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match std::str::from_utf8(&bytes) {
            Err(_) => {
                return Err(anyhow!(
                    "multi_patch rejected: result is invalid UTF-8 in {} (file not modified) — \
                 byte offsets likely split a multi-byte character",
                    file
                ))
            }
            Ok(patched_str) => {
                let pre_errors = ast_check_str(patched_str, ext);
                if !pre_errors.is_empty() {
                    let summary: Vec<String> = pre_errors
                        .iter()
                        .map(|e| format!("line {}: {} — {}", e.line, e.kind, e.text))
                        .collect();
                    return Err(anyhow!(
                        "multi_patch rejected: syntax errors in {} (file not modified):\n{}",
                        file,
                        summary.join("\n")
                    ));
                }
            }
        }

        planned_writes.push((file, full_path, bytes));
    }

    let writes: Vec<PendingWrite<'_>> = planned_writes
        .iter()
        .map(|(file, full_path, bytes)| PendingWrite {
            full_path: full_path.as_path(),
            file: file.as_str(),
            bytes: bytes.as_slice(),
        })
        .collect();
    write_batch_with_compiler_guard(&root, &writes)?;

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
        let summary: Vec<String> = pre_errors
            .iter()
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
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    fn write_runtime_config(dir: &TempDir, content: &str) {
        write_file(dir, ".yoyo/runtime.json", content);
    }

    fn command_available(cmd: &str, version_arg: &str) -> bool {
        std::process::Command::new(cmd)
            .arg(version_arg)
            .output()
            .is_ok()
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
        assert!(
            std::str::from_utf8(&check).is_err(),
            "precondition: result must be invalid UTF-8"
        );

        let err = multi_patch(Some(dir.path().to_string_lossy().into_owned()), edits).unwrap_err();

        assert!(
            err.to_string().contains("invalid UTF-8")
                || err.to_string().contains("multi_patch rejected"),
            "expected UTF-8 rejection, got: {}",
            err
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(),
            content,
            "file must be untouched on rejection"
        );
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
        assert!(
            errors.is_empty(),
            "valid zig should have no errors, got {} error(s): {:?}",
            errors.len(),
            errors
        );
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
        let err = multi_patch(Some(dir.path().to_string_lossy().into_owned()), edits).unwrap_err();

        assert!(
            err.to_string().contains("multi_patch rejected"),
            "got: {}",
            err
        );
        // File must be untouched.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(),
            "fn foo() {}\n"
        );
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
        multi_patch(Some(dir.path().to_string_lossy().into_owned()), edits).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(),
            "fn bar() {}\n"
        );
    }

    #[test]
    fn multi_patch_applies_cross_file_rust_rename_atomically() {
        let dir = TempDir::new().unwrap();
        let cargo =
            "[package]\nname = \"multi-patch-guard\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let lib = "mod a;\nmod b;\npub use a::helper_fn;\npub use b::call_value;\n";
        let a_src = "pub fn helper_fn() -> i32 { 1 }\n";
        let b_src = "use crate::helper_fn;\npub fn call_value() -> i32 { helper_fn() }\n";
        write_file(&dir, "Cargo.toml", cargo);
        write_file(&dir, "src/lib.rs", lib);
        write_file(&dir, "src/a.rs", a_src);
        write_file(&dir, "src/b.rs", b_src);

        let helper_len = "helper_fn".len();
        let b_first = b_src.find("helper_fn").unwrap();
        let b_second = b_src.rfind("helper_fn").unwrap();
        assert_ne!(
            b_first, b_second,
            "expected two helper_fn references in src/b.rs"
        );

        let out = multi_patch(
            Some(dir.path().to_string_lossy().into_owned()),
            vec![
                PatchEdit {
                    file: "src/lib.rs".to_string(),
                    byte_start: lib.find("helper_fn").unwrap(),
                    byte_end: lib.find("helper_fn").unwrap() + helper_len,
                    new_content: "renamed_fn".to_string(),
                },
                PatchEdit {
                    file: "src/a.rs".to_string(),
                    byte_start: a_src.find("helper_fn").unwrap(),
                    byte_end: a_src.find("helper_fn").unwrap() + helper_len,
                    new_content: "renamed_fn".to_string(),
                },
                PatchEdit {
                    file: "src/b.rs".to_string(),
                    byte_start: b_first,
                    byte_end: b_first + helper_len,
                    new_content: "renamed_fn".to_string(),
                },
                PatchEdit {
                    file: "src/b.rs".to_string(),
                    byte_start: b_second,
                    byte_end: b_second + helper_len,
                    new_content: "renamed_fn".to_string(),
                },
            ],
        )
        .unwrap();

        let payload: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(payload["tool"], "multi_patch");
        assert_eq!(payload["files_written"], 3);
        assert_eq!(payload["edits_applied"], 4);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap(),
            "mod a;\nmod b;\npub use a::renamed_fn;\npub use b::call_value;\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
            "pub fn renamed_fn() -> i32 { 1 }\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/b.rs")).unwrap(),
            "use crate::renamed_fn;\npub fn call_value() -> i32 { renamed_fn() }\n"
        );
    }

    #[test]
    fn multi_patch_rejects_incomplete_cross_file_rust_rename_and_restores_files() {
        let dir = TempDir::new().unwrap();
        let cargo =
            "[package]\nname = \"multi-patch-guard\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let lib = "mod a;\nmod b;\npub use a::helper_fn;\npub use b::call_value;\n";
        let a_src = "pub fn helper_fn() -> i32 { 1 }\n";
        let b_src = "use crate::helper_fn;\npub fn call_value() -> i32 { helper_fn() }\n";
        write_file(&dir, "Cargo.toml", cargo);
        write_file(&dir, "src/lib.rs", lib);
        write_file(&dir, "src/a.rs", a_src);
        write_file(&dir, "src/b.rs", b_src);

        let helper_len = "helper_fn".len();
        let err = multi_patch(
            Some(dir.path().to_string_lossy().into_owned()),
            vec![
                PatchEdit {
                    file: "src/lib.rs".to_string(),
                    byte_start: lib.find("helper_fn").unwrap(),
                    byte_end: lib.find("helper_fn").unwrap() + helper_len,
                    new_content: "renamed_fn".to_string(),
                },
                PatchEdit {
                    file: "src/a.rs".to_string(),
                    byte_start: a_src.find("helper_fn").unwrap(),
                    byte_end: a_src.find("helper_fn").unwrap() + helper_len,
                    new_content: "renamed_fn".to_string(),
                },
            ],
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("cargo")
                || err.to_string().contains("cannot find"),
            "got: {}",
            err
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap(),
            lib
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
            a_src
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/b.rs")).unwrap(),
            b_src
        );
    }

    #[test]
    fn multi_patch_rejects_python_top_level_return() {
        let dir = TempDir::new().unwrap();
        let original = "x = 1\n";
        write_file(&dir, "main.py", original);

        let err = multi_patch(
            Some(dir.path().to_string_lossy().into_owned()),
            vec![PatchEdit {
                file: "main.py".to_string(),
                byte_start: 0,
                byte_end: 5,
                new_content: "return 1".to_string(),
            }],
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("python")
                || err.to_string().contains("outside function"),
            "got: {}",
            err
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("main.py")).unwrap(),
            original
        );
    }

    #[test]
    fn write_with_compiler_guard_rejects_python_top_level_return() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.py");
        write_file(&dir, "main.py", "x = 1\n");

        let err = write_with_compiler_guard(&root, &full_path, "main.py", "return 1\n", "py")
            .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("python")
                || err.to_string().contains("outside function"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), "x = 1\n");
    }

    #[test]
    fn write_bytes_with_compiler_guard_rejects_bash_syntax_error() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.sh");
        let original = "#!/usr/bin/env bash\nif true; then\n  echo ok\nfi\n";
        write_file(&dir, "main.sh", original);

        let err = write_bytes_with_compiler_guard(
            &root,
            &full_path,
            "main.sh",
            b"#!/usr/bin/env bash\nif true; then\n  echo ok\n",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("bash")
                || err.to_string().contains("syntax"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_javascript_syntax_error() {
        if !command_available("node", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.js");
        let original = "const answer = 1;\n";
        write_file(&dir, "main.js", original);

        let err = write_with_compiler_guard(&root, &full_path, "main.js", "const = 1;\n", "js")
            .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("node")
                || err.to_string().contains("SyntaxError"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_ruby_syntax_error() {
        if !command_available("ruby", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.rb");
        let original = "def greet\n  puts \"hi\"\nend\n";
        write_file(&dir, "main.rb", original);

        let err = write_with_compiler_guard(
            &root,
            &full_path,
            "main.rb",
            "def greet(\n  puts \"hi\"\nend\n",
            "rb",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("ruby")
                || err.to_string().contains("syntax"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_php_syntax_error() {
        if !command_available("php", "-v") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.php");
        let original = "<?php\nfunction greet() {\n    echo \"hi\";\n}\n";
        write_file(&dir, "main.php", original);

        let err = write_with_compiler_guard(
            &root,
            &full_path,
            "main.php",
            "<?php\nfunction greet( {\n    echo \"hi\";\n}\n",
            "php",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("compiler/interpreter")
                || err.to_string().contains("php")
                || err.to_string().contains("Parse error"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_python_runtime_smoke_error() {
        if !command_available("python3", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.py");
        let original = "print(\"ok\")\n";
        write_file(&dir, "main.py", original);
        write_runtime_config(
            &dir,
            r#"{
  "runtime_checks": [
    {
      "language": "python",
      "command": ["python3", "{{file}}"],
      "allow_unsandboxed": true,
      "kind": "python-runtime",
      "timeout_ms": 1000
    }
  ]
}"#,
        );

        let err = write_with_compiler_guard(
            &root,
            &full_path,
            "main.py",
            "import does_not_exist\n",
            "py",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("python-runtime")
                || err.to_string().contains("does_not_exist")
                || err.to_string().contains("ModuleNotFoundError"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_javascript_runtime_smoke_error() {
        if !command_available("node", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.js");
        let original = "console.log(\"ok\");\n";
        write_file(&dir, "main.js", original);
        write_runtime_config(
            &dir,
            r#"{
  "runtime_checks": [
    {
      "language": "javascript",
      "command": ["node", "{{file}}"],
      "allow_unsandboxed": true,
      "kind": "javascript-runtime",
      "timeout_ms": 1000
    }
  ]
}"#,
        );

        let err = write_with_compiler_guard(
            &root,
            &full_path,
            "main.js",
            "require(\"./missing\");\n",
            "js",
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("javascript-runtime")
                || err.to_string().contains("Cannot find module")
                || err.to_string().contains("./missing"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_skips_unsandboxed_runtime_without_opt_in() {
        if !command_available("python3", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.py");
        let updated = "import does_not_exist\n";
        write_file(&dir, "main.py", "print(\"ok\")\n");
        write_runtime_config(
            &dir,
            r#"{
  "runtime_checks": [
    {
      "language": "python",
      "command": ["python3", "{{file}}"],
      "kind": "python-runtime",
      "timeout_ms": 1000
    }
  ]
}"#,
        );

        write_with_compiler_guard(&root, &full_path, "main.py", updated, "py").unwrap();

        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), updated);
    }

    #[test]
    fn write_with_compiler_guard_rejects_inline_runtime_command_config() {
        if !command_available("python3", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.py");
        let original = "print(\"ok\")\n";
        write_file(&dir, "main.py", original);
        write_runtime_config(
            &dir,
            r#"{
  "runtime_checks": [
    {
      "language": "python",
      "command": ["python3", "-c", "print('hi')", "{{file}}"],
      "allow_unsandboxed": true
    }
  ]
}"#,
        );

        let err =
            write_with_compiler_guard(&root, &full_path, "main.py", "print(\"still ok\")\n", "py")
                .unwrap_err();

        assert!(
            err.to_string().contains("unsafe")
                || err.to_string().contains("inline interpreter code"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_runtime_command_without_file_placeholder() {
        if !command_available("python3", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.py");
        let original = "print(\"ok\")\n";
        write_file(&dir, "main.py", original);
        write_runtime_config(
            &dir,
            r#"{
  "runtime_checks": [
    {
      "language": "python",
      "command": ["python3", "-m", "http.server"],
      "allow_unsandboxed": true
    }
  ]
}"#,
        );

        let err =
            write_with_compiler_guard(&root, &full_path, "main.py", "print(\"still ok\")\n", "py")
                .unwrap_err();

        assert!(
            err.to_string().contains("must target the changed file")
                || err.to_string().contains("{{file}}")
                || err.to_string().contains("{{abs_file}}"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn write_with_compiler_guard_rejects_env_wrapped_inline_runtime_command() {
        if !command_available("python3", "--version") {
            return;
        }

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let full_path = root.join("main.py");
        let original = "print(\"ok\")\n";
        write_file(&dir, "main.py", original);
        write_runtime_config(
            &dir,
            r#"{
  "runtime_checks": [
    {
      "language": "python",
      "command": ["env", "python3", "-c", "print('hi')", "{{file}}"],
      "allow_unsandboxed": true
    }
  ]
}"#,
        );

        let err =
            write_with_compiler_guard(&root, &full_path, "main.py", "print(\"still ok\")\n", "py")
                .unwrap_err();

        assert!(
            err.to_string().contains("unsafe")
                || err.to_string().contains("inline interpreter code"),
            "got: {}",
            err
        );
        assert_eq!(std::fs::read_to_string(&full_path).unwrap(), original);
    }

    #[test]
    fn change_edit_by_symbol_wraps_patch_payload() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/lib.rs",
            "fn greet() {\n    println!(\"hi\");\n}\n",
        );
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
        assert_eq!(
            v["patched_source"],
            "fn greet() {\n    println!(\"bye\");\n}"
        );
    }

    #[test]
    fn change_rename_wraps_graph_payload() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/lib.rs",
            "pub fn greet() {}\nfn call() { greet(); }\n",
        );
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
        assert!(std::fs::read_to_string(dir.path().join("src/lib.rs"))
            .unwrap()
            .contains("salute"));
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
        assert!(!std::fs::read_to_string(dir.path().join("src/from.rs"))
            .unwrap()
            .contains("greet"));
        assert!(std::fs::read_to_string(dir.path().join("src/to.rs"))
            .unwrap()
            .contains("greet"));
    }

    #[test]
    fn change_delete_wraps_graph_delete_payload() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/lib.rs",
            "fn target() {}\nfn caller() { target(); }\n",
        );
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
        assert!(!std::fs::read_to_string(dir.path().join("src/lib.rs"))
            .unwrap()
            .contains("fn target"));
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
            Some(vec![crate::engine::Param {
                name: "value".into(),
                type_str: "i32".into(),
            }]),
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
            Some(vec![crate::engine::Param {
                name: "value".into(),
                type_str: "i32".into(),
            }]),
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
