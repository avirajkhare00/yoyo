pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFunction {
    pub name: String,
    pub file: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub complexity: u32,
    #[serde(default)]
    pub calls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedEndpoint {
    pub method: String,
    pub path: String,
    pub file: String,
    pub handler_name: Option<String>,
    pub language: String,
    pub framework: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedType {
    pub name: String,
    pub file: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub kind: String, // "struct" | "enum" | "trait" | "type" | "class" | "interface"
}

#[derive(Debug)]
pub struct AstMatch {
    pub line: u32,
    pub snippet: String,
}

pub trait LanguageAnalyzer: Send + Sync {
    fn language(&self) -> &str;
    #[allow(dead_code)]
    fn extensions(&self) -> &[&str];
    fn analyze_file(
        &self,
        root: &Path,
        file: &Path,
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>)>;
    fn supports_ast_search(&self) -> bool {
        false
    }
    fn ast_search(
        &self,
        _source: &str,
        _query_lc: &str,
        _context: &str,
        _pattern: &str,
    ) -> Vec<AstMatch> {
        vec![]
    }
}

/// Registry — one place to add new languages.
pub fn find_analyzer(lang: &str) -> Option<Box<dyn LanguageAnalyzer>> {
    let all: Vec<Box<dyn LanguageAnalyzer>> = vec![
        Box::new(go::GoAnalyzer),
        Box::new(python::PythonAnalyzer),
        Box::new(rust::RustAnalyzer),
        Box::new(typescript::TypeScriptAnalyzer),
    ];
    all.into_iter().find(|a| a.language() == lang)
}

// ── Shared helpers used by all language analyzers ──────────────────────────

pub fn line_range(node: &Node) -> (u32, u32) {
    let start = (node.start_position().row + 1) as u32;
    let end = (node.end_position().row + 1) as u32;
    (start, end)
}

pub fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

/// Node-kind descriptors that parameterize the generic supersearch walker.
pub struct NodeKinds {
    pub identifiers: &'static [&'static str],
    pub strings: &'static [&'static str],
    pub comments: &'static [&'static str],
    pub calls: &'static [&'static str],
    pub assigns: &'static [&'static str],
    pub returns: &'static [&'static str],
}

/// Language-agnostic AST supersearch walker.
/// Each language provides its `NodeKinds`; the traversal logic is shared.
pub fn walk_supersearch(
    node: Node,
    source: &str,
    lines: &[&str],
    query_lc: &str,
    context: &str,
    pattern: &str,
    in_call: bool,
    in_assign: bool,
    in_return: bool,
    kinds: &NodeKinds,
    matches: &mut Vec<AstMatch>,
) {
    let kind = node.kind();

    let in_call = in_call || kinds.calls.contains(&kind);
    let in_assign = in_assign || kinds.assigns.contains(&kind);
    let in_return = in_return || kinds.returns.contains(&kind);

    let is_identifier = kinds.identifiers.contains(&kind);
    let is_string = kinds.strings.contains(&kind);
    let is_comment = kinds.comments.contains(&kind);

    if is_identifier || is_string || is_comment {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            if text.to_lowercase().contains(query_lc) {
                let context_ok = match context {
                    "all" => true,
                    "strings" => is_string,
                    "comments" => is_comment,
                    "identifiers" => is_identifier,
                    _ => true,
                };
                let pattern_ok = match pattern {
                    "all" => true,
                    "call" => in_call,
                    "assign" => in_assign,
                    "return" => in_return,
                    _ => true,
                };
                if context_ok && pattern_ok {
                    let row = node.start_position().row as usize;
                    let snippet = lines
                        .get(row)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| text.trim().to_string());
                    matches.push(AstMatch { line: (row + 1) as u32, snippet });
                }
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_supersearch(
            child, source, lines, query_lc, context, pattern,
            in_call, in_assign, in_return, kinds, matches,
        );
    }
}