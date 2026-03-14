use std::fs;
use std::path::Path;

use anyhow::Result;
use ast_grep_language::{LanguageExt, SupportLang};
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative, IndexedEndpoint,
    IndexedFunction, IndexedType, LanguageAnalyzer, NodeKinds, SignatureParam, Visibility,
};

pub struct TypeScriptAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &[
        "identifier",
        "property_identifier",
        "shorthand_property_identifier",
    ],
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

    fn extract_imports(&self, source: &str) -> Vec<String> {
        let mut imports = Vec::new();
        for line in source.lines() {
            let t = line.trim();
            // import ... from 'path' or "path"
            if t.starts_with("import ") {
                if let Some(from_idx) = t.rfind(" from ") {
                    let raw = t[from_idx + 6..].trim().trim_matches(&['\'', '"', ';'][..]);
                    if !raw.is_empty() {
                        imports.push(raw.to_string());
                    }
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
                            if !path.is_empty() {
                                imports.push(path.to_string());
                            }
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
    ) -> Result<(
        Vec<IndexedFunction>,
        Vec<IndexedEndpoint>,
        Vec<IndexedType>,
        Vec<crate::lang::IndexedImpl>,
    )> {
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
        walk_ts(
            &source,
            root,
            file,
            tree.root_node(),
            &mod_path,
            &mut functions,
            &mut endpoints,
            &mut types,
        );
        Ok((functions, endpoints, types, vec![]))
    }

    fn ts_language(&self) -> Option<tree_sitter::Language> {
        Some(SupportLang::TypeScript.get_ts_language())
    }
    fn node_kinds(&self) -> Option<&'static crate::lang::NodeKinds> {
        Some(&KINDS)
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
                let name = name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string();
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
                            let (params, return_type) = ts_signature_parts(value, source);
                            let param_types: Vec<String> =
                                params.iter().map(|p| p.type_str.clone()).collect();
                            let sig_hash = super::compute_sig_hash(
                                &param_types,
                                return_type.as_deref().unwrap_or_default(),
                            );
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
                                params,
                                return_type,
                                sig_hash: Some(sig_hash),
                                ..Default::default()
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
                        let name = left
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !name.is_empty() {
                            let (start_line, end_line) = line_range(&right);
                            let (byte_start, byte_end) = byte_range(&right);
                            let qname = qualified_name(mod_path, &name, "typescript");
                            let (params, return_type) = ts_signature_parts(right, source);
                            let param_types: Vec<String> =
                                params.iter().map(|p| p.type_str.clone()).collect();
                            let sig_hash = super::compute_sig_hash(
                                &param_types,
                                return_type.as_deref().unwrap_or_default(),
                            );
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
                                params,
                                return_type,
                                sig_hash: Some(sig_hash),
                                ..Default::default()
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
                let name = name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "class".to_string(),
                        module_path: mod_path.to_string(),
                        visibility: Visibility::Public,
                        fields: vec![],
                        ..Default::default()
                    });
                }
            }
        }
        "interface_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "interface".to_string(),
                        module_path: mod_path.to_string(),
                        visibility: Visibility::Public,
                        fields: vec![],
                        ..Default::default()
                    });
                }
            }
        }
        "type_alias_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "type".to_string(),
                        module_path: mod_path.to_string(),
                        visibility: Visibility::Public,
                        fields: vec![],
                        ..Default::default()
                    });
                }
            }
        }
        "enum_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "typescript".to_string(),
                        start_line,
                        end_line,
                        kind: "enum".to_string(),
                        module_path: mod_path.to_string(),
                        visibility: Visibility::Public,
                        fields: vec![],
                        ..Default::default()
                    });
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ts(
            source, root, file, child, mod_path, functions, endpoints, types,
        );
    }
}

fn node_text(node: Node, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn ts_type_text(node: Node, source: &str) -> Option<String> {
    node_text(node, source)
        .map(|s| s.trim_start_matches(':').trim().to_string())
        .filter(|s| !s.is_empty())
}

fn ts_parameter_name(node: Node, source: &str) -> Option<String> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))
        .and_then(|n| node_text(n, source))
}

fn ts_parameter_list(node: Node, source: &str) -> Vec<SignatureParam> {
    let mut params = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "required_parameter" | "optional_parameter" => {
                let mut name = ts_parameter_name(child, source).unwrap_or_default();
                if child.kind() == "optional_parameter" && !name.is_empty() && !name.ends_with('?')
                {
                    name.push('?');
                }
                let type_str = child
                    .child_by_field_name("type")
                    .and_then(|n| ts_type_text(n, source))
                    .unwrap_or_default();
                params.push(SignatureParam { name, type_str });
            }
            _ => {}
        }
    }
    params
}

fn ts_signature_parts(node: Node, source: &str) -> (Vec<SignatureParam>, Option<String>) {
    let params = node
        .child_by_field_name("parameters")
        .map(|n| ts_parameter_list(n, source))
        .or_else(|| {
            node.child_by_field_name("parameter").map(|n| {
                vec![SignatureParam {
                    name: node_text(n, source).unwrap_or_default(),
                    type_str: String::new(),
                }]
            })
        })
        .unwrap_or_default();
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|n| ts_type_text(n, source));
    (params, return_type)
}

fn ts_parent_type(node: Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "class_declaration" | "abstract_class_declaration" | "interface_declaration" => {
                return parent
                    .child_by_field_name("name")
                    .and_then(|n| node_text(n, source));
            }
            _ => current = parent.parent(),
        }
    }
    None
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
    let name = name_node
        .utf8_text(source.as_bytes())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return;
    }
    let (start_line, end_line) = line_range(&node);
    let (byte_start, byte_end) = byte_range(&node);
    let qname = qualified_name(mod_path, &name, "typescript");
    let (params, return_type) = ts_signature_parts(node, source);
    let param_types: Vec<String> = params.iter().map(|p| p.type_str.clone()).collect();
    let sig_hash =
        super::compute_sig_hash(&param_types, return_type.as_deref().unwrap_or_default());
    let parent_type = (node.kind() == "method_definition")
        .then(|| ts_parent_type(node, source))
        .flatten();
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
        parent_type,
        params,
        return_type,
        sig_hash: Some(sig_hash),
        ..Default::default()
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
                "identifier" => (
                    func.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
                    None,
                ),
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
                calls.push(crate::lang::CallSite {
                    callee,
                    qualifier,
                    line,
                });
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
        "identifier" => name_node
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_string(),
        "object_pattern" | "array_pattern" => {
            // Named arrow from destructuring: const { foo } = ...; we don't index as "foo" here.
            String::new()
        }
        _ => name_node
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_string(),
    }
}

fn estimate_complexity(node: Node, _source: &str) -> u32 {
    super::estimate_complexity_for(
        node,
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "do_statement",
            "switch_statement",
            "conditional_expression",
        ],
    )
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
    let method = prop
        .utf8_text(source.as_bytes())
        .unwrap_or("")
        .to_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS"
    ) {
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
                scope_name: String::new(),
            });
        }
    }
}
