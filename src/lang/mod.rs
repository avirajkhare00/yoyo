pub mod bash;
pub mod c;
pub mod cpp;
pub mod csharp;
pub mod go;
pub mod java;
pub mod kotlin;
pub mod php;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod swift;
pub mod typescript;

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tree_sitter::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    pub callee: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Visibility {
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "module")]
    Module, // pub(crate), pub(super), Go package-level
    #[serde(rename = "private")]
    Private,
}

impl Default for Visibility {
    fn default() -> Self { Visibility::Private }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFunction {
    pub name: String,
    pub file: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub complexity: u32,
    #[serde(default)]
    pub calls: Vec<CallSite>,
    #[serde(default)]
    pub byte_start: usize,
    #[serde(default)]
    pub byte_end: usize,
    /// Dot/colon-separated module path derived from file path or package declaration.
    /// e.g. "crates::core::flags", "flask.sansio", "src/router"
    #[serde(default)]
    pub module_path: String,
    /// Fully qualified name: module_path + separator + name.
    #[serde(default)]
    pub qualified_name: String,
    #[serde(default)]
    pub visibility: Visibility,
    /// For methods: the struct/enum/trait this is defined on (e.g. "SearchWorker").
    /// None for free functions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_type: Option<String>,
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
    #[serde(default)]
    pub module_path: String,
    #[serde(default)]
    pub visibility: Visibility,
    /// Parsed fields for structs (Rust only for now).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    /// Raw type string as written in source (e.g. "Option<String>", "*mut u8").
    pub type_str: String,
    pub visibility: Visibility,
}

/// One `impl Trait for Type` (or `impl Type`) relationship, indexed per file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedImpl {
    /// The concrete type being implemented on (e.g. "SearchWorker").
    pub type_name: String,
    /// The trait being implemented, if any (e.g. "Matcher"). None for bare `impl Type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trait_name: Option<String>,
    pub file: String,
    pub start_line: u32,
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
    ) -> Result<(Vec<IndexedFunction>, Vec<IndexedEndpoint>, Vec<IndexedType>, Vec<IndexedImpl>)>;
    /// Extract import/use/require paths from source. Line-based — no AST needed.
    fn extract_imports(&self, _source: &str) -> Vec<String> {
        vec![]
    }
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

/// Derive a module path from a relative file path and language.
///
/// Rust:  `crates/core/flags/parse.rs`  → `crates::core::flags`
/// Python: `src/flask/sansio/app.py`    → `flask.sansio`  (strips leading `src/`)
/// Go:    `cmd/server/main.go`          → `cmd/server`
/// TS/JS: `src/router/index.ts`         → `src/router`
pub fn module_path_from_file(file: &str, lang: &str) -> String {
    let path = std::path::Path::new(file);
    let dir = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();

    // Strip common source roots for Python
    let dir = if lang == "python" {
        let stripped = dir
            .strip_prefix("src/").unwrap_or(&dir)
            .strip_prefix("lib/").unwrap_or(&dir);
        stripped.to_string()
    } else {
        dir
    };

    if dir.is_empty() {
        return String::new();
    }

    let sep = match lang {
        "rust" => "::",
        "python" => ".",
        _ => "/",
    };

    // Strip `src/` for Rust — it's a filesystem convention, not a module.
    // `crate_name/src/foo/bar` → `crate_name/foo/bar` (keep crate name, drop `src`).
    // `src/foo/bar` (no crate prefix) → `foo/bar`.
    let dir = if lang == "rust" {
        if let Some(idx) = dir.find("/src/") {
            let before = &dir[..idx];
            let after = &dir[idx + 5..];
            // Take only the last segment of `before` as the crate name.
            let crate_name = before.split('/').next_back().unwrap_or(before);
            if after.is_empty() {
                crate_name.to_string()
            } else {
                format!("{}/{}", crate_name, after)
            }
        } else {
            dir.strip_prefix("src/").unwrap_or(&dir).to_string()
        }
    } else {
        dir
    };

    dir.replace('/', sep)
}

/// Build a qualified name from module path, name, and language.
pub fn qualified_name(module_path: &str, name: &str, lang: &str) -> String {
    if module_path.is_empty() {
        return name.to_string();
    }
    let sep = match lang {
        "rust" => "::",
        "python" => ".",
        _ => "/",
    };
    format!("{}{}{}", module_path, sep, name)
}

/// Registry — one place to add new languages.
pub fn find_analyzer(lang: &str) -> Option<Box<dyn LanguageAnalyzer>> {
    let all: Vec<Box<dyn LanguageAnalyzer>> = vec![
        Box::new(bash::BashAnalyzer),
        Box::new(c::CAnalyzer),
        Box::new(cpp::CppAnalyzer),
        Box::new(csharp::CSharpAnalyzer),
        Box::new(go::GoAnalyzer),
        Box::new(java::JavaAnalyzer),
        Box::new(kotlin::KotlinAnalyzer),
        Box::new(php::PhpAnalyzer),
        Box::new(python::PythonAnalyzer),
        Box::new(ruby::RubyAnalyzer),
        Box::new(rust::RustAnalyzer),
        Box::new(swift::SwiftAnalyzer),
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

pub fn byte_range(node: &Node) -> (usize, usize) {
    (node.start_byte(), node.end_byte())
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