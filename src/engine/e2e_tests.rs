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

    fn subdir_root(dir: &TempDir) -> Option<String> {
        Some(dir.path().join("src").to_string_lossy().into_owned())
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

    #[test]
    fn e2e_judge_change_returns_grounded_payload() {
        let dir = setup();
        let out = crate::engine::judge_change(
            root(&dir),
            "add numbers and format a result".to_string(),
            None,
            None,
            Some(3),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["tool"], "judge_change");
        assert!(v["ownership_layer"]["name"].as_str().unwrap().contains("src"));
        assert!(!v["candidate_symbols"].as_array().unwrap().is_empty());
        assert!(!v["invariants"].as_array().unwrap().is_empty());
        assert!(!v["verification_commands"].as_array().unwrap().is_empty());
    }

    #[test]
    fn e2e_judge_change_prioritises_symbol_hint() {
        let dir = setup();
        let out = crate::engine::judge_change(
            root(&dir),
            "format some numbers".to_string(),
            Some("sum_three".to_string()),
            None,
            Some(3),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let top = &v["candidate_symbols"].as_array().unwrap()[0];
        assert_eq!(top["name"], "sum_three");
    }

    #[test]
    fn e2e_symbol_accepts_subdirectory_path_when_bake_exists_at_project_root() {
        let dir = setup();
        let out = crate::engine::symbol(subdir_root(&dir), "add".into(), false, None, None, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(!matches.is_empty(), "expected at least one match for 'add'");
        assert_eq!(v["project_root"].as_str().unwrap(), dir.path().to_string_lossy());
    }

    #[test]
    fn e2e_symbol_suggests_project_root_when_subdirectory_has_no_bake() {
        let dir = TempDir::new().unwrap();
        copy_dir_recursive(&fixture_src(), dir.path());
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"sample-project\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let subdir = dir.path().join("src");

        let err = crate::engine::symbol(
            Some(subdir.to_string_lossy().into_owned()),
            "add".into(),
            false,
            None,
            None,
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains(&format!("No bake index found under {}", subdir.display())));
        assert!(err.contains(&format!("Did you mean to pass the project root {}", dir.path().display())));
    }

    #[test]
    fn e2e_supersearch_extracts_identifier_from_natural_language_query() {
        let dir = setup();
        let out = crate::engine::supersearch(
            root(&dir),
            "call sites of add".into(),
            "identifiers".into(),
            "all".into(),
            Some(true),
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(!matches.is_empty(), "expected natural-language supersearch query to find 'add' usages");
        assert!(
            matches.iter().any(|m| m["snippet"].as_str().unwrap().contains("add")),
            "expected at least one match snippet to contain 'add': {:?}",
            matches
        );
    }

    #[test]
    fn e2e_supersearch_keeps_empty_result_for_missing_natural_language_query() {
        let dir = setup();
        let out = crate::engine::supersearch(
            root(&dir),
            "call sites of definitely_missing_symbol".into(),
            "identifiers".into(),
            "all".into(),
            Some(true),
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(
            matches.is_empty(),
            "expected no matches for a missing symbol even after natural-language fallback: {:?}",
            matches
        );
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
        assert_eq!(
            v["next_hint"],
            "Use change(action=rename|move|delete) once the caller set is acceptable, or inspect(name=...) to review one caller."
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
        assert_eq!(
            v["next_hint"],
            "Use inspect(name=...) on the handler or a downstream callee to read the code behind this route."
        );
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

    #[test]
    fn e2e_impact_symbol_mode_wraps_blast_radius() {
        let dir = setup();
        let out = crate::engine::impact(root(&dir), Some("multiply".into()), None, None, Some(1), None)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["tool"], "impact");
        assert_eq!(v["mode"], "symbol");
        assert_eq!(v["target"]["symbol"], "multiply");
        assert!(v["callers"].as_array().unwrap().iter().any(|c| c["caller"] == "square"));
        assert_eq!(
            v["next_hint"],
            "Use inspect(name=...) to read one affected caller, or change(action=rename|move|delete) when the impact is acceptable."
        );
    }

    #[test]
    fn e2e_impact_endpoint_mode_wraps_flow() {
        let dir = setup_with_endpoint();
        let out = crate::engine::impact(
            root(&dir),
            None,
            Some("/users".into()),
            None,
            Some(3),
            Some(true),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["tool"], "impact");
        assert_eq!(v["mode"], "endpoint");
        assert_eq!(v["target"]["endpoint"], "/users");
        assert_eq!(v["handler"]["name"], "get_user");
        assert!(v["call_chain"].as_array().unwrap().iter().any(|n| n["name"] == "get_user"));
        assert_eq!(
            v["next_hint"],
            "Use inspect(name=...) on the handler or downstream callee, or change(action=edit|rename|move) once you know where the request path lands."
        );
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
    fn e2e_script_inspect_call() {
        let dir = setup();
        let out = crate::engine::run_script(
            root(&dir),
            r#"inspect(#{name: "add", include_source: true})"#.to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        assert_eq!(v["result"]["tool"], "inspect");
        assert_eq!(v["result"]["mode"], "symbol");
        assert_eq!(v["result"]["matches"][0]["name"], "add");
    }

    #[test]
    fn e2e_script_chain_inspect_then_impact() {
        let dir = setup();
        // Rhai equivalent of chaining task-shaped tools: inspect → impact using the resolved symbol
        let out = crate::engine::run_script(
            root(&dir),
            r#"
                let s = inspect(#{name: "add"});
                let name = s["matches"][0]["name"];
                impact(#{symbol: name})
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
        // clamp has no callers — condition should branch to inspect(add)
        let out = crate::engine::run_script(
            root(&dir),
            r#"
                let br = impact(#{symbol: "clamp"});
                if br["callers"].len() == 0 {
                    inspect(#{name: "add"})
                } else {
                    br
                }
            "#
            .to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tool"], "script");
        // clamp has no callers → took the true branch → returned inspect(add) result
        assert_eq!(v["result"]["tool"], "inspect");
        assert_eq!(v["result"]["matches"][0]["name"], "add");
    }

    #[test]
    fn e2e_script_map_over_array() {
        let dir = setup();
        // collect all outlined functions from inspect(file=...), filter for those containing 'add'
        let out = crate::engine::run_script(
            root(&dir),
            r#"
                let outline = inspect(#{file: "src/math.rs"});
                outline["functions"].filter(|f| f["name"].contains("add"))
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
            r#"search("add")"#.to_string(),
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
        let bake = crate::engine::db::read_bake_from_db(
            &root_path.join("bakes/latest/bake.db")
        ).unwrap();
        assert!(
            bake.files.iter().all(|f| !f.path.to_string_lossy().contains(".git/objects")),
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
    fn script_inspect_returns_function_metadata() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();
        let result = crate::engine::run_script(
            Some(root),
            r#"let s = inspect(#{name: "add"}); s"#.to_string(),
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["tool"], "script");
        assert_eq!(v["result"]["tool"], "inspect");
        assert_eq!(v["result"]["matches"][0]["name"], "add");
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
        assert!(keys.contains(&"next_hint"), "health() must expose next_hint");
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
    fn script_inspect_file_aggregation() {
        let dir = setup();
        let root = dir.path().to_string_lossy().into_owned();
        // Aggregate outlined function counts from two fixture files in one script call.
        let result = crate::engine::run_script(
            Some(root),
            r#"
let files = ["src/main.rs", "src/utils.rs"];
let report = [];
for f in files {
    let outline = inspect(#{file: f});
    let fns = outline["functions"];
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

        let json = crate::engine::health(Some(root), Some(50), None, None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let dupes = v["duplicate_code"].as_array().expect("duplicate_code array");

        let has_structural = dupes.iter().any(|d| {
            d["stem"].as_str().map(|s| s.starts_with("sig:")).unwrap_or(false)
                && d["smell"].as_str() == Some("Structural Duplicate")
        });
        assert!(has_structural, "expected health to flag add/subtract/multiply as structural duplicates via sig_hash");
    }

    #[test]
    fn health_compact_view_paginates_sections() {
        let dir = setup();
        let file = dir.path().join("src/paging.rs");
        fs::write(
            &file,
            r#"fn unused_one() {}
fn unused_two() {}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let json = crate::engine::health(
            root(&dir),
            Some(50),
            Some("compact".to_string()),
            Some(1),
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["view"], "compact");
        let sections = v["sections"].as_array().expect("sections array");
        let dead_code = sections
            .iter()
            .find(|section| section["section"] == "dead_code")
            .expect("dead_code section");
        assert_eq!(dead_code["limit"], 1);
        assert_eq!(dead_code["offset"], 0);
        assert_eq!(dead_code["items"].as_array().unwrap().len(), 1);
        assert_eq!(dead_code["next_cursor"], "dead_code:1");

        let page_2 = crate::engine::health(
            root(&dir),
            Some(50),
            Some("compact".to_string()),
            Some(1),
            Some("dead_code:1".to_string()),
        )
        .unwrap();
        let page_2_json: serde_json::Value = serde_json::from_str(&page_2).unwrap();
        let page_2_sections = page_2_json["sections"].as_array().expect("sections array");
        assert_eq!(page_2_sections.len(), 1, "cursor should return one section page");
        assert_eq!(page_2_sections[0]["section"], "dead_code");
        assert_eq!(page_2_sections[0]["offset"], 1);
    }

    #[test]
    fn llm_workflows_compact_view_sections_reference_catalog() {
        let json = crate::engine::llm_workflows(
            None,
            Some("compact".to_string()),
            Some(2),
            None,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["view"], "compact");
        let sections = v["sections"].as_array().expect("sections array");
        assert!(
            sections.iter().any(|section| section["section"] == "workflows"),
            "compact workflows view must include the workflows section"
        );
        let workflows = sections
            .iter()
            .find(|section| section["section"] == "workflows")
            .expect("workflows section");
        assert_eq!(workflows["limit"], 2);
        assert!(workflows["next_cursor"].is_string());
    }

    #[test]
    fn llm_workflows_query_returns_ranked_matches() {
        let json = crate::engine::llm_workflows(
            None,
            None,
            None,
            None,
            Some("rename symbol".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["query"], "rename symbol");
        let matches = v["matches"].as_array().expect("matches array");
        assert!(!matches.is_empty(), "query 'rename symbol' must return at least one match");
        // graph_rename workflow or decision should rank highest
        assert!(
            matches.iter().any(|m| {
                m["item"]["name"] == "Graph rename (one-shot)"
                    || m["item"]["right_tool"] == "graph_rename"
                    || m["item"]["name"] == "Rename with safety check"
            }),
            "query 'rename symbol' must surface graph_rename workflow or decision"
        );
        // results are sorted descending by score
        let scores: Vec<u64> = matches
            .iter()
            .map(|m| m["score"].as_u64().unwrap_or(0))
            .collect();
        assert!(
            scores.windows(2).all(|w| w[0] >= w[1]),
            "matches must be sorted by descending score"
        );
    }

    #[test]
    fn llm_workflows_query_delete_dead_code() {
        let json = crate::engine::llm_workflows(
            None,
            None,
            None,
            None,
            Some("delete dead code".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let matches = v["matches"].as_array().expect("matches array");
        assert!(!matches.is_empty(), "query 'delete dead code' must return matches");
        assert!(
            matches.iter().any(|m| {
                m["item"]["name"] == "Safely delete dead code"
                    || m["item"]["right_tool"] == "graph_delete"
                    || (m["kind"] == "antipattern"
                        && m["item"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("graph_delete"))
            }),
            "query 'delete dead code' must surface the safe-delete workflow or graph_delete decision"
        );
    }

    #[test]
    fn llm_workflows_query_stop_words_only_returns_empty() {
        // A query made entirely of stop words should match nothing.
        let json = crate::engine::llm_workflows(
            None, None, None, None,
            Some("how do I use the a an to".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["matches"].as_array().unwrap().len(),
            0,
            "stop-word-only query must return no matches"
        );
    }

    #[test]
    fn llm_workflows_query_no_match_returns_empty() {
        let json = crate::engine::llm_workflows(
            None, None, None, None,
            Some("xyzzy frobnicator quux".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["matches"].as_array().unwrap().len(),
            0,
            "nonsense query must return no matches"
        );
    }

    #[test]
    fn llm_workflows_query_case_insensitive() {
        let lower = crate::engine::llm_workflows(
            None, None, None, None,
            Some("rename".to_string()),
        )
        .unwrap();
        let upper = crate::engine::llm_workflows(
            None, None, None, None,
            Some("RENAME".to_string()),
        )
        .unwrap();
        let lv: serde_json::Value = serde_json::from_str(&lower).unwrap();
        let uv: serde_json::Value = serde_json::from_str(&upper).unwrap();
        assert_eq!(
            lv["matches"].as_array().unwrap().len(),
            uv["matches"].as_array().unwrap().len(),
            "query matching must be case-insensitive"
        );
    }

    #[test]
    fn llm_workflows_query_hits_decision_map() {
        let json = crate::engine::llm_workflows(
            None, None, None, None,
            Some("struct fields types".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(
            matches.iter().any(|m| m["kind"] == "decision"),
            "query 'struct fields types' must return at least one decision entry"
        );
    }

    #[test]
    fn llm_workflows_query_hits_metapattern() {
        let json = crate::engine::llm_workflows(
            None, None, None, None,
            Some("orient unfamiliar codebase".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(
            matches.iter().any(|m| m["kind"] == "metapattern"),
            "query 'orient unfamiliar codebase' must surface a metapattern"
        );
    }

    #[test]
    fn llm_workflows_query_capped_at_ten() {
        // "function" appears in almost everything — result set must be capped at 10.
        let json = crate::engine::llm_workflows(
            None, None, None, None,
            Some("function".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            v["matches"].as_array().unwrap().len() <= 10,
            "query results must be capped at 10"
        );
    }

    #[test]
    fn e2e_function_item_argument_counts_as_usage() {
        let dir = setup();
        let cache_file = dir.path().join("src/cache.rs");
        fs::write(
            &cache_file,
            r#"use std::sync::OnceLock;

static REGISTRY: OnceLock<Vec<&'static str>> = OnceLock::new();

fn build_registry() -> Vec<&'static str> {
    vec!["ok"]
}

fn get_registry() -> &'static Vec<&'static str> {
    REGISTRY.get_or_init(build_registry)
}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let blast = crate::engine::blast_radius(root(&dir), "build_registry".into(), Some(2)).unwrap();
        let blast_json: serde_json::Value = serde_json::from_str(&blast).unwrap();
        let callers = blast_json["callers"].as_array().expect("callers array");
        assert!(
            callers.iter().any(|caller| caller["caller"] == "get_registry"),
            "expected get_registry to be reported as a caller when build_registry is passed as a function item"
        );

        let health = crate::engine::health(root(&dir), Some(100), None, None, None).unwrap();
        let health_json: serde_json::Value = serde_json::from_str(&health).unwrap();
        let dead_code = health_json["dead_code"].as_array().expect("dead_code array");
        assert!(
            !dead_code.iter().any(|f| f["name"] == "build_registry" && f["file"] == "src/cache.rs"),
            "build_registry should not be flagged as dead when passed to get_or_init"
        );
    }

    #[test]
    fn e2e_health_excludes_trait_impl_methods_from_dead_code() {
        let dir = setup();
        let file = dir.path().join("src/trait_impl.rs");
        fs::write(
            &file,
            r#"trait Greeter {
    fn greet(&self) -> &'static str;
}

struct Person;

impl Person {
    fn unused_helper(&self) -> &'static str {
        "helper"
    }
}

impl Greeter for Person {
    fn greet(&self) -> &'static str {
        "hello"
    }
}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let health = crate::engine::health(root(&dir), Some(100), None, None, None).unwrap();
        let health_json: serde_json::Value = serde_json::from_str(&health).unwrap();
        let dead_code = health_json["dead_code"].as_array().expect("dead_code array");

        assert!(
            !dead_code.iter().any(|f| f["name"] == "greet" && f["file"] == "src/trait_impl.rs"),
            "trait impl methods should not be classified as dead code"
        );
        assert!(
            dead_code.iter().any(|f| f["name"] == "unused_helper" && f["file"] == "src/trait_impl.rs"),
            "unused inherent methods should still be classified as dead code"
        );
    }

    #[test]
    fn e2e_macro_body_nested_calls_are_indexed_for_rust() {
        let dir = setup();
        let file = dir.path().join("src/macro_calls.rs");
        fs::write(
            &file,
            r#"fn work() {}

fn runner() {
    vec![tokio::spawn!(work())];
}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let symbol = crate::engine::symbol(root(&dir), "runner".into(), false, None, Some(5), false).unwrap();
        let symbol_json: serde_json::Value = serde_json::from_str(&symbol).unwrap();
        let calls = symbol_json["matches"][0]["calls"].as_array().expect("calls array");

        assert!(
            calls.iter().any(|c| c["callee"] == "work"),
            "macro bodies should contribute nested function calls when the token tree contains call-like syntax: {}",
            serde_json::to_string(calls).unwrap()
        );
    }

    #[test]
    fn e2e_compiler_expansion_recovers_macro_hidden_call_edges() {
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();
        fs::write(
            root_path.join("Cargo.toml"),
            "[package]\nname = \"macro_expand_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(root_path.join("src")).unwrap();
        fs::write(
            root_path.join("src/main.rs"),
            r#"macro_rules! invoke {
    ($f:ident) => {
        $f()
    };
}

fn work() {}

fn runner() {
    invoke!(work);
}

fn main() {
    runner();
}
"#,
        )
        .unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        let symbol = crate::engine::symbol(
            Some(root_path.to_string_lossy().into_owned()),
            "runner".into(),
            false,
            None,
            Some(5),
            false,
        )
        .unwrap();
        let symbol_json: serde_json::Value = serde_json::from_str(&symbol).unwrap();
        let calls = symbol_json["matches"][0]["calls"].as_array().expect("calls array");

        assert!(
            calls.iter().any(|c| c["callee"] == "work"),
            "compiler expansion should recover call edges hidden behind macro_rules indirection: {}",
            serde_json::to_string(calls).unwrap()
        );
    }

    #[test]
    fn e2e_health_excludes_serde_default_function_refs_from_dead_code() {
        let dir = setup();
        let file = dir.path().join("src/serde_defaults.rs");
        fs::write(
            &file,
            r#"fn default_origin() -> String {
    "user".to_string()
}

#[derive(Default)]
struct Event {
    #[serde(default = "default_origin")]
    origin: String,
}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let health = crate::engine::health(root(&dir), Some(100), None, None, None).unwrap();
        let health_json: serde_json::Value = serde_json::from_str(&health).unwrap();
        let dead_code = health_json["dead_code"].as_array().expect("dead_code array");

        assert!(
            !dead_code.iter().any(|f| f["name"] == "default_origin" && f["file"] == "src/serde_defaults.rs"),
            "functions referenced by serde default attributes should not be classified as dead code"
        );
    }

    #[test]
    fn e2e_health_excludes_macro_closure_helper_calls_from_dead_code() {
        let dir = setup();
        let file = dir.path().join("src/mcp_like.rs");
        fs::write(
            &file,
            r#"use std::sync::OnceLock;

struct Args;

impl Args {
    fn str_req(&self) -> String {
        "ok".to_string()
    }

    fn bool_opt(&self) -> Option<bool> {
        Some(true)
    }
}

struct ToolEntry {
    handler: Box<dyn Fn(&Args) -> String>,
}

static REGISTRY: OnceLock<Vec<ToolEntry>> = OnceLock::new();

fn build_registry() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            handler: Box::new(|a| a.str_req()),
        },
        ToolEntry {
            handler: Box::new(|a| {
                if a.bool_opt().unwrap_or(false) {
                    a.str_req()
                } else {
                    String::new()
                }
            }),
        },
    ]
}

fn get_registry() -> &'static Vec<ToolEntry> {
    REGISTRY.get_or_init(build_registry)
}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let health = crate::engine::health(root(&dir), Some(100), None, None, None).unwrap();
        let health_json: serde_json::Value = serde_json::from_str(&health).unwrap();
        let dead_code = health_json["dead_code"].as_array().expect("dead_code array");

        for name in ["build_registry", "str_req", "bool_opt"] {
            assert!(
                !dead_code.iter().any(|f| f["name"] == name && f["file"] == "src/mcp_like.rs"),
                "{name} should not be classified as dead code when referenced from closure-heavy macro bodies"
            );
        }
    }

    #[test]
    fn e2e_health_excludes_go_init_from_dead_code() {
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();
        fs::write(
            root_path.join("main.go"),
            r#"package main

func init() {
    setup()
}

func setup() {}

func unusedHelper() {}

func main() {}
"#,
        )
        .unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        let health = crate::engine::health(Some(root_path.to_string_lossy().into_owned()), Some(100), None, None, None).unwrap();
        let health_json: serde_json::Value = serde_json::from_str(&health).unwrap();
        let dead_code = health_json["dead_code"].as_array().expect("dead_code array");

        assert!(
            !dead_code.iter().any(|f| f["name"] == "init" && f["file"] == "main.go"),
            "Go init functions are runtime entrypoints and should not be classified as dead code"
        );
        assert!(
            dead_code.iter().any(|f| f["name"] == "unusedHelper" && f["file"] == "main.go"),
            "ordinary unused Go helpers should still be classified as dead code"
        );
    }

    #[test]
    fn e2e_symbol_prefers_exact_case_sensitive_exported_match() {
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();
        fs::write(
            root_path.join("client.go"),
            r#"package client

func newClient() {}

func NewClient() {}

func NewClientWithEnvProxy() {
    NewClient()
}
"#,
        )
        .unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        let symbol = crate::engine::symbol(
            Some(root_path.to_string_lossy().into_owned()),
            "NewClient".into(),
            false,
            None,
            Some(10),
            false,
        )
        .unwrap();
        let symbol_json: serde_json::Value = serde_json::from_str(&symbol).unwrap();
        let matches = symbol_json["matches"].as_array().expect("matches array");

        assert_eq!(matches[0]["name"], "NewClient");
        assert_eq!(matches[0]["file"], "client.go");
        assert_eq!(matches[0]["primary"], true);
    }

    #[test]
    fn e2e_symbol_prefers_exact_name_over_suffix_variants() {
        let dir = TempDir::new().unwrap();
        let root_path = dir.path();
        fs::write(
            root_path.join("index.ts"),
            r#"export function createInstanceWithDefaults() {}

export function createInstance() {}
"#,
        )
        .unwrap();

        crate::engine::bake(Some(root_path.to_string_lossy().into_owned())).unwrap();

        let symbol = crate::engine::symbol(
            Some(root_path.to_string_lossy().into_owned()),
            "createInstance".into(),
            false,
            None,
            Some(10),
            false,
        )
        .unwrap();
        let symbol_json: serde_json::Value = serde_json::from_str(&symbol).unwrap();
        let matches = symbol_json["matches"].as_array().expect("matches array");

        assert_eq!(matches[0]["name"], "createInstance");
        assert_eq!(matches[0]["primary"], true);
    }

    #[test]
    fn e2e_rust_symbol_dedupes_recovered_helper_calls_per_line() {
        let dir = setup();
        let file = dir.path().join("src/mcp_like.rs");
        fs::write(
            &file,
            r#"use std::sync::OnceLock;

struct Args;

impl Args {
    fn str_req(&self) -> String {
        "ok".to_string()
    }

    fn bool_opt(&self) -> Option<bool> {
        Some(true)
    }
}

struct ToolEntry {
    handler: Box<dyn Fn(&Args) -> String>,
}

static REGISTRY: OnceLock<Vec<ToolEntry>> = OnceLock::new();

fn build_registry() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            handler: Box::new(|a| a.str_req()),
        },
        ToolEntry {
            handler: Box::new(|a| {
                if a.bool_opt().unwrap_or(false) {
                    a.str_req()
                } else {
                    String::new()
                }
            }),
        },
    ]
}

fn get_registry() -> &'static Vec<ToolEntry> {
    REGISTRY.get_or_init(build_registry)
}
"#,
        )
        .unwrap();
        crate::engine::bake(root(&dir)).unwrap();

        let symbol = crate::engine::symbol(root(&dir), "build_registry".into(), false, None, Some(5), false).unwrap();
        let symbol_json: serde_json::Value = serde_json::from_str(&symbol).unwrap();
        let calls = symbol_json["matches"][0]["calls"].as_array().expect("calls array");

        let qualified_str_req = calls.iter().filter(|c| {
            c["callee"] == "str_req" && c["qualifier"].as_str() == Some("a")
        }).count();
        let unqualified_str_req = calls.iter().filter(|c| {
            c["callee"] == "str_req" && c.get("qualifier").is_none()
        }).count();
        let qualified_bool_opt = calls.iter().filter(|c| {
            c["callee"] == "bool_opt" && c["qualifier"].as_str() == Some("a")
        }).count();
        let unqualified_bool_opt = calls.iter().filter(|c| {
            c["callee"] == "bool_opt" && c.get("qualifier").is_none()
        }).count();

        assert_eq!(
            qualified_str_req, 2,
            "expected one qualified str_req edge per source line after merge dedupe: {}",
            serde_json::to_string(calls).unwrap()
        );
        assert_eq!(
            unqualified_str_req, 0,
            "unqualified duplicate str_req edges should be removed when qualified edges exist: {}",
            serde_json::to_string(calls).unwrap()
        );
        assert_eq!(
            qualified_bool_opt, 1,
            "expected exactly one qualified bool_opt edge after merge dedupe: {}",
            serde_json::to_string(calls).unwrap()
        );
        assert_eq!(
            unqualified_bool_opt, 0,
            "unqualified duplicate bool_opt edges should be removed when qualified edges exist: {}",
            serde_json::to_string(calls).unwrap()
        );
    }

    // ── semantic_search ───────────────────────────────────────────────────────

    #[test]
    fn e2e_semantic_search_note_absent_when_embeddings_ready() {
        // Disable background embed so it can't race with our manually-seeded embeddings.db.
        std::env::set_var("YOYO_SKIP_EMBED", "1");
        let dir = setup();
        std::env::remove_var("YOYO_SKIP_EMBED");
        // Write a fake embeddings.db so vector_search path is attempted.
        // It will return empty results (no rows), fall through to TF-IDF,
        // but what matters is the note is NOT set when the file exists.
        let embed_path = dir.path().join("bakes/latest/embeddings.db");
        // Create an empty-but-valid SQLite DB with the embeddings table.
        {
            let conn = rusqlite::Connection::open(&embed_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS embeddings \
                 (name TEXT, file TEXT, start_line INTEGER, parent_type TEXT, embedding BLOB);",
            ).unwrap();
        }
        let out = crate::engine::semantic_search(
            root(&dir),
            "add numbers".into(),
            None,
            None,
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        // note should be absent — embeddings.db exists, no need to warn
        assert!(v["note"].is_null(), "note should be absent when embeddings.db exists: {:?}", v["note"]);
    }

    #[test]
    fn e2e_semantic_search_note_present_when_embeddings_absent() {
        let dir = setup();
        // Ensure embeddings.db does not exist (bake spawns rebuild in background, may or may not exist)
        let _ = std::fs::remove_file(dir.path().join("bakes/latest/embeddings.db"));

        let out = crate::engine::semantic_search(
            root(&dir),
            "compute sum".into(),
            None,
            None,
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let note = v["note"].as_str().expect("note must be present when embeddings.db is absent");
        assert!(note.contains("building"), "note should mention building: {note}");
        assert!(note.contains("TF-IDF"), "note should mention TF-IDF: {note}");
        // Results should still be returned (TF-IDF fallback works)
        assert!(v["results"].is_array());
    }

    #[test]
    fn e2e_semantic_search_tfidf_returns_ranked_results() {
        let dir = setup();
        let _ = std::fs::remove_file(dir.path().join("bakes/latest/embeddings.db"));

        let out = crate::engine::semantic_search(
            root(&dir),
            "add".into(),
            Some(5),
            None,
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let results = v["results"].as_array().unwrap();
        // Fixture has an `add` function — it should rank near the top
        assert!(!results.is_empty(), "TF-IDF should return results for 'add'");
        assert!(results.len() <= 5, "limit=5 respected");
        let names: Vec<&str> = results.iter().map(|r| r["name"].as_str().unwrap_or("")).collect();
        assert!(names.contains(&"add"), "add function should rank in results: {:?}", names);
    }
}
