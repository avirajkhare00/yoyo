use std::fs;
use std::path::Path;

use anyhow::Result;

use super::{
    module_path_from_file, qualified_name, relative, AstMatch, CallSite, IndexedEndpoint,
    IndexedFunction, IndexedImpl, IndexedType, LanguageAnalyzer, Visibility,
};
use crate::engine::types::SyntaxError;

pub struct ClojureAnalyzer;

#[derive(Clone, Copy, Debug)]
struct FormSpan {
    start_byte: usize,
    end_byte: usize,
    start_line: u32,
    end_line: u32,
}

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
        let mod_path = module_path_from_file(&relative(root, file), "clojure");
        let mut functions = Vec::new();
        let (forms, _) = scan_top_level_list_forms(&source);
        for form in forms {
            let Some(head) = head_symbol(
                &source,
                form.start_byte + 1,
                form.end_byte.saturating_sub(1),
            ) else {
                continue;
            };
            if !matches!(head.as_str(), "defn" | "defn-" | "defmacro") {
                continue;
            }
            let Some(name) = definition_name(
                &source,
                form.start_byte + 1,
                form.end_byte.saturating_sub(1),
            ) else {
                continue;
            };
            functions.push(IndexedFunction {
                name: name.clone(),
                file: relative(root, file),
                language: "clojure".to_string(),
                start_line: form.start_line,
                end_line: form.end_line,
                complexity: estimate_complexity(&source, form.start_byte, form.end_byte),
                calls: collect_calls(&source, form.start_byte, form.end_byte),
                byte_start: form.start_byte,
                byte_end: form.end_byte,
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

    fn supports_ast_search(&self) -> bool {
        true
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
    let (_, errors) = scan_top_level_list_forms(source);
    errors
}

fn scan_top_level_list_forms(source: &str) -> (Vec<FormSpan>, Vec<SyntaxError>) {
    let mut forms = Vec::new();
    let mut errors = Vec::new();
    let mut stack: Vec<(char, usize, u32)> = Vec::new();
    let mut chars = source.char_indices().peekable();
    let mut line = 1u32;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;

    while let Some((idx, ch)) = chars.next() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                line += 1;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            } else if ch == '\n' {
                line += 1;
            }
            continue;
        }

        match ch {
            '\n' => line += 1,
            ';' => in_comment = true,
            '"' => in_string = true,
            '\\' => {
                if let Some((_, next)) = chars.next() {
                    if next == '\n' {
                        line += 1;
                    }
                }
            }
            '(' | '[' | '{' => stack.push((ch, idx, line)),
            ')' | ']' | '}' => {
                let Some(&(open, start_byte, start_line)) = stack.last() else {
                    errors.push(SyntaxError {
                        line,
                        kind: "clojure".to_string(),
                        text: format!("unmatched closing delimiter '{}'", ch),
                    });
                    continue;
                };
                if !delimiters_match(open, ch) {
                    errors.push(SyntaxError {
                        line,
                        kind: "clojure".to_string(),
                        text: format!("mismatched delimiter '{}'", ch),
                    });
                    continue;
                }
                stack.pop();
                if stack.is_empty() && open == '(' {
                    forms.push(FormSpan {
                        start_byte,
                        end_byte: idx + ch.len_utf8(),
                        start_line,
                        end_line: line,
                    });
                }
            }
            _ => {}
        }
    }

    if in_string {
        errors.push(SyntaxError {
            line,
            kind: "clojure".to_string(),
            text: "unterminated string literal".to_string(),
        });
    }
    for (open, _, open_line) in stack {
        errors.push(SyntaxError {
            line: open_line,
            kind: "clojure".to_string(),
            text: format!("unclosed delimiter '{}'", open),
        });
    }
    (forms, errors)
}

fn delimiters_match(open: char, close: char) -> bool {
    matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}'))
}

