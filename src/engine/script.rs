use anyhow::{anyhow, Result};
use rhai::{Dynamic, Engine, Map as RhaiMap};
use serde_json::{Map as JsonMap, Value};

use super::util::resolve_project_root;

fn call_to_dynamic(res: Result<String>) -> Dynamic {
    match res {
        Ok(s) => json_str_to_dynamic(&s),
        Err(e) => err_dynamic(&e.to_string()),
    }
}

fn json_str_to_dynamic(s: &str) -> Dynamic {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(v) => rhai::serde::to_dynamic(v).unwrap_or(Dynamic::UNIT),
        Err(e) => err_dynamic(&e.to_string()),
    }
}

fn err_dynamic(msg: &str) -> Dynamic {
    let mut map = rhai::Map::new();
    map.insert("error".into(), Dynamic::from(msg.to_string()));
    Dynamic::from(map)
}

fn json_args(args: RhaiMap) -> Result<JsonMap<String, Value>> {
    let value: Value = rhai::serde::from_dynamic(&Dynamic::from(args))
        .map_err(|e| anyhow!("Invalid script arguments: {}", e))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("Script arguments must be an object"))
}

fn str_opt(args: &JsonMap<String, Value>, key: &str) -> Option<String> {
    args.get(key)?.as_str().map(|v| v.to_string())
}

fn bool_opt(args: &JsonMap<String, Value>, key: &str) -> Option<bool> {
    args.get(key)?.as_bool()
}

fn uint_opt(args: &JsonMap<String, Value>, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        })
        .map(|n| n as usize)
}

fn parse_params(val: Option<&Value>) -> Option<Vec<crate::engine::Param>> {
    let arr = val?.as_array()?;
    let params: Vec<_> = arr
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let type_str = item.get("type")?.as_str()?.to_string();
            Some(crate::engine::Param { name, type_str })
        })
        .collect();
    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

fn parse_edits(args: &JsonMap<String, Value>) -> Result<Option<Vec<crate::engine::PatchEdit>>> {
    let Some(items) = args.get("edits").and_then(|v| v.as_array()) else {
        return Ok(None);
    };

    let mut edits = Vec::with_capacity(items.len());
    for item in items {
        let file = item
            .get("file")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Each change.edits item must have a 'file' field"))?
            .to_string();
        let byte_start = item
            .get("byte_start")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Each change.edits item must have a 'byte_start' field"))?
            as usize;
        let byte_end = item
            .get("byte_end")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Each change.edits item must have a 'byte_end' field"))?
            as usize;
        let new_content = item
            .get("new_content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Each change.edits item must have a 'new_content' field"))?
            .to_string();
        edits.push(crate::engine::PatchEdit {
            file,
            byte_start,
            byte_end,
            new_content,
        });
    }

    Ok(Some(edits))
}

fn register_bootstrap_and_discovery(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    {
        let rc = root.clone();
        engine.register_fn("boot", move || -> Dynamic {
            call_to_dynamic(crate::engine::llm_instructions(Some(rc.clone())))
        });
    }
    {
        let rc = root.clone();
        engine.register_fn("index", move || -> Dynamic {
            call_to_dynamic(crate::engine::bake(Some(rc.clone())))
        });
    }
    engine.register_fn("help", move |name: String| -> Dynamic {
        call_to_dynamic(crate::engine::tool_help(name))
    });
}

