use ast_grep_language::{LanguageExt, SupportLang};
use tree_sitter::Parser;

fn walk(node: tree_sitter::Node, src: &[u8], depth: usize) {
    let kind = node.kind();
    let text = if node.child_count() == 0 {
        format!("= {:?}", node.utf8_text(src).unwrap_or("").chars().take(20).collect::<String>())
    } else { String::new() };
    println!("{}{} {}", "  ".repeat(depth), kind, text);
    if depth < 7 {
        let mut c = node.walk();
        for child in node.children(&mut c) { walk(child, src, depth+1); }
    }
}

fn main() {
    let src = b"public class Foo {\n    public void bar(int x) { return; }\n    private String baz() { return \"hi\"; }\n}";
    let mut parser = Parser::new();
    parser.set_language(&SupportLang::Java.get_ts_language()).unwrap();
    let tree = parser.parse(src, None).unwrap();
    walk(tree.root_node(), src, 0);
}
