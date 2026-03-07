use std::fs;
use std::path::Path;

use anyhow::Result;
use ast_grep_language::{LanguageExt, SupportLang};
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative, walk_supersearch,
    AstMatch, IndexedEndpoint, IndexedFunction, IndexedType, LanguageAnalyzer, NodeKinds,
    Visibility,
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

    fn extract_imports(&self, source: &str) -> Vec<String> {
        let mut imports = Vec::new();
        for line in source.lines() {
            let t = line.trim();
            // import ... from 'path' or "path"
            if t.starts_with("import ") {
                if let Some(from_idx) = t.rfind(" from ") {
                    let raw = t[from_idx + 6..].trim().trim_matches(&['\'', '"', ';'][..]);
                    if !raw.is_empty() { imports.push(raw.to_string()); }
                }
            }
            // require('path') or require("path")
            if let Some(s) = t.find("require(") {
                let rest = t[s + 8..].trim();
                if let Some(q) = rest.chars().next() {
                    if q == '\'' || q == '"' {
                        let inner = &rest[1..];
                        if let Some(end) = inner.find(q) {
                            let path = &inner[..end];
                            if !path.is_empty() { imports.push(path.to_string()); }
                        }
                    }
                }
            }
        }
        imports
    }

    fn analyze_file(
        &self,
        root: &Path,
        file: &Path,
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>, Vec<crate::lang::IndexedImpl>)> {
        let source = fs::read_to_string(file)?;
        let mut parser = Parser::new();
        parser
            .set_language(&SupportLang::TypeScript.get_ts_language())
            .expect("failed to load TypeScript grammar");
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;
        let mut functions = Vec::new();
        let mut endpoints = Vec::new();
        let mut types = Vec::new();
        let rel_file = relative(root, file);
        let mod_path = module_path_from_file(&rel_file, "typescript");
        walk_ts(&source, root, file, tree.root_node(), &mod_path, &mut functions, &mut endpoints, &mut types);
        Ok((functions, endpoints, types, vec![]))
    }

    fn supports_ast_search(&self) -> bool {
        true
    }

    fn ast_search(&self, source: &str, query_lc: &str, context: &str, pattern: &str) -> Vec<AstMatch> {
        let mut parser = Parser::new();
        if parser.set_language(&SupportLang::TypeScript.get_ts_language()).is_err() {
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
    mod_path: &str,
    functions: &mut Vec<IndexedFunction>,
    endpoints: &mut Vec<IndexedEndpoint>,
    types: &mut Vec<IndexedType>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                push_function(source, root, file, node, name_node, mod_path, functions);
            }
        }
        "method_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    push_function(source, root, file, node, name_node, mod_path, functions);
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
                            let (byte_start, byte_end) = byte_range(&value);
                            let qname = qualified_name(mod_path, &name, "typescript");
                            functions.push(IndexedFunction {
                                name,
                                file: relative(root, file),
                                language: "typescript".to_string(),
                                start_line,
                                end_line,
                                complexity: estimate_complexity(value, source),
                                calls: collect_calls(value, source),
                                byte_start,
                                byte_end,
                                module_path: mod_path.to_string(),
                                qualified_name: qname,
                                visibility: Visibility::Public,
                                parent_type: None,
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
                            let (byte_start, byte_end) = byte_range(&right);
                            let qname = qualified_name(mod_path, &name, "typescript");
                            functions.push(IndexedFunction {
                                name,
                                file: relative(root, file),
                                language: "typescript".to_string(),
                                start_line,
                                end_line,
                                complexity: estimate_complexity(right, source),
                                calls: collect_calls(right, source),
                                byte_start,
                                byte_end,
                                module_path: mod_path.to_string(),
                                qualified_name: qname,
                                visibility: Visibility::Public,
                                parent_type: None,
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
                        name, file: relative(root, file), language: "typescript".to_string(),
                        start_line, end_line, kind: "class".to_string(),
                        module_path: mod_path.to_string(), visibility: Visibility::Public, fields: vec![],
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
                        name, file: relative(root, file), language: "typescript".to_string(),
                        start_line, end_line, kind: "interface".to_string(),
                        module_path: mod_path.to_string(), visibility: Visibility::Public, fields: vec![],
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
                        name, file: relative(root, file), language: "typescript".to_string(),
                        start_line, end_line, kind: "type".to_string(),
                        module_path: mod_path.to_string(), visibility: Visibility::Public, fields: vec![],
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
                        name, file: relative(root, file), language: "typescript".to_string(),
                        start_line, end_line, kind: "enum".to_string(),
                        module_path: mod_path.to_string(), visibility: Visibility::Public, fields: vec![],
                    });
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ts(source, root, file, child, mod_path, functions, endpoints, types);
    }
}

fn push_function(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    name_node: Node,
    mod_path: &str,
    functions: &mut Vec<IndexedFunction>,
) {
    let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
    if name.is_empty() {
        return;
    }
    let (start_line, end_line) = line_range(&node);
    let (byte_start, byte_end) = byte_range(&node);
    let qname = qualified_name(mod_path, &name, "typescript");
    functions.push(IndexedFunction {
        name,
        file: relative(root, file),
        language: "typescript".to_string(),
        start_line,
        end_line,
        complexity: estimate_complexity(node, source),
        calls: collect_calls(node, source),
        byte_start,
        byte_end,
        module_path: mod_path.to_string(),
        qualified_name: qname,
        visibility: Visibility::Public,
        parent_type: None,
    });
}

fn collect_calls(node: Node, source: &str) -> Vec<crate::lang::CallSite> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort_by(|a, b| a.callee.cmp(&b.callee).then(a.line.cmp(&b.line)));
    calls.dedup_by(|a, b| a.callee == b.callee && a.qualifier == b.qualifier);
    calls
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<crate::lang::CallSite>) {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child_by_field_name("function") {
            let line = node.start_position().row as u32 + 1;
            let (callee, qualifier) = match func.kind() {
                "identifier" => {
                    (func.utf8_text(source.as_bytes()).unwrap_or("").to_string(), None)
                }
                "member_expression" => {
                    let callee = func
                        .child_by_field_name("property")
                        .and_then(|p| p.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("")
                        .to_string();
                    let qualifier = func
                        .child_by_field_name("object")
                        .and_then(|o| o.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());
                    (callee, qualifier)
                }
                _ => (String::new(), None),
            };
            if !callee.is_empty() {
                calls.push(crate::lang::CallSite { callee, qualifier, line });
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