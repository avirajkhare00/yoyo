use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use ast_grep_language::{LanguageExt, SupportLang};
use serde::Deserialize;
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative,
    FieldInfo, IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType, SignatureParam,
    LanguageAnalyzer, NodeKinds, Visibility,
};

pub struct RustAnalyzer;

static EXPANDED_RUST_CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();

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
            .set_language(&SupportLang::Rust.get_ts_language())
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

        let known_functions = collect_function_names(root_node, &source);

        scan_children(
            &source,
            root,
            file,
            root_node,
            &mod_path,
            None,
            None,
            &known_functions,
            &mut functions,
            &mut endpoints,
            &mut types,
        );
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
                        trait_name: trait_name.clone(),
                        file: relative(root, file),
                        start_line,
                    });
                    if let Some(body) = child.child_by_field_name("body") {
                        scan_children(
                            &source,
                            root,
                            file,
                            body,
                            &mod_path,
                            Some(&type_name),
                            trait_name.as_deref(),
                            &known_functions,
                            &mut functions,
                            &mut endpoints,
                            &mut types,
                        );
                    }
                }
            }
        }

        merge_compiler_expanded_calls(root, file, &source, &mut functions);

        Ok((functions, endpoints, types, impls))
    }

    fn ts_language(&self) -> Option<tree_sitter::Language> {
        Some(SupportLang::Rust.get_ts_language())
    }
    fn node_kinds(&self) -> Option<&'static crate::lang::NodeKinds> {
        Some(&KINDS)
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

/// Extract a structural sig_hash from a Rust function_item node.
/// Hashes (param_types, return_type) — name-agnostic.
fn rust_sig_hash(node: &tree_sitter::Node, source: &str) -> String {
    let (params, return_type, _) = rust_signature_parts(node, source);
    let param_types: Vec<String> = params.into_iter().map(|p| p.type_str).collect();
    super::compute_sig_hash(&param_types, return_type.as_deref().unwrap_or_default())
}

fn rust_signature_parts(
    node: &tree_sitter::Node,
    source: &str,
) -> (Vec<SignatureParam>, Option<String>, Option<String>) {
    let mut params: Vec<SignatureParam> = Vec::new();
    let mut receiver: Option<String> = None;
    if let Some(parameters) = node.child_by_field_name("parameters") {
        let mut cur = parameters.walk();
        for p in parameters.children(&mut cur) {
            match p.kind() {
                "parameter" => {
                    let name = p
                        .child_by_field_name("pattern")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    let type_str = p
                        .child_by_field_name("type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    if !name.is_empty() || !type_str.is_empty() {
                        params.push(SignatureParam { name, type_str });
                    }
                }
                "self_parameter" => {
                    receiver = p
                        .utf8_text(source.as_bytes())
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                }
                _ => {}
            }
        }
    }
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (params, return_type, receiver)
}

