use std::fs;
use std::path::Path;

use anyhow::Result;
use tree_sitter::{Node, Parser};

use super::{
    byte_range, line_range, module_path_from_file, qualified_name, relative, AstMatch, CallSite,
    IndexedEndpoint, IndexedFunction, IndexedImpl, IndexedType, LanguageAnalyzer, Visibility,
};
use crate::engine::types::SyntaxError;

pub struct ClojureAnalyzer;

const SPECIAL_FORMS: &[&str] = &[
    "binding", "case", "catch", "cond", "cond->", "cond->>", "def", "defmacro", "defn", "defn-",
    "defonce", "delay", "do", "doseq", "finally", "fn", "for", "future", "if", "if-let", "if-not",
    "lazy-seq", "let", "loop", "ns", "quote", "recur", "set!", "some->", "some->>", "throw", "->",
    "->>", "try", "var", "when", "when-let", "when-not",
];

const COMPLEXITY_FORMS: &[&str] = &[
    "case", "catch", "cond", "cond->", "cond->>", "doseq", "for", "if", "if-let", "if-not", "loop",
    "try", "when", "when-let", "when-not",
];

const ASSIGN_FORMS: &[&str] = &["binding", "def", "defonce", "let", "loop", "set!"];
const RETURN_FORMS: &[&str] = &["recur", "throw"];

impl LanguageAnalyzer for ClojureAnalyzer {
    fn language(&self) -> &str {
        "clojure"
    }

    fn extract_imports(&self, source: &str) -> Vec<String> {
        extract_ns_imports(source)
    }

    fn analyze_file(
        &self,
        root: &Path,
        file: &Path,
    ) -> Result<(
        Vec<IndexedFunction>,
        Vec<IndexedEndpoint>,
        Vec<IndexedType>,
        Vec<IndexedImpl>,
    )> {
        let source = fs::read_to_string(file)?;
        let tree = parse_clojure(&source)
            .ok_or_else(|| anyhow::anyhow!("failed to parse {}", file.display()))?;
        let rel_file = relative(root, file);
        let mod_path = module_path_from_file(&relative(root, file), "clojure");
        let mut functions = Vec::new();
        for form in top_level_list_forms(tree.root_node()) {
            let Some(head) = list_head_text(form, &source) else {
                continue;
            };
            if !matches!(head.as_str(), "defn" | "defn-" | "defmacro") {
                continue;
            }
            let Some(name) = definition_name(form, &source) else {
                continue;
            };
            let (start_line, end_line) = line_range(&form);
            let (byte_start, byte_end) = byte_range(&form);
            functions.push(IndexedFunction {
                name: name.clone(),
                file: rel_file.clone(),
                language: "clojure".to_string(),
                start_line,
                end_line,
                complexity: estimate_complexity(form, &source),
                calls: collect_calls(form, &source),
                byte_start,
                byte_end,
                module_path: mod_path.clone(),
                qualified_name: qualified_name(&mod_path, &name, "clojure"),
                visibility: if head == "defn-" {
                    Visibility::Private
                } else {
                    Visibility::Public
                },
                parent_type: None,
                ..Default::default()
            });
        }
        Ok((functions, vec![], vec![], vec![]))
    }

    fn ts_language(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_clojure::LANGUAGE.into())
    }

    fn ast_search(
        &self,
        source: &str,
        query_lc: &str,
        context: &str,
        pattern: &str,
    ) -> Vec<AstMatch> {
        clojure_ast_search(source, query_lc, context, pattern)
    }
}

pub(crate) fn syntax_errors(source: &str) -> Vec<SyntaxError> {
    let Some(tree) = parse_clojure(source) else {
        return vec![SyntaxError {
            line: 1,
            kind: "clojure".to_string(),
            text: "failed to parse source".to_string(),
        }];
    };
    if !tree.root_node().has_error() {
        return vec![];
    }
    let mut errors = Vec::new();
    collect_syntax_errors(tree.root_node(), source, &mut errors);
    if errors.is_empty() {
        errors.push(SyntaxError {
            line: 1,
            kind: "clojure".to_string(),
            text: "syntax error".to_string(),
        });
    }
    errors.sort_by(|a, b| a.line.cmp(&b.line).then(a.text.cmp(&b.text)));
    errors.dedup_by(|a, b| a.line == b.line && a.text == b.text);
    errors
}

fn parse_clojure(source: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_clojure::LANGUAGE.into())
        .expect("failed to load Clojure grammar");
    parser.parse(source, None)
}

fn top_level_list_forms(root: Node) -> Vec<Node> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|child| child.kind() == "list_lit")
        .collect()
}

fn semantic_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "comment" | "dis_expr"))
        .collect()
}

fn node_text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_string()
}

fn list_head_node(node: Node) -> Option<Node> {
    semantic_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "sym_lit" | "kwd_lit"))
}