fn extract_ns_imports(source: &str) -> Vec<String> {
    let (forms, _) = scan_top_level_list_forms(source);
    let ns_form = forms.into_iter().find(|form| {
        head_symbol(source, form.start_byte + 1, form.end_byte.saturating_sub(1)).as_deref()
            == Some("ns")
    });
    let Some(form) = ns_form else {
        return vec![];
    };
    let mut imports = Vec::new();
    let mut cursor = form.start_byte + 1;
    let end = form.end_byte.saturating_sub(1);
    let mut clause_active = false;
    let mut collection_stack: Vec<bool> = Vec::new();

    while let Some((token, next)) = next_token(source, cursor, end) {
        match token {
            Token::Atom(atom) => {
                if matches!(
                    atom.as_str(),
                    ":require" | ":require-macros" | ":use" | ":import"
                ) {
                    clause_active = true;
                } else if clause_active {
                    if let Some(needs_first) = collection_stack.last_mut() {
                        if *needs_first && namespace_like(&atom) {
                            imports.push(atom);
                            *needs_first = false;
                        }
                    } else if namespace_like(&atom) {
                        imports.push(atom);
                    }
                }
            }
            Token::Open(ch) => {
                if matches!(ch, '[' | '(') && clause_active {
                    collection_stack.push(true);
                } else {
                    collection_stack.push(false);
                }
            }
            Token::Close => {
                collection_stack.pop();
            }
            Token::String => {}
        }
        cursor = next;
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

fn definition_name(source: &str, start: usize, end: usize) -> Option<String> {
    let (_, after_head) = next_plain_symbol(source, start, end)?;
    let (name, _) = next_plain_symbol(source, after_head, end)?;
    Some(name)
}

fn estimate_complexity(source: &str, start: usize, end: usize) -> u32 {
    let mut count = 1u32;
    for_each_list_head(source, start, end, |head, _line| {
        if COMPLEXITY_FORMS.contains(&head.as_str()) {
            count += 1;
        }
    });
    count
}

fn collect_calls(source: &str, start: usize, end: usize) -> Vec<CallSite> {
    let mut calls = Vec::new();
    for_each_list_head(source, start, end, |head, line| {
        if SPECIAL_FORMS.contains(&head.as_str()) {
            return;
        }
        let (callee, qualifier) = split_qualified_symbol(&head);
        if !callee.is_empty() {
            calls.push(CallSite {
                callee,
                qualifier,
                line,
            });
        }
    });
    calls.sort_by(|a, b| a.callee.cmp(&b.callee).then(a.line.cmp(&b.line)));
    calls.dedup_by(|a, b| a.callee == b.callee && a.qualifier == b.qualifier && a.line == b.line);
    calls
}

fn split_qualified_symbol(symbol: &str) -> (String, Option<String>) {
    if let Some((qualifier, callee)) = symbol.rsplit_once('/') {
        return (callee.to_string(), Some(qualifier.to_string()));
    }
    (symbol.to_string(), None)
}

fn clojure_ast_search(source: &str, query_lc: &str, context: &str, pattern: &str) -> Vec<AstMatch> {
    let mut matches = Vec::new();
    let want_identifiers = context == "all" || context == "identifiers";
    let want_strings = context == "all" || context == "strings";
    let want_comments = context == "all" || context == "comments";

    if want_comments && pattern == "all" {
        for (line, text) in comment_tokens(source) {
            if text.to_lowercase().contains(query_lc) {
                matches.push(AstMatch {
                    line,
                    snippet: line_snippet(source, line),
                });
            }
        }
    }
    if want_strings && pattern == "all" {
        for (line, text) in string_tokens(source) {
            if text.to_lowercase().contains(query_lc) {
                matches.push(AstMatch {
                    line,
                    snippet: line_snippet(source, line),
                });
            }
        }
    }
    if want_identifiers && matches!(pattern, "all" | "call" | "assign" | "return") {
        for (line, head) in list_heads(source) {
            let include = match pattern {
                "call" => !SPECIAL_FORMS.contains(&head.as_str()),
                "assign" => ASSIGN_FORMS.contains(&head.as_str()),
                "return" => RETURN_FORMS.contains(&head.as_str()),
                _ => true,
            };
            if include && head.to_lowercase().contains(query_lc) {
                matches.push(AstMatch {
                    line,
                    snippet: line_snippet(source, line),
                });
            }
        }
        if pattern == "all" {
            for (line, symbol) in symbol_tokens(source) {
                if symbol.to_lowercase().contains(query_lc) {
                    matches.push(AstMatch {
                        line,
                        snippet: line_snippet(source, line),
                    });
                }
            }
        }
    }

    matches.sort_by_key(|m| m.line);
    matches.dedup_by(|a, b| a.line == b.line);
    matches
}

fn list_heads(source: &str) -> Vec<(u32, String)> {
    let mut heads = Vec::new();
    for_each_list_head(source, 0, source.len(), |head, line| {
        heads.push((line, head));
    });
    heads
}

fn symbol_tokens(source: &str) -> Vec<(u32, String)> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while let Some((token, next)) = next_token(source, cursor, source.len()) {
        if let Token::Atom(atom) = token {
            if !atom.starts_with(':') && !atom.starts_with('^') {
                let line = line_number_at(source, cursor);
                tokens.push((line, atom));
            }
        }
        cursor = next;
    }
    tokens
}

fn comment_tokens(source: &str) -> Vec<(u32, String)> {
    let mut comments = Vec::new();
    let mut line = 1u32;
    let mut chars = source.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some((_, ch)) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            } else if ch == '\n' {
                line += 1;
            }
            continue;
        }
        match ch {
            '\n' => line += 1,
            '"' => in_string = true,
            ';' => {
                let mut comment = String::new();
                while let Some((_, next)) = chars.next() {
                    if next == '\n' {
                        line += 1;
                        break;
                    }
                    comment.push(next);
                }
                comments.push((line, comment.trim().to_string()));
            }
            '\\' => {
                let _ = chars.next();
            }
            _ => {}
        }
    }
    comments
}

