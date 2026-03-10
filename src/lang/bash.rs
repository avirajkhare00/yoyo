use std::fs;
use std::path::Path;

use anyhow::Result;
use ast_grep_language::{LanguageExt, SupportLang};
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative,
    IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType, LanguageAnalyzer,
    NodeKinds, Visibility,
};

pub struct BashAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["variable_name", "word"],
    strings: &["string", "raw_string", "ansi_c_string"],
    comments: &["comment"],
    calls: &["command"],
    assigns: &["variable_assignment"],
    returns: &["return"],
};

impl LanguageAnalyzer for BashAnalyzer {
    fn language(&self) -> &str { "bash" }

    fn extract_imports(&self, source: &str) -> Vec<String> {
        source.lines()
            .filter_map(|l| {
                let t = l.trim();
                let s = t.strip_prefix("source ")
                    .or_else(|| t.strip_prefix(". "))?;
                let s = s.trim().trim_matches(|c| c == '\'' || c == '"');
                if s.is_empty() { None } else { Some(s.to_string()) }
            })
            .collect()
    }

    fn analyze_file(&self, root: &Path, file: &Path) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>, Vec<IndexedImpl>)> {
        let source = fs::read_to_string(file)?;
        let mut parser = Parser::new();
        parser.set_language(&SupportLang::Bash.get_ts_language()).expect("Bash grammar");
        let tree = parser.parse(&source, None).ok_or_else(|| anyhow::anyhow!("parse failed"))?;
        let mod_path = module_path_from_file(&relative(root, file), "bash");
        let mut functions = Vec::new();
        walk_bash(&source, root, file, tree.root_node(), &mod_path, &mut functions);
        Ok((functions, vec![], vec![], vec![]))
    }

    fn ts_language(&self) -> Option<tree_sitter::Language> {
        Some(SupportLang::Bash.get_ts_language())
    }
    fn node_kinds(&self) -> Option<&'static crate::lang::NodeKinds> {
        Some(&KINDS)
    }
}

fn walk_bash(source: &str, root: &Path, file: &Path, node: Node, mod_path: &str, functions: &mut Vec<IndexedFunction>) {
    if node.kind() == "function_definition" {
        if let Some(n) = node.child_by_field_name("name") {
            let name = n.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            if !name.is_empty() {
                let (start_line, end_line) = line_range(&node);
                let (byte_start, byte_end) = byte_range(&node);
                functions.push(IndexedFunction {
                    name: name.clone(),
                    file: relative(root, file),
                    language: "bash".to_string(),
                    start_line, end_line,
                    complexity: estimate_complexity(node, source),
                    calls: collect_calls(node, source),
                    byte_start, byte_end,
                    module_path: mod_path.to_string(),
                    qualified_name: qualified_name(mod_path, &name, "bash"),
                    visibility: Visibility::Public,
                    parent_type: None,
                    is_stdlib: false,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_bash(source, root, file, child, mod_path, functions);
    }
}

fn estimate_complexity(node: Node, _source: &str) -> u32 {
    super::estimate_complexity_for(node, &[
        "if_statement", "for_statement", "while_statement",
        "until_statement", "case_statement", "case_item",
    ])
}

fn collect_calls(node: Node, source: &str) -> Vec<super::CallSite> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort_by(|a, b| a.callee.cmp(&b.callee).then(a.line.cmp(&b.line)));
    calls.dedup_by(|a, b| a.callee == b.callee && a.qualifier == b.qualifier);
    calls
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<super::CallSite>) {
    if node.kind() == "command" {
        let line = node.start_position().row as u32 + 1;
        if let Some(name_node) = node.child_by_field_name("name") {
            let callee = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            if !callee.is_empty() && !callee.starts_with('-') {
                calls.push(super::CallSite { callee, qualifier: None, line });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_inner(child, source, calls);
    }
}
