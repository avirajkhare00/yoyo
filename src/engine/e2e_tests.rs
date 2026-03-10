/// E2E tests against the version-controlled fixture at tests/fixtures/sample_project.
///
/// Every test copies the fixture into a TempDir so mutations don't affect the
/// source tree and tests can run in parallel without clobbering each other.
///
/// No AI inference — every assertion is on deterministic tool output.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// Absolute path to the checked-in fixture.
    fn fixture_src() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sample_project")
    }

    /// Copy the fixture into a fresh TempDir and bake it. Returns the TempDir
    /// (must be kept alive for the duration of the test).
    fn setup() -> TempDir {
        let dir = TempDir::new().unwrap();
        copy_dir_recursive(&fixture_src(), dir.path());
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        dir
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                copy_dir_recursive(&src_path, &dst_path);
            } else {
                fs::copy(&src_path, &dst_path).unwrap();
            }
        }
    }

    fn root(dir: &TempDir) -> Option<String> {
        Some(dir.path().to_string_lossy().into_owned())
    }

    // ── symbol ────────────────────────────────────────────────────────────────

    #[test]
    fn e2e_symbol_finds_function_in_correct_file() {
        let dir = setup();
        let out = crate::engine::symbol(root(&dir), "add".into(), false, None, None, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(!matches.is_empty(), "expected at least one match for 'add'");
        let m = &matches[0];
        assert!(
            m["file"].as_str().unwrap().contains("math.rs"),
            "expected add() to be in math.rs, got: {}",
            m["file"]
        );
    }

    #[test]
    fn e2e_symbol_returns_all_functions_in_fixture() {
        let dir = setup();
        // math.rs: add, subtract, multiply, square
        // utils.rs: sum_three, clamp, format_result
        for name in &["add", "subtract", "multiply", "square", "sum_three", "clamp", "format_result"] {
            let out = crate::engine::symbol(root(&dir), name.to_string(), false, None, None, false).unwrap();
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            let matches = v["matches"].as_array().unwrap();
            assert!(
                !matches.is_empty(),
                "expected match for '{}', got none",
                name
            );
        }
    }

    // ── blast_radius ──────────────────────────────────────────────────────────

    #[test]
    fn e2e_blast_radius_finds_direct_caller() {
        let dir = setup();
        // square() calls multiply() — so multiply's blast radius should include square
        let out = crate::engine::blast_radius(root(&dir), "multiply".into(), Some(1)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let callers: Vec<&str> = v["callers"]
            .as_array().unwrap()
            .iter()
            .map(|c| c["caller"].as_str().unwrap())
            .collect();
        assert!(
            callers.contains(&"square"),
            "expected 'square' in blast_radius of 'multiply', got: {:?}",
            callers
        );
    }

    #[test]
    fn e2e_blast_radius_affected_files_includes_caller_file() {
        let dir = setup();
        let out = crate::engine::blast_radius(root(&dir), "multiply".into(), Some(1)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let files: Vec<&str> = v["affected_files"]
            .as_array().unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert!(
            files.iter().any(|f| f.contains("math.rs")),
            "expected math.rs in affected_files, got: {:?}",
            files
        );
    }

    #[test]
    fn e2e_blast_radius_import_graph_catches_file_dep() {
        let dir = setup();
        // utils.rs imports math.rs (`use crate::math::add`)
        // so blast_radius of `add` should include utils.rs via import graph
        let out = crate::engine::blast_radius(root(&dir), "add".into(), Some(2)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let files: Vec<&str> = v["affected_files"]
            .as_array().unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert!(
            files.iter().any(|f| f.contains("utils.rs")),
            "expected utils.rs in affected_files via import graph, got: {:?}",
            files
        );
    }

    // ── graph_rename ──────────────────────────────────────────────────────────

    #[test]
    fn e2e_graph_rename_unique_symbol_renames_definition_and_callsites() {
        let dir = setup();
        // subtract is unique — only defined and used in math.rs
        let out = crate::engine::graph_rename(root(&dir), "subtract".into(), "sub".into()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["old_name"], "subtract");
        assert_eq!(v["new_name"], "sub");
        assert!(v["occurrences_renamed"].as_u64().unwrap() >= 1);

        let content = fs::read_to_string(dir.path().join("src/math.rs")).unwrap();
        assert!(content.contains("fn sub("), "definition not renamed");
        assert!(!content.contains("subtract"), "old name still present");
    }

    #[test]
    fn e2e_graph_rename_updates_cross_file_callsites() {
        let dir = setup();
        // sum_three in utils.rs calls add() from math.rs
        // renaming add → add_ints should update both the definition in math.rs
        // and the callsite in utils.rs
        crate::engine::graph_rename(root(&dir), "add".into(), "add_ints".into()).unwrap();

        let math = fs::read_to_string(dir.path().join("src/math.rs")).unwrap();
        let utils = fs::read_to_string(dir.path().join("src/utils.rs")).unwrap();

        assert!(math.contains("fn add_ints("), "definition not renamed in math.rs");
        assert!(utils.contains("add_ints("), "call site not updated in utils.rs");
        assert!(!utils.contains("add(add("), "old call site still present in utils.rs");
    }

    // ── graph_move ────────────────────────────────────────────────────────────

    #[test]
    fn e2e_graph_move_injects_needed_imports_into_destination() {
        let dir = setup();
        // sum_three (utils.rs) calls add() which is imported from crate::math.
        // Moving sum_three to math.rs should NOT need to add any imports
        // (add is in the same file). But moving it to a new file would.
        // Instead: move `clamp` from utils.rs to a new file that has no imports.
        fs::write(dir.path().join("src/extra.rs"), "// extra module\n").unwrap();
        // Rebake to register extra.rs
        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();

        // clamp has no external imports needed — pure primitive ops.
        // But sum_three uses `add` from crate::math — moving it to extra.rs
        // should inject `use crate::math::add;`
        let out = crate::engine::graph_move(
            Some(dir.path().to_string_lossy().into_owned()),
            "sum_three".into(),
            "src/extra.rs".into(),
        )
        .unwrap();

        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "graph_move");

        let extra = fs::read_to_string(dir.path().join("src/extra.rs")).unwrap();
        // The function body should be present
        assert!(extra.contains("fn sum_three"), "sum_three not moved to extra.rs");
        // The import for add should have been injected
        assert!(
            extra.contains("use crate::math::add") || extra.contains("use crate::math"),
            "expected import for crate::math::add in extra.rs, got:\n{}",
            extra
        );
    }

    // ── flow ──────────────────────────────────────────────────────────────────

    fn setup_with_endpoint() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        // handler with a callout to a service function
        fs::write(
            dir.path().join("src/handlers.rs"),
            r#"
#[get("/users/{id}")]
pub async fn get_user(id: u32) -> String {
    fetch_user(id)
}

pub fn fetch_user(id: u32) -> String {
    format!("user:{}", id)
}
"#,
        ).unwrap();

        crate::engine::bake(Some(dir.path().to_string_lossy().into_owned())).unwrap();
        dir
    }

    #[test]
    fn e2e_flow_returns_endpoint_and_handler() {
        let dir = setup_with_endpoint();
        let out = crate::engine::flow(root(&dir), "/users".into(), None, None, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["tool"], "flow");
        assert!(v["endpoint"]["path"].as_str().unwrap().contains("/users"));
        assert_eq!(v["handler"]["name"], "get_user");
        assert!(v["handler"]["file"].as_str().unwrap().contains("handlers.rs"));
    }

    #[test]
    fn e2e_flow_includes_call_chain() {
        let dir = setup_with_endpoint();
        let out = crate::engine::flow(root(&dir), "/users".into(), None, Some(3), false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        let chain = v["call_chain"].as_array().unwrap();
        assert!(!chain.is_empty(), "expected non-empty call_chain");
        // depth 0 is the handler itself
        let handler_node = chain.iter().find(|n| n["name"] == "get_user");
        assert!(handler_node.is_some(), "expected get_user in call_chain");
    }

    #[test]
    fn e2e_flow_summary_contains_endpoint_and_handler() {
        let dir = setup_with_endpoint();
        let out = crate::engine::flow(root(&dir), "/users".into(), None, None, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        let summary = v["summary"].as_str().unwrap();
        assert!(summary.contains("get_user"), "summary missing handler: {}", summary);
        assert!(summary.contains("/users"), "summary missing endpoint: {}", summary);
    }

    #[test]
    fn e2e_flow_include_source_populates_handler_source() {
        let dir = setup_with_endpoint();
        let out = crate::engine::flow(root(&dir), "/users".into(), None, None, true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        let source = v["handler"]["source"].as_str();
        assert!(source.is_some(), "expected source to be present with include_source=true");
        assert!(source.unwrap().contains("fetch_user"), "source missing body content");
    }

    #[test]
    fn e2e_flow_errors_on_unknown_endpoint() {
        let dir = setup_with_endpoint();
        let err = crate::engine::flow(root(&dir), "/nonexistent".into(), None, None, false).unwrap_err();
        assert!(err.to_string().contains("No endpoint matching"), "unexpected error: {}", err);
    }

    // ── graph_delete ──────────────────────────────────────────────────────────

    #[test]
    fn e2e_graph_delete_removes_function_and_leaves_rest_intact() {
        let dir = setup();
        let out = crate::engine::graph_delete(root(&dir), "clamp".into(), None, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["bytes_removed"].as_u64().unwrap() > 0);

        let content = fs::read_to_string(dir.path().join("src/utils.rs")).unwrap();
        assert!(!content.contains("fn clamp"), "clamp still present after delete");
        assert!(content.contains("fn sum_three"), "sum_three was incorrectly removed");
        assert!(content.contains("fn format_result"), "format_result was incorrectly removed");
    }

    // ── script ────────────────────────────────────────────────────────────────

    #[test]
    fn e2e_script_symbol_call() {
        let dir = setup();
        let out = crate::engine::run_script(
            root(&dir),
            r#"symbol("add")"#.to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        let matches = v["result"]["matches"].as_array().unwrap();
        assert!(!matches.is_empty(), "symbol('add') should find a match");
        assert_eq!(matches[0]["name"], "add");
    }

    #[test]
    fn e2e_script_chain_symbol_then_blast_radius() {
        let dir = setup();
        // Rhai equivalent of the old two-step pipeline: symbol → blast_radius using result
        let out = crate::engine::run_script(
            root(&dir),
            r#"
                let s = symbol("add");
                let name = s["matches"][0]["name"];
                blast_radius(name)
            "#
            .to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        assert!(v["result"]["callers"].is_array());
    }

    #[test]
    fn e2e_script_conditional_on_caller_count() {
        let dir = setup();
        // clamp has no callers — condition should branch to symbol("add")
        let out = crate::engine::run_script(
            root(&dir),
            r#"
                let br = blast_radius("clamp");
                if br["callers"].len() == 0 {
                    symbol("add")
                } else {
                    br
                }
            "#
            .to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        // clamp has no callers → took the true branch → returned symbol("add") result
        assert!(v["result"]["matches"].is_array(), "expected symbol result for 'add'");
    }

    #[test]
    fn e2e_script_map_over_array() {
        let dir = setup();
        // collect all function names from file_functions, filter for those containing 'add'
        let out = crate::engine::run_script(
            root(&dir),
            r#"
                let ff = file_functions("src/math.rs");
                ff["functions"].filter(|f| f["name"].contains("add"))
            "#
            .to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        let filtered = v["result"].as_array().unwrap();
        assert!(!filtered.is_empty(), "filter for 'add' should return at least one result");
    }

    #[test]
    fn e2e_script_supersearch_result() {
        let dir = setup();
        let out = crate::engine::run_script(
            root(&dir),
            r#"supersearch("add")"#.to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        let matches = v["result"]["matches"].as_array().unwrap();
        assert!(!matches.is_empty(), "supersearch for 'add' should find results");
    }

    // ── patch pre-write AST validation ───────────────────────────────────────

    #[test]
    fn e2e_patch_rejects_invalid_rust_syntax() {
        let dir = setup();
        // Attempt to patch add() with syntactically broken Rust — missing closing brace
        let result = crate::engine::patch_string(
            root(&dir),
            "src/math.rs".into(),
            "a + b".into(),
            "{ { { not valid rust".into(),
        );
        assert!(result.is_err(), "patch_string should reject syntactically invalid Rust");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("syntax") || msg.contains("parse") || msg.contains("error"),
            "error message should mention syntax/parse: {}", msg
        );
    }

    #[test]
    fn e2e_patch_file_unchanged_on_invalid_syntax() {
        let dir = setup();
        let original = fs::read_to_string(dir.path().join("src/math.rs")).unwrap();

        let _ = crate::engine::patch_string(
            root(&dir),
            "src/math.rs".into(),
            "a + b".into(),
            "{ { { not valid rust".into(),
        );

        let after = fs::read_to_string(dir.path().join("src/math.rs")).unwrap();
        assert_eq!(original, after, "file must not be modified when patch is rejected");
    }

    #[test]
    fn e2e_patch_accepts_valid_syntax() {
        let dir = setup();
        let result = crate::engine::patch_string(
            root(&dir),
            "src/math.rs".into(),
            "a + b".into(),
            "a.saturating_add(b)".into(),
        );
        assert!(result.is_ok(), "patch_string should accept valid Rust: {:?}", result.err());
        let after = fs::read_to_string(dir.path().join("src/math.rs")).unwrap();
        assert!(after.contains("saturating_add"), "file should contain the new content");
    }

    // ── bake skips .git/ ──────────────────────────────────────────────────────

    #[test]
    fn e2e_bake_does_not_index_git_directory() {
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();

        fs::write(root_path.join("main.rs"), "fn hello() {}\n").unwrap();

        // Simulate a .git directory with object blobs (like a real repo has thousands of)
        let git_dir = root_path.join(".git");
        fs::create_dir_all(git_dir.join("objects/ab")).unwrap();
        fs::write(git_dir.join("objects/ab/cdef1234"), b"blob content").unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        let out = crate::engine::symbol(
            Some(root_path.to_string_lossy().into_owned()),
            "hello".to_string(), false, None, None, false,
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        // .git/ blobs must not appear in the file list
        let bake_json = std::fs::read_to_string(
            root_path.join("bakes/latest/bake.json")
        ).unwrap();
        assert!(
            !bake_json.contains(".git/objects"),
            ".git/objects must not be indexed"
        );
        // main.rs must still be found
        assert!(
            v["matches"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "main.rs should still be indexed"
        );
    }

    // ── bake .gitignore ───────────────────────────────────────────────────────

    #[test]
    fn e2e_bake_respects_gitignore() {
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();

        // Write a real source file that should be indexed.
        fs::write(root_path.join("main.rs"), "fn hello() {}\n").unwrap();

        // Write a file inside an ignored directory that must NOT be indexed.
        let dist = root_path.join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("bundle.rs"), "fn ignored_fn() {}\n").unwrap();

        // Write .gitignore that excludes dist/.
        fs::write(root_path.join(".gitignore"), "dist/\n").unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        // symbol should find hello (source file indexed).
        let out = crate::engine::symbol(
            Some(root_path.to_string_lossy().into_owned()),
            "hello".to_string(),
            false,
            None,
            Some(20),
            false,
        )
        .unwrap();
        assert!(out.contains("hello"), "main.rs should be indexed");

        // ignored_fn must not appear anywhere in the index.
        let out2 = crate::engine::symbol(
            Some(root_path.to_string_lossy().into_owned()),
            "ignored_fn".to_string(),
            false,
            None,
            Some(20),
            false,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert!(
            v["matches"].as_array().map(|a| a.is_empty()).unwrap_or(true),
            "dist/bundle.rs should be excluded via .gitignore"
        );
    }

    // ── compiler guard (write_with_compiler_guard) ────────────────────────────

    #[test]
    fn e2e_compiler_guard_rejects_zig_type_error() {
        // zig ast-check catches undefined identifiers without needing a full build.
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();

        fs::write(
            root_path.join("main.zig"),
            "const std = @import(\"std\");\npub fn main() void {\n    const x: u32 = 1;\n    _ = x;\n}\n",
        ).unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        // Inject a type error: reference an undefined variable.
        let result = crate::engine::patch_string(
            Some(root_path.to_string_lossy().into_owned()),
            "main.zig".into(),
            "const x: u32 = 1;".into(),
            "const x: u32 = undefined_symbol;".into(),
        );

        assert!(result.is_err(), "patch_string should reject Zig type errors");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("compiler") || msg.contains("zig") || msg.contains("error"),
            "error should mention compiler rejection: {}", msg
        );

        // File must be restored to original.
        let after = fs::read_to_string(root_path.join("main.zig")).unwrap();
        assert!(
            after.contains("const x: u32 = 1;"),
            "file must be restored after compiler rejection"
        );
    }

    #[test]
    fn e2e_compiler_guard_rejects_rust_type_error() {
        // A minimal Rust crate — cargo check can run against it.
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();

        fs::write(root_path.join("Cargo.toml"),
            "[package]\nname = \"guard_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ).unwrap();
        fs::create_dir_all(root_path.join("src")).unwrap();
        fs::write(
            root_path.join("src/main.rs"),
            "fn add(a: i32, b: i32) -> i32 { a + b }\nfn main() {}\n",
        ).unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        let original = fs::read_to_string(root_path.join("src/main.rs")).unwrap();

        // Inject a type error: call undefined function (syntactically valid, semantically wrong).
        let result = crate::engine::patch_string(
            Some(root_path.to_string_lossy().into_owned()),
            "src/main.rs".into(),
            "a + b".into(),
            "totally_undefined_fn_xyz(a, b)".into(),
        );

        assert!(result.is_err(), "patch_string should reject Rust type errors via cargo check");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("compiler") || msg.contains("cargo") || msg.contains("error"),
            "error should mention compiler rejection: {}", msg
        );

        // File must be restored.
        let after = fs::read_to_string(root_path.join("src/main.rs")).unwrap();
        assert_eq!(original, after, "file must be restored after cargo check rejection");
    }

    // ── script integration ────────────────────────────────────────────────────

    #[test]
    fn script_symbol_returns_function_matches() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();
        let result = crate::engine::run_script(
            Some(root),
            r#"let s = symbol("add"); s"#.to_string(),
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        let matches = v["result"]["matches"].as_array().expect("matches array");
        assert!(!matches.is_empty(), "symbol('add') should return at least one match");
        assert_eq!(matches[0]["name"], "add");
    }

    #[test]
    fn script_health_returns_all_smell_keys() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();
        let result = crate::engine::run_script(
            Some(root),
            r#"let h = health(); h.keys()"#.to_string(),
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        let keys: Vec<&str> = v["result"].as_array().unwrap()
            .iter().filter_map(|k| k.as_str()).collect();
        assert!(keys.contains(&"dead_code"), "health() must have dead_code key");
        assert!(keys.contains(&"large_functions"), "health() must have large_functions key");
    }

    #[test]
    fn script_dead_code_visibility_triage() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();
        // The triage script from the new workflow — must return a map with both keys.
        let result = crate::engine::run_script(
            Some(root),
            r#"
let h = health();
let pub_dead = [];
let priv_dead = [];
for d in h["dead_code"] {
    if d["visibility"] == "public" { pub_dead += [d["name"]]; }
    else { priv_dead += [d["name"]]; }
}
#{ public_dead: pub_dead, private_dead_count: priv_dead.len() }
"#.to_string(),
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        assert!(v["result"]["public_dead"].is_array());
        assert!(v["result"]["private_dead_count"].is_number());
    }

    #[test]
    fn script_file_functions_aggregation() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();
        // Aggregate fn count from two fixture files in one script call.
        let result = crate::engine::run_script(
            Some(root),
            r#"
let files = ["src/main.rs", "src/utils.rs"];
let report = [];
for f in files {
    let ff = file_functions(f);
    let fns = ff["functions"];
    report += [#{ file: f, fn_count: fns.len() }];
}
report
"#.to_string(),
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        let rows = v["result"].as_array().expect("array of rows");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r["fn_count"].as_i64().unwrap_or(-1) >= 0));
    }

    #[test]
    fn symbol_stdlib_flag_finds_rust_stdlib() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();

        // Gracefully skip if Rust stdlib sysroot is not installed.
        let stdlib_paths = crate::engine::util::detect_stdlib_paths();
        if !stdlib_paths.iter().any(|(lang, _)| lang == "rust") {
            eprintln!("skip: Rust stdlib not found via rustc --print sysroot");
            return;
        }

        // "HashMap" is defined in the Rust stdlib — should produce at least one stdlib match.
        let json = crate::engine::symbol(
            Some(root),
            "HashMap".to_string(),
            false,
            None,
            Some(10),
            true,
        ).unwrap();

        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let matches = v["matches"].as_array().expect("matches array");
        let has_stdlib = matches.iter().any(|m| m["is_stdlib"].as_bool().unwrap_or(false));
        assert!(has_stdlib, "expected at least one is_stdlib: true match for HashMap in Rust stdlib");
    }

    #[test]
    fn sig_hash_is_populated_for_rust_functions() {
        // Fixture has add(a: i64, b: i64) -> i64 — a Rust function. Should carry sig_hash.
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();

        let json = crate::engine::symbol(
            Some(root),
            "add".to_string(),
            false,
            None,
            Some(5),
            false,
        ).unwrap();

        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let matches = v["matches"].as_array().expect("matches array");
        let has_hash = matches.iter().any(|m| m["sig_hash"].is_string());
        assert!(has_hash, "expected Rust function 'add' to carry a sig_hash");
    }

    #[test]
    fn health_structural_duplicate_detected_via_sig_hash() {
        // Fixture has add, subtract, multiply — all (i64, i64) -> i64.
        // health should flag them as structural duplicates via sig_hash.
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();

        let json = crate::engine::health(Some(root), Some(50)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let dupes = v["duplicate_code"].as_array().expect("duplicate_code array");

        let has_structural = dupes.iter().any(|d| {
            d["stem"].as_str().map(|s| s.starts_with("sig:")).unwrap_or(false)
                && d["smell"].as_str() == Some("Structural Duplicate")
        });
        assert!(has_structural, "expected health to flag add/subtract/multiply as structural duplicates via sig_hash");
    }
}
