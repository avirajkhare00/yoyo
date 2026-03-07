use std::fs;
use std::path::Path;

use anyhow::Result;
use ast_grep_language::{LanguageExt, SupportLang};
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative, walk_supersearch,
    AstMatch, IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType, LanguageAnalyzer,
    NodeKinds, Visibility,
};

pub struct RubyAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["identifier", "constant"],
    strings: &["string_content", "heredoc_body"],
    comments: &["comment"],
    calls: &["call", "method_call"],
    assigns: &["assignment"],
    returns: &["return"],
};

impl LanguageAnalyzer for RubyAnalyzer {
    fn language(&self) -> &str { "ruby" }
    fn extensions(&self) -> &[&str] { &["rb"] }

    fn extract_imports(&self, source: &str) -> Vec<String> {
        source.lines()
            .filter_map(|l| {
                let t = l.trim();
                let s = t.strip_prefix("require ")
                    .or_else(|| t.strip_prefix("require_relative "))?
                    .trim().trim_matches(|c| c == '\'' || c == '"');
                if s.is_empty() { None } else { Some(s.to_string()) }
            })
            .collect()
    }

    fn analyze_file(&self, root: &Path, file: &Path) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>, Vec<IndexedImpl>)> {
        let source = fs::read_to_string(file)?;
        let mut parser = Parser::new();
        parser.set_language(&SupportLang::Ruby.get_ts_language()).expect("Ruby grammar");
        let tree = parser.parse(&source, None).ok_or_else(|| anyhow::anyhow!("parse failed"))?;
        let mod_path = module_path_from_file(&relative(root, file), "ruby");
        let mut functions = Vec::new();
        let mut types = Vec::new();
        walk_ruby(&source, root, file, tree.root_node(), &mod_path, &mut functions, &mut types);
        Ok((functions, vec![], types, vec![]))
    }

    fn supports_ast_search(&self) -> bool { true }
    fn ast_search(&self, source: &str, query_lc: &str, context: &str, pattern: &str) -> Vec<AstMatch> {
        let mut parser = Parser::new();
        if parser.set_language(&SupportLang::Ruby.get_ts_language()).is_err() { return vec![]; }
        let tree = match parser.parse(source, None) { Some(t) => t, None => return vec![] };
        let lines: Vec<&str> = source.lines().collect();
        let mut matches = Vec::new();
        walk_supersearch(tree.root_node(), source, &lines, query_lc, context, pattern, false, false, false, &KINDS, &mut matches);
        matches
    }
}

fn walk_ruby(source: &str, root: &Path, file: &Path, node: Node, mod_path: &str, functions: &mut Vec<IndexedFunction>, types: &mut Vec<IndexedType>) {
    match node.kind() {
        "method" | "singleton_method" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = n.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    let (byte_start, byte_end) = byte_range(&node);
                    functions.push(IndexedFunction {
                        name: name.clone(),
                        file: relative(root, file),
                        language: "ruby".to_string(),
                        start_line, end_line,
                        complexity: estimate_complexity(node, source),
                        calls: collect_calls(node, source),
                        byte_start, byte_end,
                        module_path: mod_path.to_string(),
                        qualified_name: qualified_name(mod_path, &name, "ruby"),
                        visibility: Visibility::Public,
                        parent_type: None,
                    });
                }
            }
        }
        "class" | "module" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = n.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let kind = if node.kind() == "module" { "module" } else { "class" };
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name, file: relative(root, file), language: "ruby".to_string(),
                        start_line, end_line, kind: kind.to_string(),
                        module_path: mod_path.to_string(), visibility: Visibility::Public, fields: vec![],
                    });
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ruby(source, root, file, child, mod_path, functions, types);
    }
}

fn estimate_complexity(node: Node, source: &str) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "if" | "unless" | "while" | "until" | "for" | "case" | "when"
            | "rescue" | "elsif" | "conditional" => count += 1,
            _ => {}
        }
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}

fn collect_calls(node: Node, source: &str) -> Vec<super::CallSite> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort_by(|a, b| a.callee.cmp(&b.callee).then(a.line.cmp(&b.line)));
    calls.dedup_by(|a, b| a.callee == b.callee && a.qualifier == b.qualifier);
    calls
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<super::CallSite>) {
    if node.kind() == "call" {
        let line = node.start_position().row as u32 + 1;
        let callee = node.child_by_field_name("method")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .unwrap_or("").to_string();
        let qualifier = node.child_by_field_name("receiver")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string());
        if !callee.is_empty() {
            calls.push(super::CallSite { callee, qualifier, line });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_inner(child, source, calls);
    }
}
