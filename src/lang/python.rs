use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};
use tree_sitter_python::LANGUAGE;

use super::{
    line_range, relative, walk_supersearch, AstMatch, IndexedEndpoint, IndexedFunction,
    IndexedType, LanguageAnalyzer, NodeKinds,
};

pub struct PythonAnalyzer;

const KINDS: NodeKinds = NodeKinds {
    identifiers: &["identifier"],
    strings: &["string"],
    comments: &["comment"],
    calls: &["call"],
    assigns: &["assignment", "named_expression"],
    returns: &["return_statement"],
};

impl LanguageAnalyzer for PythonAnalyzer {
    fn language(&self) -> &str {
        "python"
    }

    fn extensions(&self) -> &[&str] {
        &["py"]
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
            .expect("failed to load Python grammar");
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;

        let mut functions = Vec::new();
        let mut endpoints = Vec::new();
        let mut types = Vec::new();
        walk_py(&source, root, file, tree.root_node(), &mut functions, &mut endpoints, &mut types);
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

fn walk_py(
    source: &str,
    root: &Path,
    file: &Path,
    node: Node,
    functions: &mut Vec<IndexedFunction>,
    endpoints: &mut Vec<IndexedEndpoint>,
    types: &mut Vec<IndexedType>,
) {
    match node.kind() {
        "class_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                if !name.is_empty() {
                    let (start_line, end_line) = line_range(&node);
                    types.push(IndexedType {
                        name,
                        file: relative(root, file),
                        language: "python".to_string(),
                        start_line,
                        end_line,
                        kind: "class".to_string(),
                    });
                }
            }
        }
        "function_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                let (start_line, end_line) = line_range(&node);
                functions.push(IndexedFunction {
                    name,
                    file: relative(root, file),
                    language: "python".to_string(),
                    start_line,
                    end_line,
                    complexity: estimate_complexity(node, source),
                    calls: collect_calls(node, source),
                });
            }
        }
        "decorated_definition" => {
            // Collect HTTP-method decorators and pair with the definition.
            let mut method_path: Option<(String, String)> = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "decorator" {
                    if let Some(mp) = extract_http_decorator(source, child) {
                        method_path = Some(mp);
                    }
                }
            }
            // Walk into the definition for functions/classes inside.
            if let Some(def) = node.child_by_field_name("definition") {
                if def.kind() == "function_definition" {
                    if let Some(name_node) = def.child_by_field_name("name") {
                        let name = name_node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                        let (start_line, end_line) = line_range(&def);
                        functions.push(IndexedFunction {
                            name: name.clone(),
                            file: relative(root, file),
                            language: "python".to_string(),
                            start_line,
                            end_line,
                            complexity: estimate_complexity(def, source),
                            calls: collect_calls(def, source),
                        });
                        if let Some((method, path)) = method_path {
                            endpoints.push(IndexedEndpoint {
                                method,
                                path,
                                file: relative(root, file),
                                handler_name: Some(name),
                                language: "python".to_string(),
                                framework: "flask/fastapi".to_string(),
                            });
                        }
                    }
                } else {
                    // Could be a class — recurse into it.
                    walk_py(source, root, file, def, functions, endpoints, types);
                }
                return; // children already handled
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_py(source, root, file, child, functions, endpoints, types);
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
    if node.kind() == "call" {
        if let Some(func) = node.child_by_field_name("function") {
            let name = match func.kind() {
                "identifier" => func.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
                "attribute" => func
                    .child_by_field_name("attribute")
                    .and_then(|a| a.utf8_text(source.as_bytes()).ok())
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

/// Try to extract (METHOD, "/path") from a decorator node.
/// Handles: @app.get("/path"), @router.post("/path"), @app.route("/path", methods=["GET"])
fn extract_http_decorator(source: &str, decorator: Node) -> Option<(String, String)> {
    // The decorator's first named child is the actual expression (call or attribute).
    let expr = decorator.named_child(0)?;

    match expr.kind() {
        "call" => {
            let func = expr.child_by_field_name("function")?;
            let method = http_method_from_expr(source, func)?;
            let args = expr.child_by_field_name("arguments")?;
            let path = first_string_arg(source, args)?;
            Some((method, path))
        }
        "attribute" => {
            // bare @app.get (no args) — skip path extraction
            let _ = http_method_from_expr(source, expr)?;
            None
        }
        _ => None,
    }
}

/// Extract an HTTP method name from an `attribute` node like `app.get` or `router.post`.
/// Also handles `app.route` → returns "ROUTE" (we skip those without explicit methods).
fn http_method_from_expr(source: &str, node: Node) -> Option<String> {
    let prop = match node.kind() {
        "attribute" => node.child_by_field_name("attribute")?,
        "identifier" => node,
        _ => return None,
    };
    let name = prop.utf8_text(source.as_bytes()).ok()?;
    match name.to_lowercase().as_str() {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => {
            Some(name.to_uppercase())
        }
        "route" => Some("ROUTE".to_string()),
        _ => None,
    }
}

/// Return the first positional string argument from an `argument_list` / `arguments` node.
fn first_string_arg(source: &str, args: Node) -> Option<String> {
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        if child.kind() == "string" {
            let raw = child.utf8_text(source.as_bytes()).ok()?;
            // Strip quotes — Python strings: "...", '...', """...""", f"..."
            let stripped = raw
                .trim_start_matches(['f', 'b', 'r'])
                .trim_matches(|c| c == '"' || c == '\'');
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
            | "elif_clause"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "conditional_expression" => count += 1,
            _ => {}
        }
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}