fn list_head_text(node: Node, source: &str) -> Option<String> {
    list_head_node(node).map(|child| node_text(child, source))
}

fn extract_ns_imports(source: &str) -> Vec<String> {
    let Some(tree) = parse_clojure(source) else {
        return vec![];
    };
    let ns_form = top_level_list_forms(tree.root_node())
        .into_iter()
        .find(|form| list_head_text(*form, source).as_deref() == Some("ns"));
    let Some(form) = ns_form else {
        return vec![];
    };
    let mut imports = Vec::new();
    for clause in semantic_children(form).into_iter().skip(2) {
        if clause.kind() != "list_lit" {
            continue;
        }
        let Some(head) = list_head_text(clause, source) else {
            continue;
        };
        if !matches!(
            head.as_str(),
            ":require" | ":require-macros" | ":use" | ":import"
        ) {
            continue;
        }
        collect_clause_imports(clause, source, &mut imports);
    }
    imports.sort();
    imports.dedup();
    imports
}

fn namespace_like(atom: &str) -> bool {
    if atom.starts_with(':') || atom.starts_with('^') {
        return false;
    }
    if atom == "&" || atom == "nil" || atom == "true" || atom == "false" {
        return false;
    }
    atom.chars()
        .any(|ch| matches!(ch, '.' | '/' | '-' | '_') || ch.is_ascii_alphabetic())
}

fn collect_clause_imports(node: Node, source: &str, imports: &mut Vec<String>) {
    for child in semantic_children(node).into_iter().skip(1) {
        collect_import_targets(child, source, imports);
    }
}

fn collect_import_targets(node: Node, source: &str, imports: &mut Vec<String>) {
    match node.kind() {
        "sym_lit" => {
            let text = node_text(node, source);
            if namespace_like(&text) {
                imports.push(text);
            }
        }
        "vec_lit" | "list_lit" => {
            if let Some(ns) = first_namespace_symbol(node, source) {
                imports.push(ns);
            }
            for child in semantic_children(node) {
                if matches!(child.kind(), "vec_lit" | "list_lit") {
                    collect_import_targets(child, source, imports);
                }
            }
        }
        _ => {
            for child in semantic_children(node) {
                collect_import_targets(child, source, imports);
            }
        }
    }
}

fn first_namespace_symbol(node: Node, source: &str) -> Option<String> {
    for child in semantic_children(node) {
        if child.kind() != "sym_lit" {
            continue;
        }
        let text = node_text(child, source);
        if namespace_like(&text) {
            return Some(text);
        }
    }
    None
}

fn definition_name(node: Node, source: &str) -> Option<String> {
    semantic_children(node)
        .into_iter()
        .skip(1)
        .find(|child| child.kind() == "sym_lit")
        .map(|child| node_text(child, source))
}

fn estimate_complexity(node: Node, source: &str) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "list_lit" {
            if let Some(head) = list_head_text(child, source) {
                if COMPLEXITY_FORMS.contains(&head.as_str()) {
                    count += 1;
                }
            }
        }
        count += estimate_complexity(child, source).saturating_sub(1);
    }
    count
}

fn collect_calls(node: Node, source: &str) -> Vec<CallSite> {
    let mut calls = Vec::new();
    collect_calls_inner(node, source, &mut calls);
    calls.sort_by(|a, b| a.callee.cmp(&b.callee).then(a.line.cmp(&b.line)));
    calls.dedup_by(|a, b| a.callee == b.callee && a.qualifier == b.qualifier && a.line == b.line);
    calls
}

