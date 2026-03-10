use anyhow::Result;
use rhai::{Dynamic, Engine};

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

pub fn run_script(path: Option<String>, code: String) -> Result<String> {
    let root = resolve_project_root(path)?;
    let r = root.to_string_lossy().into_owned();

    let mut engine = Engine::new();

    // --- Read tools ---
    {
        let rc = r.clone();
        engine.register_fn("symbol", move |name: String| -> Dynamic {
            call_to_dynamic(crate::engine::symbol(Some(rc.clone()), name, true, None, None, false))
        });
    }
    {
        let rc = r.clone();
        engine.register_fn("blast_radius", move |name: String| -> Dynamic {
            call_to_dynamic(crate::engine::blast_radius(Some(rc.clone()), name, None))
        });
    }
    {
        let rc = r.clone();
        engine.register_fn("health", move || -> Dynamic {
            call_to_dynamic(crate::engine::health(Some(rc.clone()), None, None, None, None))
        });
    }
    {
        let rc = r.clone();
        engine.register_fn("supersearch", move |query: String| -> Dynamic {
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
    {
        let rc = r.clone();
        engine.register_fn("file_functions", move |file: String| -> Dynamic {
            call_to_dynamic(crate::engine::file_functions(Some(rc.clone()), file, None))
        });
    }
    {
        let rc = r.clone();
        engine.register_fn("flow", move |endpoint: String, method: String| -> Dynamic {
            call_to_dynamic(crate::engine::flow(
                Some(rc.clone()),
                endpoint,
                Some(method),
                None,
                false,
            ))
        });
    }
    {
        let rc = r.clone();
        engine.register_fn(
            "slice",
            move |file: String, start: i64, end: i64| -> Dynamic {
                call_to_dynamic(crate::engine::slice(
                    Some(rc.clone()),
                    file,
                    start as u32,
                    end as u32,
                ))
            },
        );
    }

    // --- Write tools (deletion only) ---
    {
        let rc = r.clone();
        engine.register_fn("graph_delete", move |name: String| -> Dynamic {
            call_to_dynamic(crate::engine::graph_delete(
                Some(rc.clone()),
                name,
                None,
                false,
            ))
        });
    }

    let result: Dynamic = engine
        .eval(&code)
        .map_err(|e| anyhow::anyhow!("Script error: {}", e))?;

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
        let result = run_script(
            None,
            r#"let m = #{x: 1, y: "hello"}; m"#.to_string(),
        )
        .unwrap();
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
            // blast_radius on a non-existent symbol: returns {"error": "..."} map
            r#"blast_radius("__no_such_symbol__")"#.to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        // result should be an object (either a valid result or an error map)
        assert!(v["result"].is_object());
    }
}