fn string_tokens(source: &str) -> Vec<(u32, String)> {
    let mut strings = Vec::new();
    let mut line = 1u32;
    let mut chars = source.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();
    let mut string_line = line;

    while let Some((_, ch)) = chars.next() {
        if in_string {
            if escaped {
                current.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => {
                    strings.push((string_line, current.clone()));
                    current.clear();
                    in_string = false;
                }
                '\n' => {
                    current.push(ch);
                    line += 1;
                }
                _ => current.push(ch),
            }
            continue;
        }
        match ch {
            '\n' => line += 1,
            '"' => {
                in_string = true;
                string_line = line;
            }
            ';' => {
                while let Some((_, next)) = chars.next() {
                    if next == '\n' {
                        line += 1;
                        break;
                    }
                }
            }
            '\\' => {
                let _ = chars.next();
            }
            _ => {}
        }
    }

    strings
}

fn for_each_list_head<F>(source: &str, start: usize, end: usize, mut f: F)
where
    F: FnMut(String, u32),
{
    let mut chars = source[start..end].char_indices().peekable();
    let mut line = line_number_at(source, start);
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;

    while let Some((offset, ch)) = chars.next() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                line += 1;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            } else if ch == '\n' {
                line += 1;
            }
            continue;
        }
        match ch {
            '\n' => line += 1,
            ';' => in_comment = true,
            '"' => in_string = true,
            '\\' => {
                let _ = chars.next();
            }
            '(' => {
                let absolute = start + offset + ch.len_utf8();
                if let Some(head) = head_symbol(source, absolute, end) {
                    f(head, line);
                }
            }
            _ => {}
        }
    }
}

fn head_symbol(source: &str, start: usize, end: usize) -> Option<String> {
    next_plain_symbol(source, start, end).map(|(symbol, _)| symbol)
}

fn next_plain_symbol(source: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let mut cursor = start;
    while cursor < end {
        cursor = skip_ws_comments(source, cursor, end);
        if cursor >= end {
            return None;
        }
        let ch = source[cursor..].chars().next()?;
        if ch == '^' {
            cursor += ch.len_utf8();
            cursor = skip_ws_comments(source, cursor, end);
            if cursor < end {
                cursor = advance_form(source, cursor, end);
            }
            continue;
        }
        if ch == '#' {
            let mut iter = source[cursor..].chars();
            let _ = iter.next();
            if matches!(iter.next(), Some('_')) {
                cursor += 2;
                cursor = skip_ws_comments(source, cursor, end);
                if cursor < end {
                    cursor = advance_form(source, cursor, end);
                }
                continue;
            }
        }
        if matches!(ch, '(' | '[' | '{' | '"') {
            cursor = advance_form(source, cursor, end);
            continue;
        }
        let (token, next) = read_atom(source, cursor, end)?;
        if token.starts_with(':') || token.starts_with('^') || token == "&" {
            cursor = next;
            continue;
        }
        return Some((token, next));
    }
    None
}

