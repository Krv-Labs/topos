//! Tree-sitter parsing shared by the import rules.

use tree_sitter::{Language, Node, Parser, Tree};

pub(super) fn parse_tree(text: &str, language: Language) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    parser.parse(text, None)
}

/// Every named node under `root`, without recursion.
pub(super) fn named_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut stack = vec![root];
    let mut nodes = Vec::new();
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
        nodes.push(node);
    }
    nodes
}

pub(super) fn text(node: Node<'_>, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}
