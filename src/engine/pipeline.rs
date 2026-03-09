use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_path ────────────────────────────────────────────────────────────

    #[test]
    fn parse_path_simple_key() {
        let parts = parse_path("foo");
        assert_eq!(parts.len(), 1);
        assert!(matches!(&parts[0], PathPart::Key(k) if k == "foo"));
    }

    #[test]
    fn parse_path_dotted_keys() {
        let parts = parse_path("s1.results");
        assert_eq!(parts.len(), 2);
        assert!(matches!(&parts[0], PathPart::Key(k) if k == "s1"));
        assert!(matches!(&parts[1], PathPart::Key(k) if k == "results"));
    }

    #[test]
    fn parse_path_array_index() {
        let parts = parse_path("s1.matches[0].name");
        assert_eq!(parts.len(), 4);
        assert!(matches!(&parts[0], PathPart::Key(k) if k == "s1"));
        assert!(matches!(&parts[1], PathPart::Key(k) if k == "matches"));
        assert!(matches!(&parts[2], PathPart::Index(0)));
        assert!(matches!(&parts[3], PathPart::Key(k) if k == "name"));
    }

    #[test]
    fn parse_path_root_array_index() {
        // path starts directly with bracket: "[0]" — unlikely but shouldn't panic
        let parts = parse_path("[0]");
        assert_eq!(parts.len(), 1);
        assert!(matches!(&parts[0], PathPart::Index(0)));
    }

    #[test]
    fn parse_path_multi_index() {
        let parts = parse_path("s1.arr[2][1]");
        assert_eq!(parts.len(), 4);
        assert!(matches!(&parts[2], PathPart::Index(2)));
        assert!(matches!(&parts[3], PathPart::Index(1)));
    }

    // ── resolve_ref ───────────────────────────────────────────────────────────

    fn ctx(entries: &[(&str, Value)]) -> HashMap<String, Value> {
        entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn resolve_ref_simple_field() {
        let c = ctx(&[("s1", json!({"name": "add"}))]);
        let val = resolve_ref("s1.name", &c);
        assert_eq!(val, Some(json!("add")));
    }

    #[test]
    fn resolve_ref_array_index() {
        let c = ctx(&[("s1", json!({"matches": [{"name": "foo"}, {"name": "bar"}]}))]);
        let val = resolve_ref("s1.matches[0].name", &c);
        assert_eq!(val, Some(json!("foo")));
    }

    #[test]
    fn resolve_ref_second_array_element() {
        let c = ctx(&[("s1", json!({"matches": [{"name": "foo"}, {"name": "bar"}]}))]);
        let val = resolve_ref("s1.matches[1].name", &c);
        assert_eq!(val, Some(json!("bar")));
    }

    #[test]
    fn resolve_ref_missing_step_returns_none() {
        let c = ctx(&[]);
        assert_eq!(resolve_ref("s1.name", &c), None);
    }

    #[test]
    fn resolve_ref_missing_field_returns_none() {
        let c = ctx(&[("s1", json!({"foo": 1}))]);
        assert_eq!(resolve_ref("s1.bar", &c), None);
    }

    #[test]
    fn resolve_ref_out_of_bounds_index_returns_none() {
        let c = ctx(&[("s1", json!({"items": [1, 2]}))]);
        assert_eq!(resolve_ref("s1.items[5]", &c), None);
    }

    #[test]
    fn resolve_ref_strips_pipe_filter() {
        // The | part is for conditions; resolve_ref should ignore it
        let c = ctx(&[("s1", json!({"results": [1, 2, 3]}))]);
        let val = resolve_ref("s1.results | length == 3", &c);
        assert_eq!(val, Some(json!([1, 2, 3])));
    }

    // ── resolve_value ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_value_whole_string_ref_preserves_type() {
        let c = ctx(&[("s1", json!({"count": 42}))]);
        let result = resolve_value(json!("{{s1.count}}"), &c);
        assert_eq!(result, json!(42));
    }

    #[test]
    fn resolve_value_interpolated_string() {
        let c = ctx(&[("s1", json!({"name": "add"}))]);
        let result = resolve_value(json!("fn {{s1.name}}()"), &c);
        assert_eq!(result, json!("fn add()"));
    }

    #[test]
    fn resolve_value_multiple_refs_in_string() {
        let c = ctx(&[("a", json!({"x": "foo"})), ("b", json!({"y": "bar"}))]);
        let result = resolve_value(json!("{{a.x}}-{{b.y}}"), &c);
        assert_eq!(result, json!("foo-bar"));
    }

    #[test]
    fn resolve_value_object_resolves_recursively() {
        let c = ctx(&[("s1", json!({"name": "multiply"}))]);
        let result = resolve_value(json!({"query": "{{s1.name}}"}), &c);
        assert_eq!(result["query"], json!("multiply"));
    }

    #[test]
    fn resolve_value_array_resolves_recursively() {
        let c = ctx(&[("s1", json!({"v": "x"}))]);
        let result = resolve_value(json!(["{{s1.v}}", "literal"]), &c);
        assert_eq!(result[0], json!("x"));
        assert_eq!(result[1], json!("literal"));
    }

    #[test]
    fn resolve_value_unresolved_ref_kept_as_is() {
        let c = ctx(&[]);
        let result = resolve_value(json!("{{missing.field}}"), &c);
        // Should remain a string with the original template
        assert_eq!(result, json!("{{missing.field}}"));
    }

    #[test]
    fn resolve_value_non_string_passthrough() {
        let c = ctx(&[]);
        assert_eq!(resolve_value(json!(42), &c), json!(42));
        assert_eq!(resolve_value(json!(true), &c), json!(true));
        assert_eq!(resolve_value(json!(null), &c), json!(null));
    }

    // ── eval_condition ────────────────────────────────────────────────────────

    #[test]
    fn eval_condition_truthy_number() {
        let c = ctx(&[("s1", json!({"count": 5}))]);
        assert!(eval_condition("{{s1.count}}", &c));
    }

    #[test]
    fn eval_condition_falsy_zero() {
        let c = ctx(&[("s1", json!({"count": 0}))]);
        assert!(!eval_condition("{{s1.count}}", &c));
    }

    #[test]
    fn eval_condition_truthy_non_empty_array() {
        let c = ctx(&[("s1", json!({"items": [1, 2]}))]);
        assert!(eval_condition("{{s1.items}}", &c));
    }

    #[test]
    fn eval_condition_falsy_empty_array() {
        let c = ctx(&[("s1", json!({"items": []}))]);
        assert!(!eval_condition("{{s1.items}}", &c));
    }

    #[test]
    fn eval_condition_missing_ref_is_false() {
        let c = ctx(&[]);
        assert!(!eval_condition("{{missing.field}}", &c));
    }

    #[test]
    fn eval_condition_length_eq_zero_on_empty_array() {
        let c = ctx(&[("s1", json!({"callers": []}))]);
        assert!(eval_condition("{{s1.callers | length == 0}}", &c));
    }

    #[test]
    fn eval_condition_length_eq_zero_on_nonempty_array() {
        let c = ctx(&[("s1", json!({"callers": ["x", "y"]}))]);
        assert!(!eval_condition("{{s1.callers | length == 0}}", &c));
    }

    #[test]
    fn eval_condition_length_gt() {
        let c = ctx(&[("s1", json!({"items": [1, 2, 3]}))]);
        assert!(eval_condition("{{s1.items | length > 2}}", &c));
        assert!(!eval_condition("{{s1.items | length > 3}}", &c));
    }

    #[test]
    fn eval_condition_length_ne() {
        let c = ctx(&[("s1", json!({"items": [1, 2]}))]);
        assert!(eval_condition("{{s1.items | length != 0}}", &c));
        assert!(!eval_condition("{{s1.items | length != 2}}", &c));
    }

    #[test]
    fn eval_condition_length_gte() {
        let c = ctx(&[("s1", json!({"items": [1, 2, 3]}))]);
        assert!(eval_condition("{{s1.items | length >= 3}}", &c));
        assert!(eval_condition("{{s1.items | length >= 2}}", &c));
        assert!(!eval_condition("{{s1.items | length >= 4}}", &c));
    }

    #[test]
    fn eval_condition_length_lte() {
        let c = ctx(&[("s1", json!({"items": [1]}))]);
        assert!(eval_condition("{{s1.items | length <= 1}}", &c));
        assert!(!eval_condition("{{s1.items | length <= 0}}", &c));
    }

    #[test]
    fn eval_condition_length_on_string() {
        let c = ctx(&[("s1", json!({"name": "hello"}))]);
        assert!(eval_condition("{{s1.name | length == 5}}", &c));
        assert!(!eval_condition("{{s1.name | length == 3}}", &c));
    }

    #[test]
    fn eval_condition_no_braces_falls_through_as_true() {
        // If no {{}} wrapping, inner resolve_ref finds nothing — returns false
        let c = ctx(&[]);
        // A bare string that isn't a ref — treated as a ref lookup that fails
        assert!(!eval_condition("{{no_such_step.field}}", &c));
    }

    // ── pipeline() unit-level (no bake needed) ────────────────────────────────

    #[test]
    fn pipeline_empty_spec_returns_empty_steps() {
        let out = pipeline(None, json!([])).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["steps"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn pipeline_rejects_non_array_spec() {
        let err = pipeline(None, json!({"tool": "shake"})).unwrap_err();
        assert!(err.to_string().contains("array"), "expected array error, got: {}", err);
    }

    #[test]
    fn pipeline_step_missing_id_returns_error() {
        let err = pipeline(None, json!([{"tool": "shake"}])).unwrap_err();
        assert!(err.to_string().contains("id"), "expected id error, got: {}", err);
    }

    #[test]
    fn pipeline_step_missing_tool_returns_error() {
        let err = pipeline(None, json!([{"id": "s1"}])).unwrap_err();
        assert!(err.to_string().contains("tool"), "expected tool error, got: {}", err);
    }

    #[test]
    fn pipeline_unknown_tool_produces_error_step_and_stops() {
        let out = pipeline(
            None,
            json!([
                {"id": "s1", "tool": "nonexistent_tool_xyz"},
                {"id": "s2", "tool": "shake"}
            ]),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let steps = v["steps"].as_array().unwrap();
        // Only s1 ran (errored), s2 never started
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["id"], "s1");
        assert_eq!(steps[0]["ok"], false);
        assert!(steps[0]["error"].as_str().unwrap().contains("nonexistent_tool_xyz"));
    }

    #[test]
    fn pipeline_false_condition_marks_step_skipped() {
        // Condition references an empty ctx — evaluates false
        let out = pipeline(
            None,
            json!([
                {"id": "s1", "tool": "shake", "if": "{{missing.val}}"}
            ]),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let steps = v["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["skipped"], true);
    }

    #[test]
    fn pipeline_skipped_step_does_not_stop_pipeline() {
        // s1 skipped, s2 (unknown tool) should still attempt and error
        let out = pipeline(
            None,
            json!([
                {"id": "s1", "tool": "shake", "if": "{{nothing}}"},
                {"id": "s2", "tool": "bad_tool"}
            ]),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let steps = v["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["skipped"], true);
        assert_eq!(steps[1]["ok"], false);
    }
}

/// Execute a sequential pipeline spec — the yoyo answer to Code Mode.
///
/// `spec` is a JSON array of step objects. Each step:
/// ```json
/// {
///   "id": "s1",
///   "tool": "symbol",
///   "args": { "name": "my_func", "include_source": true },
///   "if": "{{s0.results | length > 0}}"
/// }
/// ```
/// Output refs in args use `{{step_id.field[N].subfield}}` syntax.
/// Conditions use `{{expr | predicate}}` where predicate is `length == N`, etc.
pub fn pipeline(path: Option<String>, spec: Value) -> Result<String> {
    let steps = spec
        .as_array()
        .ok_or_else(|| anyhow!("pipeline spec must be a JSON array of steps"))?;

    let mut ctx: HashMap<String, Value> = HashMap::new();
    let mut output: Vec<Value> = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        let id = step
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("step {} missing required 'id'", i))?;
        let tool = step
            .get("tool")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("step '{}' missing required 'tool'", id))?;

        // Evaluate condition — skip step if false
        if let Some(cond_str) = step.get("if").and_then(|v| v.as_str()) {
            if !eval_condition(cond_str, &ctx) {
                output.push(json!({"id": id, "tool": tool, "skipped": true, "condition": cond_str}));
                continue;
            }
        }

        // Resolve template refs in args
        let raw_args = step.get("args").cloned().unwrap_or(json!({}));
        let resolved_args = resolve_value(raw_args, &ctx);

        // Step-level path override, fall back to pipeline-level path
        let step_path = resolved_args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| path.clone());

        match dispatch(tool, &resolved_args, step_path) {
            Ok(result_str) => {
                let result_val: Value = serde_json::from_str(&result_str)
                    .unwrap_or(Value::String(result_str));
                ctx.insert(id.to_string(), result_val.clone());
                output.push(json!({"id": id, "tool": tool, "ok": true, "result": result_val}));
            }
            Err(e) => {
                let msg = e.to_string();
                output.push(json!({"id": id, "tool": tool, "ok": false, "error": msg}));
                break;
            }
        }
    }

    Ok(serde_json::to_string_pretty(&json!({"steps": output}))?)
}

// ── Template resolution ────────────────────────────────────────────────────────

/// Recursively resolve all `{{...}}` references in a JSON value.
fn resolve_value(val: Value, ctx: &HashMap<String, Value>) -> Value {
    match val {
        Value::String(s) => {
            // Single whole-string reference → preserve original type
            if s.starts_with("{{") && s.ends_with("}}") && s.matches("{{").count() == 1 {
                let inner = s[2..s.len() - 2].trim();
                resolve_ref(inner, ctx).unwrap_or(Value::String(s))
            } else {
                Value::String(replace_templates(&s, ctx))
            }
        }
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, resolve_value(v, ctx))).collect())
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(|v| resolve_value(v, ctx)).collect()),
        other => other,
    }
}