fn scan_children(
    source: &str,
    root_path: &Path,
    file: &Path,
    parent: Node,
    mod_path: &str,
    parent_type: Option<&str>,
    implemented_trait: Option<&str>,
    known_functions: &HashSet<String>,
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
                        let (params, return_type, receiver) = rust_signature_parts(&child, source);
                        let sig_hash = rust_sig_hash(&child, source);
                        functions.push(IndexedFunction {
                            name: name.to_string(),
                            file: relative(root_path, file),
                            language: "rust".to_string(),
                            start_line,
                            end_line,
                            complexity: estimate_complexity(child, source),
                            calls: collect_calls(child, source, known_functions),
                            byte_start,
                            byte_end,
                            module_path: mod_path.to_string(),
                            qualified_name: qname,
                            visibility: vis,
                            parent_type: parent_type.map(str::to_string),
                            implemented_trait: implemented_trait.map(str::to_string),
                            params,
                            return_type,
                            receiver,
                            sig_hash: Some(sig_hash),
                            ..Default::default()
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
                            ..Default::default()
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

fn collect_calls(
    node: Node,
    source: &str,
    known_functions: &HashSet<String>,
) -> Vec<crate::lang::CallSite> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, known_functions, &mut calls);
    normalize_call_sites(&mut calls);
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

fn collect_calls_inner(
    node: Node,
    source: &str,
    known_functions: &HashSet<String>,
    calls: &mut Vec<crate::lang::CallSite>,
) {
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
            if let Some(arguments) = node.child_by_field_name("arguments") {
                collect_fn_refs_from_args(arguments, source, known_functions, calls);
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
            if let Some(arguments) = node.child_by_field_name("arguments") {
                collect_fn_refs_from_args(arguments, source, known_functions, calls);
            }
        }
        "macro_invocation" => {
            collect_macro_invocation_calls(node, source, known_functions, calls);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_inner(child, source, known_functions, calls);
    }
}

fn collect_function_names(parent: Node, source: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_function_names_inner(parent, source, &mut names);
    names
}

fn collect_function_names_inner(parent: Node, source: &str, names: &mut HashSet<String>) {
    let mut cursor = parent.walk();
    for child in parent.children(&mut cursor) {
        if child.kind() == "function_item" {
            if let Some(name_node) = child.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                    names.insert(name.to_string());
                }
            }
        }
        collect_function_names_inner(child, source, names);
    }
}

fn collect_fn_refs_from_args(
    arguments: Node,
    source: &str,
    known_functions: &HashSet<String>,
    calls: &mut Vec<crate::lang::CallSite>,
) {
    let mut cursor = arguments.walk();
    for child in arguments.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    if known_functions.contains(name) {
                        calls.push(crate::lang::CallSite {
                            callee: name.to_string(),
                            qualifier: None,
                            line: child.start_position().row as u32 + 1,
                        });
                    }
                }
            }
            "scoped_identifier" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok());
                let qualifier = child
                    .child_by_field_name("path")
                    .and_then(|p| p.utf8_text(source.as_bytes()).ok());
                if let Some(name) = name {
                    if known_functions.contains(name) {
                        calls.push(crate::lang::CallSite {
                            callee: name.to_string(),
                            qualifier: qualifier.map(str::to_string),
                            line: child.start_position().row as u32 + 1,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_macro_invocation_calls(
    node: Node,
    source: &str,
    known_functions: &HashSet<String>,
    calls: &mut Vec<crate::lang::CallSite>,
) {
    let Ok(text) = node.utf8_text(source.as_bytes()) else {
        return;
    };
    let base_line = node.start_position().row as u32 + 1;

    calls.extend(scan_macro_invocations(text, base_line));
    calls.extend(scan_macro_body_calls(text, base_line, known_functions));
}

fn scan_macro_body_calls(
    text: &str,
    base_line: u32,
    known_functions: &HashSet<String>,
) -> Vec<crate::lang::CallSite> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if !is_ident_start(ch) {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;
        while i < bytes.len() && is_ident_continue(bytes[i] as char) {
            i += 1;
        }
        let token = &text[start..i];
        let mut j = i;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }

        let line = base_line + text[..start].chars().filter(|c| *c == '\n').count() as u32;

        if j < bytes.len() && bytes[j] as char == '.' {
            let mut k = j + 1;
            while k < bytes.len() && (bytes[k] as char).is_whitespace() {
                k += 1;
            }
            if k < bytes.len() && is_ident_start(bytes[k] as char) {
                let method_start = k;
                k += 1;
                while k < bytes.len() && is_ident_continue(bytes[k] as char) {
                    k += 1;
                }
                let method = &text[method_start..k];
                let mut l = k;
                while l < bytes.len() && (bytes[l] as char).is_whitespace() {
                    l += 1;
                }
                if l < bytes.len() && bytes[l] as char == '(' {
                    out.push(crate::lang::CallSite {
                        callee: method.to_string(),
                        qualifier: Some(token.to_string()),
                        line,
                    });
                }
            }
            continue;
        }

        if j < bytes.len() && ((bytes[j] as char == '(') || (bytes[j] as char == '!')) {
            if let Some((callee, qualifier)) = split_qualified_name(token) {
                if !is_rust_keyword(&callee) && preceding_keyword(text, start) != Some("fn") {
                    out.push(crate::lang::CallSite { callee, qualifier, line });
                }
            }
            continue;
        }

        if known_functions.contains(token) {
            let prev = text[..start].chars().rev().find(|c| !c.is_whitespace());
            let next = text[j..].chars().find(|c| !c.is_whitespace());
            let looks_like_arg_ref = matches!(prev, Some('(' | ',' | '['))
                && matches!(next, Some(',' | ')' | ']'));
            if looks_like_arg_ref {
                out.push(crate::lang::CallSite {
                    callee: token.to_string(),
                    qualifier: None,
                    line,
                });
            }
        }
    }
    out
}

fn scan_macro_invocations(text: &str, base_line: u32) -> Vec<crate::lang::CallSite> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for (idx, ch) in text.char_indices() {
        if ch != '!' {
            continue;
        }
        let mut next = idx + 1;
        while next < bytes.len() && (bytes[next] as char).is_whitespace() {
            next += 1;
        }
        if next >= bytes.len() || !matches!(bytes[next] as char, '(' | '[' | '{') {
            continue;
        }

        let mut end = idx;
        while end > 0 && (bytes[end - 1] as char).is_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && is_ident_continue(bytes[start - 1] as char) {
            start -= 1;
        }
        if start == end {
            continue;
        }
        let token = &text[start..end];
        if let Some((callee, qualifier)) = split_qualified_name(token) {
            if !is_rust_keyword(&callee) {
                let line = base_line + text[..start].chars().filter(|c| *c == '\n').count() as u32;
                out.push(crate::lang::CallSite { callee, qualifier, line });
            }
        }
    }
    out
}

fn split_qualified_name(token: &str) -> Option<(String, Option<String>)> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((qualifier, callee)) = trimmed.rsplit_once("::") {
        if callee.is_empty() {
            return None;
        }
        return Some((callee.to_string(), Some(qualifier.to_string())));
    }
    Some((trimmed.to_string(), None))
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch == ':' || ch.is_ascii_alphanumeric()
}

