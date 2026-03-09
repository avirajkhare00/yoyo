use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative,
    IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType, LanguageAnalyzer,
    NodeKinds, Visibility,
};

pub struct ZigAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["identifier"],
    strings: &["string", "multiline_string"],
    comments: &["comment"],
    calls: &["call_expression", "builtin_function"],
    assigns: &["assignment_expression", "variable_declaration"],
    returns: &["return_expression"],
};

impl LanguageAnalyzer for ZigAnalyzer {
    fn language(&self) -> &str {
        "zig"
    }

    fn extract_imports(&self, source: &str) -> Vec<String> {
        let mut imports = Vec::new();
        for line in source.lines() {
            let t = line.trim();
            if t.contains("@import(") {
                if let Some(start) = t.find('"') {
                    let rest = &t[start + 1..];
                    if let Some(end) = rest.find('"') {
                        let path = &rest[..end];
                        if !path.is_empty() {
                            imports.push(path.to_string());
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
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>, Vec<IndexedImpl>)>
    {
        let source = fs::read_to_string(file)?;
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_zig::LANGUAGE.into())
            .expect("failed to load Zig grammar");
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;

        let rel = relative(root, file);
        let mod_path = module_path_from_file(&rel, "zig");
        let mut functions = Vec::new();
        let mut types = Vec::new();

        walk_zig(
            &source,
            root,
            file,
            tree.root_node(),
            &mod_path,
            &mut functions,
            &mut types,
        );

        Ok((functions, vec![], types, vec![]))
    }

    fn ts_language(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_zig::LANGUAGE.into())
    }
    fn node_kinds(&self) -> Option<&'static crate::lang::NodeKinds> {
        Some(&KINDS)
    }
}

/// Check if a node has a `pub` keyword child (first child).
fn has_pub(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "pub" {
            return true;
        }
        // pub is always the first token if present; stop after first real token
        if child.is_named() {
            break;
        }
        let text = child.utf8_text(source.as_bytes()).unwrap_or("");
        if text != "pub" && !text.is_empty() {
            break;
        }
    }
    false
}

fn zig_visibility(node: Node, source: &str) -> Visibility {
    if has_pub(node, source) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn walk_zig(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    mod_path: &str,
    functions: &mut Vec<IndexedFunction>,
    types: &mut Vec<IndexedType>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    let (byte_start, byte_end) = byte_range(&node);
                    let vis = zig_visibility(node, source);
                    let qname = qualified_name(mod_path, &name, "zig");
                    functions.push(IndexedFunction {
                        name,
                        file: relative(root, file),
                        language: "zig".to_string(),
                        start_line,
                        end_line,
                        complexity: estimate_complexity(node, source),
                        calls: collect_calls(node, source),
                        byte_start,
                        byte_end,
                        module_path: mod_path.to_string(),
                        qualified_name: qname,
                        visibility: vis,
                        parent_type: None,
                    });
                }
            }
        }
        "variable_declaration" => {
            // Detect `const Name = struct/enum/union/opaque { ... };`
            if let Some((type_name, kind)) = extract_type_decl(node, source) {
                let (start_line, end_line) = line_range(&node);
                let vis = zig_visibility(node, source);
                types.push(IndexedType {
                    name: type_name,
                    file: relative(root, file),
                    language: "zig".to_string(),
                    start_line,
                    end_line,
                    kind,
                    module_path: mod_path.to_string(),
                    visibility: vis,
                    fields: vec![],
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_zig(source, root, file, child, mod_path, functions, types);
    }
}

/// For a `variable_declaration` node, check if its value is a type declaration.
/// Returns (name, kind) if so.
fn extract_type_decl(node: Node, source: &str) -> Option<(String, String)> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    // Scan for the first identifier (the variable name) and a type declaration child.
    let mut name: Option<String> = None;
    let mut kind: Option<&str> = None;

    for child in &children {
        match child.kind() {
            "identifier" if name.is_none() => {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !text.is_empty() {
                    name = Some(text);
                }
            }
            "struct_declaration" => { kind = Some("struct"); }
            "enum_declaration" => { kind = Some("enum"); }
            "union_declaration" => { kind = Some("union"); }
            "opaque_declaration" => { kind = Some("opaque"); }
            _ => {}
        }
    }

    match (name, kind) {
        (Some(n), Some(k)) => Some((n, k.to_string())),
        _ => None,
    }
}

fn estimate_complexity(node: Node, _source: &str) -> u32 {
    super::estimate_complexity_for(node, &[
        "if_expression", "if_statement", "for_expression", "for_statement",
        "while_expression", "while_statement", "switch_expression",
        "catch_expression", "try_expression",
    ])
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
                    let name = func.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                    (name, None)
                }
                "field_expression" => {
                    // obj.method(...)
                    let mut cur = func.walk();
                    let parts: Vec<Node> = func.named_children(&mut cur).collect();
                    let callee = parts
                        .last()
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("")
                        .to_string();
                    let qualifier = parts
                        .first()
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());
                    (callee, qualifier)
                }
                _ => {
                    let text = func.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                    (text, None)
                }
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