/// Replace all `{{...}}` occurrences in a string with their string representations.
fn replace_templates(s: &str, ctx: &HashMap<String, Value>) -> String {
    let mut result = s.to_string();
    loop {
        let start = match result.find("{{") {
            Some(p) => p,
            None => break,
        };
        let end_offset = match result[start..].find("}}") {
            Some(p) => p,
            None => break,
        };
        let inner = result[start + 2..start + end_offset].trim();
        let replacement = resolve_ref(inner, ctx)
            .map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            })
            .unwrap_or_else(|| format!("{{{{{}}}}}", inner));
        result = format!("{}{}{}", &result[..start], replacement, &result[start + end_offset + 2..]);
    }
    result
}

/// Resolve an expression like `step_id.field[0].subfield` (ignoring any `| predicate` tail).
fn resolve_ref(expr: &str, ctx: &HashMap<String, Value>) -> Option<Value> {
    let path_part = if let Some(p) = expr.find('|') { expr[..p].trim() } else { expr.trim() };
    let parts = parse_path(path_part);
    if parts.is_empty() {
        return None;
    }
    let first_key = match &parts[0] {
        PathPart::Key(k) => k.as_str(),
        PathPart::Index(_) => return None,
    };
    let mut current = ctx.get(first_key)?.clone();
    for part in &parts[1..] {
        current = navigate(&current, part)?;
    }
    Some(current)
}

