use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};
use tree_sitter_typescript::LANGUAGE_TYPESCRIPT;

/// Summary of a TypeScript function.
#[derive(Debug, Serialize, Deserialize)]
pub struct TsFunction {
    pub name: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    /// Very rough complexity estimate: count of branching/loop constructs.
    pub complexity: u32,
}

/// Summary of an Express-style endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExpressEndpoint {
    pub method: String,
    pub path: String,
    pub file: String,
    pub handler_name: Option<String>,
}

/// Analyze a TypeScript file for functions and Express endpoints.
pub fn analyze_typescript_file(
    root: &Path,
    file: &Path,
) -> Result<(Vec<TsFunction>, Vec<ExpressEndpoint>)> {
    let source = fs::read_to_string(file)?;

    let mut parser = Parser::new();
    parser
        .set_language(&LANGUAGE_TYPESCRIPT.into())
        .expect("failed to load TypeScript grammar");

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("failed to parse TypeScript file {}", file.display()))?;

    let mut functions = Vec::new();
    let mut endpoints = Vec::new();

    let root_node = tree.root_node();
    walk_ts(
        &source,
        root,
        file,
        root_node,
        &mut functions,
        &mut endpoints,
    );

    Ok((functions, endpoints))
}

fn walk_ts(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    functions: &mut Vec<TsFunction>,
    endpoints: &mut Vec<ExpressEndpoint>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                let (start_line, end_line) = line_range(&node);
                let complexity = estimate_complexity(node, source);
                functions.push(TsFunction {
                    name,
                    file: relative(root, file),
                    start_line,
                    end_line,
                    complexity,
                });
            }
        }
        "call_expression" => {
            detect_express_call(source, root, file, node, endpoints);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ts(source, root, file, child, functions, endpoints);
    }
}

fn line_range(node: &Node) -> (u32, u32) {
    let start = (node.start_position().row + 1) as u32;
    let end = (node.end_position().row + 1) as u32;
    (start, end)
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

fn estimate_complexity(node: Node, source: &str) -> u32 {
    let mut count = 1; // base cost
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "conditional_expression" => {
                count += 1;
            }
            _ => {}
        }
        // Recurse
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}

fn detect_express_call(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    endpoints: &mut Vec<ExpressEndpoint>,
) {
    // Rough heuristic:
    // Look for app.get('/path', handler) or router.post("/path", handler)
    // AST: call_expression with callee = member_expression (object . property)
    if let Some(callee) = node.child_by_field_name("function") {
        if callee.kind() == "member_expression" {
            let object = callee.child_by_field_name("object");
            let property = callee.child_by_field_name("property");
            if let (Some(_object), Some(prop)) = (object, property) {
                let method = prop.utf8_text(source.as_bytes()).unwrap_or("").to_uppercase();
                if !matches!(
                    method.as_str(),
                    "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS"
                ) {
                    return;
                }

                // First argument: path string literal
                if let Some(args) = node.child_by_field_name("arguments") {
                    if let Some(first_arg) = args.named_child(0) {
                        if first_arg.kind() == "string" {
                            let raw_path =
                                first_arg.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                            let path_clean = raw_path.trim_matches(&['"', '\''][..]).to_string();

                            // Second argument: handler identifier (optional)
                            let handler_name = args
                                .named_child(1)
                                .and_then(|h| h.utf8_text(source.as_bytes()).ok())
                                .map(|s| s.to_string());

                            endpoints.push(ExpressEndpoint {
                                method,
                                path: path_clean,
                                file: relative(root, file),
                                handler_name,
                            });
                        }
                    }
                }
            }
        }
    }
}