fn collect_calls_inner(node: Node, source: &str, calls: &mut Vec<CallSite>) {
    if node.kind() == "list_lit" {
        if let Some(head_node) = list_head_node(node) {
            let head = node_text(head_node, source);
            if !SPECIAL_FORMS.contains(&head.as_str()) {
                let (callee, qualifier) = split_qualified_symbol(&head);
                if !callee.is_empty() {
                    calls.push(CallSite {
                        callee,
                        qualifier,
                        line: (head_node.start_position().row + 1) as u32,
                    });
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_inner(child, source, calls);
    }
}

fn collect_syntax_errors(node: Node, source: &str, errors: &mut Vec<SyntaxError>) {
    if node.is_missing() {
        errors.push(SyntaxError {
            line: (node.start_position().row + 1) as u32,
            kind: "clojure".to_string(),
            text: format!("missing {}", node.kind()),
        });
        return;
    }
    if node.is_error() {
        let snippet = node_text(node, source);
        let text = if snippet.trim().is_empty() {
            "syntax error".to_string()
        } else {
            let snippet = snippet.trim().replace('\n', " ");
            let truncated: String = snippet.chars().take(40).collect();
            format!("syntax error near {}", truncated)
        };
        errors.push(SyntaxError {
            line: (node.start_position().row + 1) as u32,
            kind: "clojure".to_string(),
            text,
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_syntax_errors(child, source, errors);
    }
}

#[derive(Clone, Copy, Default)]
struct SearchState {
    in_call: bool,
    in_assign: bool,
    in_return: bool,
}

fn clojure_ast_search(source: &str, query_lc: &str, context: &str, pattern: &str) -> Vec<AstMatch> {
    let Some(tree) = parse_clojure(source) else {
        return vec![];
    };
    let lines: Vec<&str> = source.lines().collect();
    let mut matches = Vec::new();
    walk_search(
        tree.root_node(),
        source,
        &lines,
        query_lc,
        context,
        pattern,
        SearchState::default(),
        &mut matches,
    );
    matches.sort_by_key(|m| m.line);
    matches.dedup_by(|a, b| a.line == b.line);
    matches
}

fn walk_search(
    node: Node,
    source: &str,
    lines: &[&str],
    query_lc: &str,
    context: &str,
    pattern: &str,
    state: SearchState,
    matches: &mut Vec<AstMatch>,
) {
    let mut state = state;
    if node.kind() == "list_lit" {
        if let Some(head) = list_head_text(node, source) {
            if !SPECIAL_FORMS.contains(&head.as_str()) {
                state.in_call = true;
            }
            if ASSIGN_FORMS.contains(&head.as_str()) {
                state.in_assign = true;
            }
            if RETURN_FORMS.contains(&head.as_str()) {
                state.in_return = true;
            }
        }
    }

    let kind = node.kind();
    let is_identifier = matches!(kind, "sym_lit" | "sym_val_lit");
    let is_string = matches!(kind, "str_lit" | "char_lit" | "regex_lit");
    let is_comment = kind == "comment";

    if is_identifier || is_string || is_comment {
        let text = node_text(node, source);
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
                "call" => state.in_call,
                "assign" => state.in_assign,
                "return" => state.in_return,
                _ => true,
            };
            if context_ok && pattern_ok {
                let row = node.start_position().row as usize;
                let snippet = lines
                    .get(row)
                    .map(|line| line.trim().to_string())
                    .unwrap_or_else(|| text.trim().to_string());
                matches.push(AstMatch {
                    line: (row + 1) as u32,
                    snippet,
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_search(
            child, source, lines, query_lc, context, pattern, state, matches,
        );
    }
}

fn split_qualified_symbol(symbol: &str) -> (String, Option<String>) {
    if let Some((qualifier, callee)) = symbol.rsplit_once('/') {
        return (callee.to_string(), Some(qualifier.to_string()));
    }
    (symbol.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, rel: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn clojure_analyzer_indexes_functions_and_imports() {
        let dir = TempDir::new().unwrap();
        let file = write_file(
            &dir,
            "src/my/app/core.clj",
            "(ns my.app.core\n  (:require [clojure.string :as str]\n            [my.app.util :as util])\n  (:import [java.time Instant]))\n\n(defn greet [xs]\n  (str/join \",\" xs))\n\n(defn- helper [x]\n  (inc x))\n\n(defmacro unless [pred & body]\n  `(if (not ~pred)\n     (do ~@body)))\n",
        );
        let analyzer = ClojureAnalyzer;
        let source = std::fs::read_to_string(&file).unwrap();
        let imports = analyzer.extract_imports(&source);
        let (functions, _, _, _) = analyzer.analyze_file(dir.path(), &file).unwrap();

        assert_eq!(
            imports,
            vec![
                "clojure.string".to_string(),
                "java.time".to_string(),
                "my.app.util".to_string()
            ]
        );
        assert_eq!(functions.len(), 3);
        assert!(functions
            .iter()
            .any(|f| f.name == "greet" && f.qualified_name == "my.app/greet"));
        assert!(functions
            .iter()
            .any(|f| f.name == "helper" && f.visibility == Visibility::Private));
        assert!(functions.iter().any(|f| f.name == "unless"));
    }

    #[test]
    fn clojure_ast_search_finds_calls_and_identifiers() {
        let analyzer = ClojureAnalyzer;
        let source = "(ns my.app.core)\n(defn greet [xs]\n  (str/join \",\" xs))\n(defn helper [value]\n  (inc value))\n";

        let call_matches = analyzer.ast_search(source, "join", "identifiers", "call");
        assert_eq!(call_matches.len(), 1);
        assert_eq!(call_matches[0].line, 3);
        assert!(call_matches[0].snippet.contains("str/join"));

        let ident_matches = analyzer.ast_search(source, "helper", "identifiers", "all");
        assert_eq!(ident_matches.len(), 1);
        assert_eq!(ident_matches[0].line, 4);
        assert!(ident_matches[0].snippet.contains("defn helper"));
    }

    #[test]
    fn clojure_syntax_errors_catches_unbalanced_forms() {
        let errors = syntax_errors("(defn greet []\n  (println \"hi\")\n");
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|err| err.kind == "clojure"));
    }
}