#[derive(Debug)]
enum PathPart {
    Key(String),
    Index(usize),
}

/// Parse `a.b[0].c` into `[Key("a"), Key("b"), Index(0), Key("c")]`.
fn parse_path(path: &str) -> Vec<PathPart> {
    let mut parts = Vec::new();
    for segment in path.split('.') {
        if let Some(bracket) = segment.find('[') {
            let key = &segment[..bracket];
            if !key.is_empty() {
                parts.push(PathPart::Key(key.to_string()));
            }
            let rest = &segment[bracket..];
            let chars: Vec<char> = rest.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '[' {
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != ']' {
                        i += 1;
                    }
                    let idx_str: String = chars[start..i].iter().collect();
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        parts.push(PathPart::Index(idx));
                    }
                    i += 1; // skip ']'
                } else {
                    i += 1;
                }
            }
        } else if !segment.is_empty() {
            parts.push(PathPart::Key(segment.to_string()));
        }
    }
    parts
}

fn navigate(val: &Value, part: &PathPart) -> Option<Value> {
    match part {
        PathPart::Key(k) => val.get(k).cloned(),
        PathPart::Index(i) => val.as_array().and_then(|arr| arr.get(*i)).cloned(),
    }
}

// ── Condition evaluation ───────────────────────────────────────────────────────

