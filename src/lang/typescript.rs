use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};
use tree_sitter_typescript::LANGUAGE_TYPESCRIPT;

use super::{
    line_range, relative, walk_supersearch, AstMatch, IndexedEndpoint, IndexedFunction,
    IndexedType, LanguageAnalyzer, NodeKinds,
};

pub struct TypeScriptAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["identifier", "property_identifier", "shorthand_property_identifier"],
    strings: &["string"],
    comments: &["comment"],
    calls: &["call_expression"],
    assigns: &["assignment_expression", "variable_declarator"],
    returns: &["return_statement"],
};

impl LanguageAnalyzer for TypeScriptAnalyzer {
    fn language(&self) -> &str {
        "typescript"
    }

    fn extensions(&self) -> &[&str] {
        &["ts", "tsx"]
    }

    fn analyze_file(
        &self,
        root: &Path,
        file: &Path,
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>)> {
        let source = fs::read_to_string(file)?;
        let mut parser = Parser::new();
        parser
            .set_language(&LANGUAGE_TYPESCRIPT.into())
            .expect("failed to load TypeScript grammar");
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;
        let mut functions = Vec::new();
        let mut endpoints = Vec::new();
        let mut types = Vec::new();
        walk_ts(&source, root, file, tree.root_node(), &mut functions, &mut endpoints, &mut types);
        Ok((functions, endpoints, types))
    }

    fn supports_ast_search(&self) -> bool {
        true
    }

    fn ast_search(&self, source: &str, query_lc: &str, context: &str, pattern: &str) -> Vec<AstMatch> {
        let mut parser = Parser::new();
        if parser.set_language(&LANGUAGE_TYPESCRIPT.into()).is_err() {
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

fn walk_ts(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    functions: &mut Vec<IndexedFunction>,
    endpoints: &mut Vec<IndexedEndpoint>,
    types: &mut Vec<IndexedType>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                push_function(source, root, file, node, name_node, functions);
            }
        }
        "method_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    push_function(source, root, file, node, name_node, functions);
                }
            }
        }
        "variable_declarator" => {
            if let Some(value) = node.child_by_field_name("value") {
                if value.kind() == "arrow_function" {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = name_from_declarator(name_node, source);
                        if !name.is_empty() {
                            let (start_line, end_line) = line_range(&value);
                            functions.push(IndexedFunction {
                                name,
                                file: relative(root, file),
                                language: "typescript".to_string(),
                                start_line,
                                end_line,
                                complexity: estimate_complexity(value, source),
                                calls: collect_calls(value, source),
                            });
                        }
                    }
                }
            }
        }
        "assignment_expression" => {
            if let Some(right) = node.child_by_field_name("right") {
                if right.kind() == "arrow_function" {
                    if let Some(left) = node.child_by_field_name("left") {
                        let name = left.utf8_text(source.as_bytes()).unwrap_or("").trim().to_string();
                        if !name.is_empty() {
                            let (start_line, end_line) = line_range(&right);
                            functions.push(IndexedFunction {
                                name,
                                file: relative(root, file),
                                language: "typescript".to_string(),
                                start_line,
                                end_line,
                                complexity: estimate_complexity(right, source),
                                calls: collect_calls(right, source),
                            });
                        }
                    }
                }
            }
        }
        "call_expression" => {
            detect_express_call(source, root, file, node, endpoints);
        }
        "class_declaration" | "abstract_class_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "class".to_string(),
                    });
                }
            }
        }
        "interface_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "interface".to_string(),
                    });
                }
            }
        }
        "type_alias_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "type".to_string(),
                    });
                }
            }
        }
        "enum_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "enum".to_string(),
                    });
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ts(source, root, file, child, functions, endpoints, types);
    }
}

fn push_function(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    name_node: Node,
    functions: &mut Vec<IndexedFunction>,
) {
    let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
    if name.is_empty() {
        return;
    }
    let (start_line, end_line) = line_range(&node);
    functions.push(IndexedFunction {
        name,
        file: relative(root, file),
        language: "typescript".to_string(),
        start_line,
        end_line,
        complexity: estimate_complexity(node, source),
        calls: collect_calls(node, source),
    });
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
                "member_expression" => func
                    .child_by_field_name("property")
                    .and_then(|p| p.utf8_text(source.as_bytes()).ok())
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

/// Get a single name from a declarator (identifier, or "constructor" for class property assign).
fn name_from_declarator(name_node: Node, source: &str) -> String {
    match name_node.kind() {
        "identifier" => name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
        "object_pattern" | "array_pattern" => {
            // Named arrow from destructuring: const { foo } = ...; we don't index as "foo" here.
            String::new()
        }
        _ => name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
    }
}

fn estimate_complexity(node: Node, source: &str) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "conditional_expression" => count += 1,
            _ => {}
        }
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}

fn detect_express_call(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    endpoints: &mut Vec<IndexedEndpoint>,
) {
    let callee = match node.child_by_field_name("function") {
        Some(n) if n.kind() == "member_expression" => n,
        _ => return,
    };
    let prop = match callee.child_by_field_name("property") {
        Some(p) => p,
        None => return,
    };
    let method = prop.utf8_text(source.as_bytes()).unwrap_or("").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS") {
        return;
    }
    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return,
    };
    if let Some(first) = args.named_child(0) {
        if first.kind() == "string" {
            let raw = first.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            let path = raw.trim_matches(&['"', '\''][..]).to_string();
            let handler_name = args
                .named_child(1)
                .and_then(|h| h.utf8_text(source.as_bytes()).ok())
                .map(|s| s.to_string());
            endpoints.push(IndexedEndpoint {
                method,
                path,
                file: relative(root, file),
                handler_name,
                language: "typescript".to_string(),
                framework: "express".to_string(),
            });
        }
    }
}