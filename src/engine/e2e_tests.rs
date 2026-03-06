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
        let out = crate::engine::symbol(root(&dir), "add".into(), false, None, None).unwrap();
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
            let out = crate::engine::symbol(root(&dir), name.to_string(), false, None, None).unwrap();
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
}