fn skip_ws_comments(source: &str, mut cursor: usize, end: usize) -> usize {
    while cursor < end {
        let ch = match source[cursor..].chars().next() {
            Some(ch) => ch,
            None => break,
        };
        if ch.is_whitespace() || ch == ',' {
            cursor += ch.len_utf8();
            continue;
        }
        if ch == ';' {
            while cursor < end {
                let next = match source[cursor..].chars().next() {
                    Some(next) => next,
                    None => break,
                };
                cursor += next.len_utf8();
                if next == '\n' {
                    break;
                }
            }
            continue;
        }
        break;
    }
    cursor
}

fn advance_form(source: &str, cursor: usize, end: usize) -> usize {
    let ch = match source[cursor..].chars().next() {
        Some(ch) => ch,
        None => return end,
    };
    match ch {
        '"' => advance_string(source, cursor, end),
        '(' | '[' | '{' => advance_collection(source, cursor, end),
        ';' => skip_ws_comments(source, cursor, end),
        '\\' => {
            let mut next = cursor + ch.len_utf8();
            if let Some(ch) = source[next..].chars().next() {
                next += ch.len_utf8();
            }
            next
        }
        _ => read_atom(source, cursor, end)
            .map(|(_, next)| next)
            .unwrap_or(end),
    }
}

fn advance_string(source: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut escaped = false;
    while cursor < end {
        let ch = match source[cursor..].chars().next() {
            Some(ch) => ch,
            None => return end,
        };
        cursor += ch.len_utf8();
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '"' && cursor > start + 1 {
            return cursor;
        }
    }
    end
}

fn advance_collection(source: &str, start: usize, end: usize) -> usize {
    let open = match source[start..].chars().next() {
        Some(ch) => ch,
        None => return end,
    };
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return start + open.len_utf8(),
    };
    let mut cursor = start + open.len_utf8();
    let mut depth = 1u32;
    let mut in_string = false;
    let mut escaped = false;
    while cursor < end {
        let ch = match source[cursor..].chars().next() {
            Some(ch) => ch,
            None => return end,
        };
        if in_string {
            cursor += ch.len_utf8();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                cursor += ch.len_utf8();
            }
            ';' => {
                cursor = skip_ws_comments(source, cursor, end);
            }
            '\\' => {
                cursor += ch.len_utf8();
                if cursor < end {
                    if let Some(next) = source[cursor..].chars().next() {
                        cursor += next.len_utf8();
                    }
                }
            }
            c if c == open => {
                depth += 1;
                cursor += c.len_utf8();
            }
            c if c == close => {
                depth -= 1;
                cursor += c.len_utf8();
                if depth == 0 {
                    return cursor;
                }
            }
            _ => cursor += ch.len_utf8(),
        }
    }
    end
}

#[derive(Debug)]
enum Token {
    Atom(String),
    String,
    Open(char),
    Close,
}

fn next_token(source: &str, start: usize, end: usize) -> Option<(Token, usize)> {
    let cursor = skip_ws_comments(source, start, end);
    if cursor >= end {
        return None;
    }
    let ch = source[cursor..].chars().next()?;
    match ch {
        '(' | '[' | '{' => Some((Token::Open(ch), cursor + ch.len_utf8())),
        ')' | ']' | '}' => Some((Token::Close, cursor + ch.len_utf8())),
        '"' => {
            let next = advance_string(source, cursor, end);
            Some((Token::String, next))
        }
        _ => read_atom(source, cursor, end).map(|(atom, next)| (Token::Atom(atom), next)),
    }
}

fn read_atom(source: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let mut cursor = start;
    while cursor < end {
        let ch = source[cursor..].chars().next()?;
        if ch.is_whitespace()
            || ch == ','
            || matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '"' | ';')
        {
            break;
        }
        cursor += ch.len_utf8();
    }
    if cursor == start {
        None
    } else {
        Some((source[start..cursor].to_string(), cursor))
    }
}

fn line_number_at(source: &str, byte_idx: usize) -> u32 {
    source[..byte_idx.min(source.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count() as u32
        + 1
}

fn line_snippet(source: &str, line: u32) -> String {
    source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .map(|line| line.trim().to_string())
        .unwrap_or_default()
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
        assert!(errors
            .iter()
            .any(|err| err.text.contains("unclosed delimiter")));
    }
}