/// Evaluate `{{expr | predicate}}` or `{{expr}}` (truthy check).
fn eval_condition(cond: &str, ctx: &HashMap<String, Value>) -> bool {
    let inner = cond
        .trim()
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
        .unwrap_or(cond.trim());

    if let Some(pipe_idx) = inner.find('|') {
        let path_part = inner[..pipe_idx].trim();
        let predicate = inner[pipe_idx + 1..].trim();
        let val = resolve_ref(path_part, ctx);

        if predicate.starts_with("length") {
            let len = val
                .as_ref()
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .or_else(|| val.as_ref().and_then(|v| v.as_str()).map(|s| s.len()))
                .unwrap_or(0);
            eval_num_predicate(predicate.trim_start_matches("length").trim(), len)
        } else {
            // Unknown filter — default true
            true
        }
    } else {
        is_truthy(resolve_ref(inner, ctx).as_ref())
    }
}

/// Evaluate `== N`, `!= N`, `> N`, `>= N`, `< N`, `<= N`.
fn eval_num_predicate(op_and_val: &str, actual: usize) -> bool {
    let s = op_and_val.trim();
    if let Some(rest) = s.strip_prefix("==") {
        actual == rest.trim().parse().unwrap_or(usize::MAX)
    } else if let Some(rest) = s.strip_prefix("!=") {
        actual != rest.trim().parse().unwrap_or(usize::MAX)
    } else if let Some(rest) = s.strip_prefix(">=") {
        actual >= rest.trim().parse().unwrap_or(usize::MAX)
    } else if let Some(rest) = s.strip_prefix(">") {
        actual > rest.trim().parse().unwrap_or(usize::MAX)
    } else if let Some(rest) = s.strip_prefix("<=") {
        actual <= rest.trim().parse().unwrap_or(usize::MAX)
    } else if let Some(rest) = s.strip_prefix("<") {
        actual < rest.trim().parse().unwrap_or(usize::MAX)
    } else {
        true
    }
}

