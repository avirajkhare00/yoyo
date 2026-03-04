use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};
use tree_sitter_go::LANGUAGE;

use super::{
    line_range, relative, walk_supersearch, AstMatch, IndexedEndpoint, IndexedFunction,
    IndexedType, LanguageAnalyzer, NodeKinds,
};

pub struct GoAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["identifier", "field_identifier", "type_identifier", "package_identifier"],
    strings: &["interpreted_string_literal", "raw_string_literal"],
    comments: &["comment"],
    calls: &["call_expression"],
    assigns: &["short_var_declaration", "assignment_statement"],
    returns: &["return_statement"],
};

impl LanguageAnalyzer for GoAnalyzer {
    fn language(&self) -> &str {
        "go"
    }

    fn extensions(&self) -> &[&str] {
        &["go"]
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
            .expect("failed to load Go grammar");
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;

        let mut functions = Vec::new();
        let mut endpoints = Vec::new();
        let mut types = Vec::new();
        walk_go(&source, root, file, tree.root_node(), &mut functions, &mut endpoints, &mut types);
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

fn walk_go(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    functions: &mut Vec<IndexedFunction>,
    endpoints: &mut Vec<IndexedEndpoint>,
    types: &mut Vec<IndexedType>,
) {
    match node.kind() {
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                        if !name.is_empty() {
                            let kind = child
                                .child_by_field_name("type")
                                .map(|t| match t.kind() {
                                    "struct_type" => "struct",
                                    "interface_type" => "interface",
                                    _ => "type",
                                })
                                .unwrap_or("type");
                            let (start_line, end_line) = line_range(&child);
                            types.push(IndexedType {
                                name,
                                file: relative(root, file),
                                language: "go".to_string(),
                                start_line,
                                end_line,
                                kind: kind.to_string(),
                            });
                        }
                    }
                }
            }
        }
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                let (start_line, end_line) = line_range(&node);
                functions.push(IndexedFunction {
                    name,
                    file: relative(root, file),
                    language: "go".to_string(),
                    start_line,
                    end_line,
                    complexity: estimate_complexity(node, source),
                    calls: collect_calls(node, source),
                });
            }
        }
        "method_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                let (start_line, end_line) = line_range(&node);
                functions.push(IndexedFunction {
                    name,
                    file: relative(root, file),
                    language: "go".to_string(),
                    start_line,
                    end_line,
                    complexity: estimate_complexity(node, source),
                    calls: collect_calls(node, source),
                });
            }
        }
        "call_expression" => {
            // Detect HTTP routes: r.GET("/path", ...), router.POST(...), e.GET(...), etc.
            if let Some(ep) = extract_http_route(source, node, file, root) {
                endpoints.push(ep);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_go(source, root, file, child, functions, endpoints, types);
    }
}

/// Detect gin/echo/net-http-style route registration calls.
/// Patterns: r.GET("/path", ...) | http.HandleFunc("/path", ...) | r.Handle("/path", ...)
fn extract_http_route(source: &str, node: Node, file: &Path, root: &Path) -> Option<IndexedEndpoint> {
    let func = node.child_by_field_name("function")?;

    // Must be a selector expression: <receiver>.<method>
    if func.kind() != "selector_expression" {
        return None;
    }
    let method_node = func.child_by_field_name("field")?;
    let method_name = method_node.utf8_text(source.as_bytes()).ok()?;

    let http_method = match method_name.to_uppercase().as_str() {
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" => {
            method_name.to_uppercase()
        }
        "HANDLEFUNC" | "HANDLE" => "ANY".to_string(),
        _ => return None,
    };

    let args = node.child_by_field_name("arguments")?;
    let route_path = first_string_arg(source, args)?;

    Some(IndexedEndpoint {
        method: http_method,
        path: route_path,
        file: relative(root, file),
        handler_name: None,
        language: "go".to_string(),
        framework: "gin/echo/net-http".to_string(),
    })
}

/// Return the first string literal argument from an `argument_list` node.
fn first_string_arg(source: &str, args: Node) -> Option<String> {
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        let kind = child.kind();
        if kind == "interpreted_string_literal" || kind == "raw_string_literal" {
            let raw = child.utf8_text(source.as_bytes()).ok()?;
            let stripped = raw.trim_matches('"').trim_matches('`');
            return Some(stripped.to_string());
        }
    }
    None
}

fn estimate_complexity(node: Node, source: &str) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "if_statement"
            | "for_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement"
            | "communication_case" => count += 1,
            _ => {}
        }
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}

fn collect_calls(node: Node, source: &str) -> Vec<String> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort();
    calls.dedup();
    calls
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<String>) {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child_by_field_name("function") {
            let name = match func.kind() {
                "identifier" => func.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
                "selector_expression" => func
                    .child_by_field_name("field")
                    .and_then(|f| f.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            };
            if !name.is_empty() {
                calls.push(name);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_inner(child, source, calls);
    }
}