fn register_inspect_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    engine.register_fn("inspect", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::inspect(
                Some(root.clone()),
                str_opt(&args, "name"),
                str_opt(&args, "file"),
                uint_opt(&args, "start_line").map(|v| v as u32),
                uint_opt(&args, "end_line").map(|v| v as u32),
                bool_opt(&args, "include_source"),
                bool_opt(&args, "include_summaries"),
                uint_opt(&args, "limit"),
                bool_opt(&args, "stdlib"),
                bool_opt(&args, "signature_only"),
                bool_opt(&args, "type_only"),
                str_opt(&args, "depth"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_search_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    {
        let rc = root.clone();
        engine.register_fn("search", move |query: String| -> Dynamic {
            call_to_dynamic(crate::engine::supersearch(
                Some(rc.clone()),
                query,
                "all".to_string(),
                "all".to_string(),
                None,
                None,
                None,
            ))
        });
    }
    engine.register_fn("search", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            const MODES: &[&str] = &["all", "call", "assign", "return"];
            let args = json_args(args)?;
            let raw_pattern = str_opt(&args, "pattern");
            let query = if let Some(query) = str_opt(&args, "query") {
                query
            } else if let Some(ref pattern) = raw_pattern {
                if MODES.contains(&pattern.as_str()) {
                    return Err(anyhow!("Missing required 'query' argument for search"));
                }
                pattern.clone()
            } else {
                return Err(anyhow!("Missing required 'query' argument for search"));
            };
            let pattern = raw_pattern
                .filter(|p| MODES.contains(&p.as_str()))
                .unwrap_or_else(|| "all".to_string());
            crate::engine::supersearch(
                Some(root.clone()),
                query,
                str_opt(&args, "context").unwrap_or_else(|| "all".to_string()),
                pattern,
                bool_opt(&args, "exclude_tests"),
                str_opt(&args, "file"),
                uint_opt(&args, "limit"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_ask_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    {
        let rc = root.clone();
        engine.register_fn("ask", move |query: String| -> Dynamic {
            call_to_dynamic(crate::engine::semantic_search(
                Some(rc.clone()),
                query,
                None,
                None,
                None,
            ))
        });
    }
    engine.register_fn("ask", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::semantic_search(
                Some(root.clone()),
                str_opt(&args, "query")
                    .ok_or_else(|| anyhow!("Missing required 'query' argument for ask"))?,
                uint_opt(&args, "limit"),
                str_opt(&args, "file"),
                str_opt(&args, "scope"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_judge_change_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    engine.register_fn("judge_change", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::judge_change(
                Some(root.clone()),
                str_opt(&args, "query")
                    .ok_or_else(|| anyhow!("Missing required 'query' argument for judge_change"))?,
                str_opt(&args, "symbol"),
                str_opt(&args, "file"),
                uint_opt(&args, "limit"),
                str_opt(&args, "scope"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_map_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    {
        let rc = root.clone();
        engine.register_fn("map", move |intent: String| -> Dynamic {
            call_to_dynamic(crate::engine::architecture_map(
                Some(rc.clone()),
                Some(intent),
                None,
            ))
        });
    }
    engine.register_fn("map", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::architecture_map(
                Some(root.clone()),
                str_opt(&args, "intent"),
                uint_opt(&args, "limit"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_routes_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    {
        let rc = root.clone();
        engine.register_fn("routes", move || -> Dynamic {
            call_to_dynamic(crate::engine::all_endpoints(
                Some(rc.clone()),
                None,
                None,
                None,
                None,
            ))
        });
    }
    engine.register_fn("routes", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::all_endpoints(
                Some(root.clone()),
                str_opt(&args, "query"),
                str_opt(&args, "method"),
                str_opt(&args, "scope"),
                uint_opt(&args, "limit"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_impact_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    engine.register_fn("impact", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::impact(
                Some(root.clone()),
                str_opt(&args, "symbol"),
                str_opt(&args, "endpoint"),
                str_opt(&args, "method"),
                uint_opt(&args, "depth"),
                bool_opt(&args, "include_source"),
                str_opt(&args, "scope"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_health_tool(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    {
        let rc = root.clone();
        engine.register_fn("health", move || -> Dynamic {
            call_to_dynamic(crate::engine::health(
                Some(rc.clone()),
                None,
                None,
                None,
                None,
            ))
        });
    }
    engine.register_fn("health", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::health(
                Some(root.clone()),
                uint_opt(&args, "top"),
                str_opt(&args, "view"),
                uint_opt(&args, "limit"),
                str_opt(&args, "cursor"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn register_read_tools(engine: &mut Engine, root: &str) {
    register_inspect_tool(engine, root);
    register_search_tool(engine, root);
    register_ask_tool(engine, root);
    register_judge_change_tool(engine, root);
    register_map_tool(engine, root);
    register_routes_tool(engine, root);
    register_impact_tool(engine, root);
    register_health_tool(engine, root);
}

fn register_write_tools(engine: &mut Engine, root: &str) {
    let root = root.to_string();
    let rc = root.clone();
    engine.register_fn("change", move |args: RhaiMap| -> Dynamic {
        let res = (|| {
            let args = json_args(args)?;
            crate::engine::change(
                Some(rc.clone()),
                str_opt(&args, "action")
                    .ok_or_else(|| anyhow!("Missing required 'action' argument for change"))?,
                str_opt(&args, "name"),
                str_opt(&args, "file"),
                uint_opt(&args, "start_line").map(|v| v as u32),
                uint_opt(&args, "end_line").map(|v| v as u32),
                str_opt(&args, "new_content"),
                str_opt(&args, "old_string"),
                str_opt(&args, "new_string"),
                uint_opt(&args, "match_index"),
                parse_edits(&args)?,
                str_opt(&args, "new_name"),
                str_opt(&args, "to_file"),
                bool_opt(&args, "force"),
                str_opt(&args, "function_name"),
                str_opt(&args, "entity_type"),
                str_opt(&args, "after_symbol"),
                str_opt(&args, "language"),
                parse_params(args.get("params")),
                str_opt(&args, "returns"),
                str_opt(&args, "on"),
            )
        })();
        call_to_dynamic(res)
    });
}

fn build_script_engine(root: &str) -> Engine {
    let mut engine = Engine::new();
    register_bootstrap_and_discovery(&mut engine, root);
    register_read_tools(&mut engine, root);
    register_write_tools(&mut engine, root);
    engine
}

fn execute_script(engine: &Engine, code: &str) -> Result<Dynamic> {
    engine
        .eval(&code)
        .map_err(|e| anyhow::anyhow!("Script error: {}", e))
}

fn render_script_result(root: &std::path::Path, result: Dynamic) -> Result<String> {
    let json_val: serde_json::Value = rhai::serde::from_dynamic(&result)
        .map_err(|e| anyhow::anyhow!("Failed to serialize result: {}", e))?;

    let payload = serde_json::json!({
        "tool": "script",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "result": json_val,
    });

    Ok(serde_json::to_string_pretty(&payload)?)
}

pub fn run_script(path: Option<String>, code: String) -> Result<String> {
    let root = resolve_project_root(path)?;
    let root_str = root.to_string_lossy().into_owned();
    let engine = build_script_engine(&root_str);
    let result = execute_script(&engine, &code)?;
    render_script_result(&root, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_script_pure_expr() {
        let result = run_script(None, "40 + 2".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        assert_eq!(v["result"], 42);
    }

    #[test]
    fn test_run_script_map_result() {
        let result = run_script(None, r#"let m = #{x: 1, y: "hello"}; m"#.to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        assert_eq!(v["result"]["x"], 1);
        assert_eq!(v["result"]["y"], "hello");
    }

    #[test]
    fn test_run_script_syntax_error() {
        let err = run_script(None, "let x = ".to_string()).unwrap_err();
        assert!(err.to_string().contains("Script error"));
    }

    #[test]
    fn test_run_script_err_dynamic_propagates() {
        // A function that returns an error map does not panic — result is still valid JSON.
        let result = run_script(
            None,
            // impact on a non-existent symbol: returns {"error": "..."} map
            r#"impact(#{symbol: "__no_such_symbol__"})"#.to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        // result should be an object (either a valid result or an error map)
        assert!(v["result"].is_object());
    }

    #[test]
    fn test_run_script_uses_task_surface() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        let result = run_script(
            Some(dir.path().to_string_lossy().into_owned()),
            r#"inspect(#{name: "main", include_source: true})"#.to_string(),
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        assert_eq!(v["result"]["tool"], "inspect");
        assert_eq!(v["result"]["mode"], "symbol");
    }

    #[test]
    fn test_run_script_hides_legacy_mechanism_functions() {
        let err = run_script(None, r#"symbol("main")"#.to_string()).unwrap_err();
        assert!(err.to_string().contains("Function not found"));
        assert!(err.to_string().contains("symbol"));
    }
}