fn is_truthy(val: Option<&Value>) -> bool {
    match val {
        None => false,
        Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

// ── Tool dispatcher ────────────────────────────────────────────────────────────

fn dispatch(tool: &str, args: &Value, path: Option<String>) -> Result<String> {
    // Helpers to extract args
    let s = |key: &str| -> Option<String> {
        args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    };
    let s_req = |key: &str| -> Result<String> {
        s(key).ok_or_else(|| anyhow!("tool '{}' requires arg '{}'", tool, key))
    };
    let b = |key: &str| -> Option<bool> { args.get(key).and_then(|v| v.as_bool()) };
    let u = |key: &str| -> Option<usize> {
        args.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
    };

    match tool {
        "llm_instructions" => crate::engine::llm_instructions(path),
        "shake" => crate::engine::shake(path),
        "bake" => crate::engine::bake(path),
        "all_endpoints" => crate::engine::all_endpoints(path),
        "api_surface" => crate::engine::api_surface(path, s("package"), u("limit")),
        "architecture_map" => crate::engine::architecture_map(path, s("intent")),
        "crud_operations" => crate::engine::crud_operations(path, s("entity")),
        "health" => crate::engine::health(path, u("top")),
        "symbol" => crate::engine::symbol(
            path,
            s_req("name")?,
            b("include_source").unwrap_or(false),
            s("file"),
            u("limit"),
        ),
        "flow" => crate::engine::flow(
            path,
            s_req("endpoint")?,
            s("method"),
            u("depth"),
            b("include_source").unwrap_or(false),
        ),
        "slice" => crate::engine::slice(
            path,
            s_req("file")?,
            u("start_line")
                .ok_or_else(|| anyhow!("slice requires 'start_line'"))? as u32,
            u("end_line")
                .ok_or_else(|| anyhow!("slice requires 'end_line'"))? as u32,
        ),
        "file_functions" => crate::engine::file_functions(path, s_req("file")?, b("include_summaries")),
        "supersearch" => crate::engine::supersearch(
            path,
            s_req("query")?,
            s("context").unwrap_or_else(|| "all".to_string()),
            s("pattern").unwrap_or_else(|| "all".to_string()),
            b("exclude_tests"),
            s("file"),
            u("limit"),
        ),
        "package_summary" => crate::engine::package_summary(path, s("package")),
        "suggest_placement" => crate::engine::suggest_placement(
            path,
            s_req("function_name")?,
            s_req("function_type")?,
            s("related_to"),
        ),
        "api_trace" => crate::engine::api_trace(path, s_req("endpoint")?, s("method")),
        "find_docs" => crate::engine::find_docs(path, s("doc_type"), u("limit")),
        "patch" => {
            if let Some(old_string) = s("old_string") {
                crate::engine::patch_string(path, s_req("file")?, old_string, s_req("new_string")?)
            } else if let Some(name) = s("name") {
                crate::engine::patch_by_symbol(path, name, s_req("new_content")?, u("match_index"))
            } else {
                crate::engine::patch(
                    path,
                    s_req("file")?,
                    u("start").ok_or_else(|| anyhow!("patch requires 'start'"))? as u32,
                    u("end").ok_or_else(|| anyhow!("patch requires 'end'"))? as u32,
                    s_req("new_content")?,
                )
            }
        }
        "patch_bytes" => crate::engine::patch_bytes(
            path,
            s_req("file")?,
            u("byte_start").ok_or_else(|| anyhow!("patch_bytes requires 'byte_start'"))?,
            u("byte_end").ok_or_else(|| anyhow!("patch_bytes requires 'byte_end'"))?,
            s_req("new_content")?,
        ),
        "multi_patch" => {
            let edits_val = args
                .get("edits")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("multi_patch requires 'edits' array"))?;
            let mut edits = Vec::new();
            for item in edits_val {
                let file = item
                    .get("file")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("each edit must have 'file'"))?
                    .to_string();
                let byte_start = item
                    .get("byte_start")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow!("each edit must have 'byte_start'"))? as usize;
                let byte_end = item
                    .get("byte_end")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow!("each edit must have 'byte_end'"))? as usize;
                let new_content = item
                    .get("new_content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("each edit must have 'new_content'"))?
                    .to_string();
                edits.push(crate::engine::PatchEdit { file, byte_start, byte_end, new_content });
            }
            crate::engine::multi_patch(path, edits)
        }
        "blast_radius" => {
            crate::engine::blast_radius(path, s_req("symbol")?, u("depth"))
        }
        "graph_rename" => {
            crate::engine::graph_rename(path, s_req("name")?, s_req("new_name")?)
        }
        "graph_create" => {
            crate::engine::graph_create(path, s_req("file")?, s_req("function_name")?, s("language"), None, None)
        }
        "graph_add" => crate::engine::graph_add(
            path,
            s_req("entity_type")?,
            s_req("name")?,
            s_req("file")?,
            s("after_symbol"),
            s("language"),
            None, None, None,
        ),
        "graph_move" => crate::engine::graph_move(path, s_req("name")?, s_req("to_file")?),
        "trace_down" => crate::engine::trace_down(path, s_req("name")?, u("depth"), s("file")),
        "graph_delete" => {
            crate::engine::graph_delete(path, s_req("name")?, s("file"), b("force").unwrap_or(false))
        }
        "semantic_search" => {
            crate::engine::semantic_search(path, s_req("query")?, u("limit"), s("file"))
        }
        other => Err(anyhow!("unknown tool '{}' — check tool name against llm_instructions catalog", other)),
    }
}