fn is_rust_keyword(token: &str) -> bool {
    matches!(
        token,
        "Self"
            | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

fn preceding_keyword<'a>(text: &'a str, start: usize) -> Option<&'a str> {
    let prefix = &text[..start];
    let mut end = prefix.len();
    while end > 0 && prefix.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut begin = end;
    while begin > 0 {
        let ch = prefix.as_bytes()[begin - 1] as char;
        if ch == '_' || ch.is_ascii_alphanumeric() {
            begin -= 1;
        } else {
            break;
        }
    }
    if begin == end {
        None
    } else {
        Some(&prefix[begin..end])
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

fn estimate_complexity(node: Node, _source: &str) -> u32 {
    super::estimate_complexity_for(node, &[
        "if_expression", "match_expression", "while_expression",
        "for_expression", "loop_expression",
    ])
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

fn merge_compiler_expanded_calls(
    root: &Path,
    _file: &Path,
    source: &str,
    functions: &mut [IndexedFunction],
) {
    if !source.contains('!') {
        return;
    }
    let Some(expanded) = cargo_expand_root(root) else {
        return;
    };
    let expanded_functions = analyze_expanded_functions(&expanded);
    if expanded_functions.is_empty() {
        return;
    }

    let mut local_counts: HashMap<String, usize> = HashMap::new();
    for f in functions.iter() {
        *local_counts.entry(f.name.clone()).or_default() += 1;
    }

    let mut expanded_by_name: HashMap<String, Vec<&IndexedFunction>> = HashMap::new();
    for f in &expanded_functions {
        expanded_by_name.entry(f.name.clone()).or_default().push(f);
    }

    for func in functions.iter_mut() {
        if local_counts.get(&func.name).copied().unwrap_or(0) != 1 {
            continue;
        }
        let Some(matches) = expanded_by_name.get(&func.name) else {
            continue;
        };
        if matches.len() != 1 {
            continue;
        }
        for call in &matches[0].calls {
            func.calls.push(call.clone());
        }
        normalize_call_sites(&mut func.calls);
    }
}

fn normalize_call_sites(calls: &mut Vec<crate::lang::CallSite>) {
    calls.sort_by(|a, b| {
        a.callee
            .cmp(&b.callee)
            .then(a.line.cmp(&b.line))
            .then_with(|| qualifier_sort_key(&a.qualifier).cmp(&qualifier_sort_key(&b.qualifier)))
    });
    calls.dedup_by(|a, b| a.callee == b.callee && a.line == b.line && a.qualifier == b.qualifier);

    let qualified_on_line: HashSet<(String, u32)> = calls
        .iter()
        .filter(|call| call.qualifier.is_some())
        .map(|call| (call.callee.clone(), call.line))
        .collect();

    calls.retain(|call| {
        call.qualifier.is_some()
            || !qualified_on_line.contains(&(call.callee.clone(), call.line))
    });
}

fn qualifier_sort_key(qualifier: &Option<String>) -> (&str, &str) {
    match qualifier.as_deref() {
        Some(q) => ("", q),
        None => ("~", ""),
    }
}

fn analyze_expanded_functions(source: &str) -> Vec<IndexedFunction> {
    let mut parser = Parser::new();
    parser
        .set_language(&SupportLang::Rust.get_ts_language())
        .expect("failed to load Rust grammar");
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let root_node = tree.root_node();
    let known_functions = collect_function_names(root_node, source);
    let mut functions = Vec::new();
    let mut endpoints = Vec::new();
    let mut types = Vec::new();
    scan_children(
        source,
        Path::new("."),
        Path::new("expanded.rs"),
        root_node,
        "expanded",
        None,
        None,
        &known_functions,
        &mut functions,
        &mut endpoints,
        &mut types,
    );
    functions
}

fn cargo_expand_root(root: &Path) -> Option<String> {
    let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let cache = EXPANDED_RUST_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache.lock().ok()?.get(&key).cloned() {
        return cached;
    }

    let expanded = cargo_expand_root_uncached(&key);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, expanded.clone());
    }
    expanded
}

fn cargo_expand_root_uncached(root: &Path) -> Option<String> {
    let manifest_path = root.join("Cargo.toml");
    if !manifest_path.exists() {
        return None;
    }
    let target = select_expand_target(&manifest_path)?;
    let mut cmd = Command::new("cargo");
    cmd.arg("+nightly")
        .arg("rustc")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest_path);
    match target.kind.as_str() {
        "bin" => {
            cmd.arg("--bin").arg(target.name);
        }
        "lib" => {
            cmd.arg("--lib");
        }
        _ => return None,
    }
    let output = cmd.arg("--").arg("-Zunpretty=expanded,identified").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn select_expand_target(manifest_path: &Path) -> Option<ExpandTarget> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(manifest_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout).ok()?;
    let pkg = metadata.packages.first()?;
    let bin = pkg.targets.iter().find(|t| t.kind.iter().any(|k| k == "bin"));
    if let Some(target) = bin {
        return Some(ExpandTarget { kind: "bin".to_string(), name: target.name.clone() });
    }
    let lib = pkg.targets.iter().find(|t| t.kind.iter().any(|k| k == "lib"));
    lib.map(|target| ExpandTarget { kind: "lib".to_string(), name: target.name.clone() })
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    targets: Vec<CargoTarget>,
}

#[derive(Deserialize)]
struct CargoTarget {
    kind: Vec<String>,
    name: String,
}

struct ExpandTarget {
    kind: String,
    name: String,
}
