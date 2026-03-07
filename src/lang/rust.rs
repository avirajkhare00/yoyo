use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};
use tree_sitter_rust::LANGUAGE;

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative, walk_supersearch,
    AstMatch, FieldInfo, IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType,
    LanguageAnalyzer, NodeKinds, Visibility,
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

    fn extract_imports(&self, source: &str) -> Vec<String> {
        source.lines()
            .filter_map(|line| {
                let t = line.trim();
                if t.starts_with("use ") {
                    Some(t.trim_start_matches("use ").trim_end_matches(';').trim().to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn analyze_file(
        &self,
        root: &Path,
        file: &Path,
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>, Vec<IndexedImpl>)> {
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
        let mut impls: Vec<IndexedImpl> = Vec::new();
        let root_node = tree.root_node();
        let rel_file = relative(root, file);
        let mod_path = module_path_from_file(&rel_file, "rust");

        scan_children(&source, root, file, root_node, &mod_path, None, &mut functions, &mut endpoints, &mut types);
        let mut cursor = root_node.walk();
        for child in root_node.children(&mut cursor) {
            if child.kind() == "impl_item" {
                let type_name = child
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let trait_name = child
                    .child_by_field_name("trait")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                if let Some(type_name) = type_name {
                    let (start_line, _) = line_range(&child);
                    impls.push(IndexedImpl {
                        type_name: type_name.clone(),
                        trait_name,
                        file: relative(root, file),
                        start_line,
                    });
                    if let Some(body) = child.child_by_field_name("body") {
                        scan_children(&source, root, file, body, &mod_path, Some(&type_name), &mut functions, &mut endpoints, &mut types);
                    }
                }
            }
        }

        Ok((functions, endpoints, types, impls))
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

fn rust_visibility(node: Node, source: &str) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            return if text == "pub" { Visibility::Public } else { Visibility::Module };
        }
    }
    Visibility::Private
}

fn scan_children(
    source: &str,
    root_path: &Path,
    file: &Path,
    parent: Node,
    mod_path: &str,
    parent_type: Option<&str>,
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
                        let (byte_start, byte_end) = byte_range(&child);
                        let vis = rust_visibility(child, source);
                        let qname = qualified_name(mod_path, name, "rust");
                        functions.push(IndexedFunction {
                            name: name.to_string(),
                            file: relative(root_path, file),
                            language: "rust".to_string(),
                            start_line,
                            end_line,
                            complexity: estimate_complexity(child, source),
                            calls: collect_calls(child, source),
                            byte_start,
                            byte_end,
                            module_path: mod_path.to_string(),
                            qualified_name: qname,
                            visibility: vis,
                            parent_type: parent_type.map(str::to_string),
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
                        let vis = rust_visibility(child, source);
                        let fields = if kind == "struct" {
                            collect_struct_fields(&child, source)
                        } else {
                            vec![]
                        };
                        types.push(IndexedType {
                            name: name.to_string(),
                            file: relative(root_path, file),
                            language: "rust".to_string(),
                            start_line,
                            end_line,
                            kind: kind.to_string(),
                            module_path: mod_path.to_string(),
                            visibility: vis,
                            fields,
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

fn collect_calls(node: Node, source: &str) -> Vec<crate::lang::CallSite> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort_by(|a, b| a.callee.cmp(&b.callee).then(a.line.cmp(&b.line)));
    calls.dedup_by(|a, b| a.callee == b.callee && a.qualifier == b.qualifier);
    calls
}

/// Walk up a receiver/value chain to find the root identifier.
/// `db.query(...).fetch_one(...)` → `"db"` instead of `"db.query(...)"`.
fn root_receiver<'a>(node: Node<'a>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "self" => node.utf8_text(source.as_bytes()).ok().map(str::to_string),
        "method_call_expression" => node
            .child_by_field_name("receiver")
            .and_then(|r| root_receiver(r, source)),
        "call_expression" => node
            .child_by_field_name("function")
            .and_then(|f| root_receiver(f, source)),
        "field_expression" => node
            .child_by_field_name("value")
            .and_then(|v| root_receiver(v, source)),
        "await_expression" => node
            .named_child(0)
            .and_then(|c| root_receiver(c, source)),
        _ => None,
    }
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<crate::lang::CallSite>) {
    let line = node.start_position().row as u32 + 1;
    match node.kind() {
        "call_expression" => {
            if let Some(func) = node.child_by_field_name("function") {
                let (callee, qualifier) = match func.kind() {
                    "identifier" => {
                        (func.utf8_text(source.as_bytes()).unwrap_or("").to_string(), None)
                    }
                    "scoped_identifier" => {
                        let callee = func
                            .child_by_field_name("name")
                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                            .unwrap_or("")
                            .to_string();
                        let qualifier = func
                            .child_by_field_name("path")
                            .and_then(|p| p.utf8_text(source.as_bytes()).ok())
                            .map(|s| s.to_string());
                        (callee, qualifier)
                    }
                    "field_expression" => {
                        let callee = func
                            .child_by_field_name("field")
                            .and_then(|f| f.utf8_text(source.as_bytes()).ok())
                            .unwrap_or("")
                            .to_string();
                        let qualifier = func
                            .child_by_field_name("value")
                            .and_then(|v| root_receiver(v, source));
                        (callee, qualifier)
                    }
                    _ => (String::new(), None),
                };
                if !callee.is_empty() {
                    calls.push(crate::lang::CallSite { callee, qualifier, line });
                }
            }
        }
        "method_call_expression" => {
            if let Some(method) = node.child_by_field_name("method") {
                let callee = method.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                let qualifier = node
                    .child_by_field_name("receiver")
                    .and_then(|r| root_receiver(r, source));
                if !callee.is_empty() {
                    calls.push(crate::lang::CallSite { callee, qualifier, line });
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

/// Extract named fields from a struct_item node.
fn collect_struct_fields(struct_node: &Node, source: &str) -> Vec<FieldInfo> {
    let mut fields = Vec::new();
    let mut cursor = struct_node.walk();
    for child in struct_node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let mut fc = child.walk();
            for field in child.children(&mut fc) {
                if field.kind() == "field_declaration" {
                    let name = field
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("")
                        .to_string();
                    let type_str = field
                        .child_by_field_name("type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !name.is_empty() {
                        let vis = rust_visibility(field, source);
                        fields.push(FieldInfo { name, type_str, visibility: vis });
                    }
                }
            }
        }
    }
    fields
}
