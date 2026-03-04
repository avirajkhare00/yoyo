use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};
use tree_sitter_rust::LANGUAGE;

use super::{
    line_range, relative, walk_supersearch, AstMatch, IndexedEndpoint, IndexedFunction,
    IndexedType, LanguageAnalyzer, NodeKinds,
};

pub struct RustAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["identifier", "field_identifier", "type_identifier"],
    strings: &["string_literal"],
    comments: &["line_comment", "block_comment"],
    calls: &["call_expression"],
    assigns: &["assignment_expression", "let_declaration"],
    returns: &["return_expression"],
};

impl LanguageAnalyzer for RustAnalyzer {
    fn language(&self) -> &str {
        "rust"
    }

    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn analyze_file(
        &self,
        root: &Path,
        file: &Path,
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>)> {
        let source = fs::read_to_string(file)?;
        let mut parser = Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("failed to load Rust grammar");
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;

        let mut functions = Vec::new();
        let mut endpoints = Vec::new();
        let mut types = Vec::new();
        let root_node = tree.root_node();

        scan_children(&source, root, file, root_node, &mut functions, &mut endpoints, &mut types);
        let mut cursor = root_node.walk();
        for child in root_node.children(&mut cursor) {
            if child.kind() == "impl_item" {
                if let Some(body) = child.child_by_field_name("body") {
                    scan_children(&source, root, file, body, &mut functions, &mut endpoints, &mut types);
                }
            }
        }

        Ok((functions, endpoints, types))
    }

    fn supports_ast_search(&self) -> bool {
        true
    }

    fn ast_search(&self, source: &str, query_lc: &str, context: &str, pattern: &str) -> Vec<AstMatch> {
        let mut parser = Parser::new();
        if parser.set_language(&LANGUAGE.into()).is_err() {
            return vec![];
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };
        let lines: Vec<&str> = source.lines().collect();
        let mut matches = Vec::new();
        walk_supersearch(
            tree.root_node(), source, &lines, query_lc, context, pattern,
            false, false, false, &KINDS, &mut matches,
        );
        matches
    }
}

fn scan_children(
    source: &str,
    root_path: &Path,
    file: &Path,
    parent: Node,
    functions: &mut Vec<IndexedFunction>,
    endpoints: &mut Vec<IndexedEndpoint>,
    types: &mut Vec<IndexedType>,
) {
    let mut cursor = parent.walk();
    let children: Vec<Node> = parent.children(&mut cursor).collect();
    let mut pending_http: Option<(String, String)> = None;

    for child in children {
        match child.kind() {
            "attribute_item" => {
                if let Some(attr) = extract_http_attr(source, child) {
                    pending_http = Some(attr);
                }
            }
            "function_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        let (start_line, end_line) = line_range(&child);
                        functions.push(IndexedFunction {
                            name: name.to_string(),
                            file: relative(root_path, file),
                            language: "rust".to_string(),
                            start_line,
                            end_line,
                            complexity: estimate_complexity(child, source),
                            calls: collect_calls(child, source),
                        });
                        if let Some((method, path)) = pending_http.take() {
                            endpoints.push(IndexedEndpoint {
                                method,
                                path,
                                file: relative(root_path, file),
                                handler_name: Some(name.to_string()),
                                language: "rust".to_string(),
                                framework: "actix/rocket".to_string(),
                            });
                        }
                    }
                }
                pending_http = None;
            }
            "struct_item" | "enum_item" | "trait_item" | "type_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        let (start_line, end_line) = line_range(&child);
                        let kind = match child.kind() {
                            "struct_item" => "struct",
                            "enum_item"   => "enum",
                            "trait_item"  => "trait",
                            _             => "type",
                        };
                        types.push(IndexedType {
                            name: name.to_string(),
                            file: relative(root_path, file),
                            language: "rust".to_string(),
                            start_line,
                            end_line,
                            kind: kind.to_string(),
                        });
                    }
                }
                pending_http = None;
            }
            "line_comment" | "block_comment" => {}
            _ => {
                pending_http = None;
            }
        }
    }
}

fn collect_calls(node: Node, source: &str) -> Vec<String> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort();
    calls.dedup();
    calls
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<String>) {
    match node.kind() {
        "call_expression" => {
            if let Some(func) = node.child_by_field_name("function") {
                let name = match func.kind() {
                    "identifier" => func.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
                    "scoped_identifier" => func
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("")
                        .to_string(),
                    "field_expression" => func
                        .child_by_field_name("field")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("")
                        .to_string(),
                    _ => String::new(),
                };
                if !name.is_empty() {
                    calls.push(name);
                }
            }
        }
        "method_call_expression" => {
            if let Some(method) = node.child_by_field_name("method") {
                let name = method.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    calls.push(name);
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_inner(child, source, calls);
    }
}

fn extract_http_attr(source: &str, node: Node) -> Option<(String, String)> {
    let attr = node.named_child(0)?;
    let name_node = attr.named_child(0)?;
    let name = name_node.utf8_text(source.as_bytes()).ok()?;
    let method = match name.to_lowercase().as_str() {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => name.to_uppercase(),
        _ => return None,
    };
    let args = attr.child_by_field_name("arguments")?;
    let path = find_string_in_token_tree(source, args)?;
    Some((method, path))
}

fn find_string_in_token_tree(source: &str, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_literal" {
            let text = child.utf8_text(source.as_bytes()).ok()?;
            return Some(text.trim_matches('"').to_string());
        }
        if child.kind() == "token_tree" {
            if let Some(s) = find_string_in_token_tree(source, child) {
                return Some(s);
            }
        }
    }
    None
}

fn estimate_complexity(node: Node, source: &str) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "if_expression"
            | "match_expression"
            | "while_expression"
            | "for_expression"
            | "loop_expression" => count += 1,
            _ => {}
        }
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}
