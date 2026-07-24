//! Python → UAST mapper.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::mapper_common::{logical_operator_attribute, map_tree_sitter_to_uast, TestNodeFilter};
use super::models::{AttributeValue, UASTNode};

const DUNDER_NAME: &[u8] = b"__name__";
const DUNDER_MAIN: &[u8] = b"__main__";

/// True if `condition` is `__name__ == "__main__"` (in either order).
///
/// Tree-sitter-python's `comparison_operator` node stores its operator(s)
/// under the `operators` field and leaves the operand nodes unlabeled,
/// so operands are everything that *isn't* the `operators` field.
fn is_name_equals_main(condition: Node, source: &[u8]) -> bool {
    if condition.kind() != "comparison_operator" {
        return false;
    }

    let mut cursor = condition.walk();
    let mut operators: Vec<&str> = Vec::new();
    let mut operands: Vec<Node> = Vec::new();
    for (index, child) in condition.children(&mut cursor).enumerate() {
        if condition.field_name_for_child(index as u32) == Some("operators") {
            operators.push(child.kind());
        } else {
            operands.push(child);
        }
    }

    if operators != ["=="] || operands.len() != 2 {
        return false;
    }

    let stripped: HashSet<&[u8]> = operands
        .iter()
        .map(|operand| strip_quotes(operand.utf8_text(source).unwrap_or("").as_bytes()))
        .collect();
    stripped == HashSet::from([DUNDER_NAME, DUNDER_MAIN])
}

/// Strip a single layer of matching leading/trailing `'` or `"`.
fn strip_quotes(text: &[u8]) -> &[u8] {
    for quote in *b"'\"" {
        if text.len() >= 2 && text.first() == Some(&quote) && text.last() == Some(&quote) {
            return &text[1..text.len() - 1];
        }
    }
    text
}

/// Python's [`TestNodeFilter`]: drop `if __name__ == "__main__":` guards.
///
/// The guard is fully self-contained (condition + body live under the
/// `if_statement` node itself), so unlike Rust's `#[cfg(test)]` this
/// needs no cross-sibling correlation — each candidate is classified
/// purely from its own subtree, which takes the guard's body with it
/// once dropped.
pub struct MainGuardFilter;

impl TestNodeFilter for MainGuardFilter {
    fn drop_set(&self, named_siblings: &[Node], source: &[u8]) -> HashSet<usize> {
        named_siblings
            .iter()
            .filter(|node| is_guard(node, source))
            .map(Node::id)
            .collect()
    }
}

fn is_guard(node: &Node, source: &[u8]) -> bool {
    if node.kind() != "if_statement" {
        return false;
    }
    match node.child_by_field_name("condition") {
        Some(condition) if is_name_equals_main(condition, source) => {
            // Only a *bare* guard is pure entrypoint scaffolding. A guard
            // carrying an `else`/`elif` holds real fallback logic; dropping the
            // whole `if_statement` would silently discard that branch, so keep
            // it (matches Python's `_is_guard`). A full fix — dropping only the
            // `__main__` consequence while keeping the alternative — needs
            // subtree rewriting the drop-by-id filter can't express.
            node.child_by_field_name("alternative").is_none()
        }
        _ => false,
    }
}

pub fn map_node_kind(kind: &str) -> &'static str {
    const NODE_KIND_TABLE: &[(&str, &str)] = &[
        ("function_definition", "FunctionDecl"),
        ("class_definition", "TypeDecl"),
        ("lexical_declaration", "VarDecl"),
        ("variable_declaration", "VarDecl"),
        ("if_statement", "IfStmt"),
        ("for_statement", "ForStmt"),
        ("while_statement", "WhileStmt"),
        ("match_statement", "MatchStmt"),
        ("return_statement", "ReturnStmt"),
        ("break_statement", "BreakStmt"),
        ("continue_statement", "ContinueStmt"),
        ("try_statement", "TryStmt"),
        ("except_clause", "CatchClause"),
        ("with_statement", "WithStmt"),
        ("assert_statement", "AssertStmt"),
        ("conditional_expression", "ConditionalExpr"),
        ("list_comprehension", "Comprehension"),
        ("set_comprehension", "Comprehension"),
        ("dictionary_comprehension", "Comprehension"),
        ("generator_expression", "Comprehension"),
        ("expression_statement", "ExprStmt"),
        ("assignment", "AssignExpr"),
        ("augmented_assignment", "AssignExpr"),
        ("binary_expression", "BinaryExpr"),
        ("boolean_operator", "BinaryExpr"),
        ("unary_expression", "UnaryExpr"),
        ("call", "CallExpr"),
        ("call_expression", "CallExpr"),
        ("member_expression", "MemberExpr"),
        ("field_expression", "MemberExpr"),
        ("subscript", "MemberExpr"),
        ("identifier", "Identifier"),
        ("module", "File"),
        ("program", "File"),
        ("translation_unit", "File"),
        ("source_file", "File"),
    ];
    if let Some((_, mapped)) = NODE_KIND_TABLE.iter().find(|(k, _)| *k == kind) {
        return mapped;
    }
    if kind.ends_with("literal") || matches!(kind, "string" | "integer" | "float") {
        return "Literal";
    }
    "Unknown"
}

/// First-pass, name-based Abstractness heuristic -- no import-alias
/// resolution, so `from foo import ABC as Base` won't be recognized.
/// Good enough for the overwhelmingly common `abc.ABC` /
/// `typing.Protocol` spellings.
const ABSTRACT_BASE_MARKERS: &[&str] = &["ABC", "Protocol", "ABCMeta"];

fn has_abstract_base(node: &Node, source: &[u8]) -> bool {
    let Some(superclasses) = node.child_by_field_name("superclasses") else {
        return false;
    };
    let Ok(text) = superclasses.utf8_text(source) else {
        return false;
    };
    ABSTRACT_BASE_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
}

fn has_abstractmethod(node: &Node, source: &[u8]) -> bool {
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() != "decorated_definition" {
            continue;
        }
        let mut inner_cursor = child.walk();
        for grandchild in child.named_children(&mut inner_cursor) {
            if grandchild.kind() == "decorator"
                && grandchild
                    .utf8_text(source)
                    .is_ok_and(|text| text.contains("abstractmethod"))
            {
                return true;
            }
        }
    }
    false
}

fn extract_type_attributes(node: &Node, source: &[u8]) -> HashMap<String, AttributeValue> {
    if node.kind() != "class_definition" {
        return HashMap::new();
    }
    let is_abstract = has_abstract_base(node, source) || has_abstractmethod(node, source);
    let kind = if is_abstract {
        "abstractClass"
    } else {
        "class"
    };
    HashMap::from([(
        "typeKind".to_string(),
        AttributeValue::Str(kind.to_string()),
    )])
}

fn extract_attributes(node: &Node, source: &[u8]) -> HashMap<String, AttributeValue> {
    let mut attrs = extract_type_attributes(node, source);
    attrs.extend(logical_operator_attribute(node, source));
    attrs
}

pub fn map_python_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "python",
        map_node_kind,
        source,
        file,
        Some(&MainGuardFilter),
        Some(&extract_attributes),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn collect_kinds(node: &UASTNode, out: &mut HashSet<String>) {
        out.insert(node.native.node_kind.clone());
        for child in &node.children {
            collect_kinds(child, out);
        }
    }

    fn collect_binary_operators(node: &UASTNode, out: &mut Vec<String>) {
        if node.kind == "BinaryExpr" {
            if let Some(AttributeValue::Str(op)) = node.attributes.get("operator") {
                out.push(op.clone());
            }
        }
        for child in &node.children {
            collect_binary_operators(child, out);
        }
    }

    #[test]
    fn records_logical_operators_on_boolean_expr() {
        let source = "def f(a, b, c):\n    return a and b and c\n";
        let tree = parse(source);
        let uast = map_python_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut ops = Vec::new();
        collect_binary_operators(&uast, &mut ops);
        assert_eq!(ops, vec!["and", "and"]);
    }

    #[test]
    fn drops_dunder_main_guard() {
        let source = "def main():\n    print('hi')\n\nif __name__ == \"__main__\":\n    main()\n";
        let tree = parse(source);
        let uast = map_python_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut kinds = HashSet::new();
        collect_kinds(&uast, &mut kinds);
        assert!(!kinds.contains("if_statement"));
        assert!(!kinds.contains("comparison_operator"));
        assert!(kinds.contains("function_definition"));
    }

    #[test]
    fn keeps_unrelated_if_statements() {
        let source = "if value == \"__main__\":\n    pass\nif name != __name__:\n    pass\n";
        let tree = parse(source);
        let uast = map_python_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut kinds = HashSet::new();
        collect_kinds(&uast, &mut kinds);
        assert!(kinds.contains("if_statement"));
        assert!(kinds.contains("comparison_operator"));
    }

    #[test]
    fn drops_reversed_operand_order() {
        let source = "def main():\n    pass\n\nif \"__main__\" == __name__:\n    main()\n";
        let tree = parse(source);
        let uast = map_python_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut kinds = HashSet::new();
        collect_kinds(&uast, &mut kinds);
        assert!(!kinds.contains("if_statement"));
    }

    #[test]
    fn keeps_main_guard_carrying_an_else_branch() {
        // A `__main__` guard with an `else`/`elif` holds real fallback logic;
        // dropping the whole `if_statement` would silently delete that branch.
        // Python keeps it (mapper_python.py::_is_guard) — so must the Rust port.
        let source =
            "if __name__ == \"__main__\":\n    main()\nelse:\n    configure_as_library()\n";
        let tree = parse(source);
        let uast = map_python_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut kinds = HashSet::new();
        collect_kinds(&uast, &mut kinds);
        assert!(
            kinds.contains("if_statement"),
            "a __main__ guard with an else branch must be kept, not dropped"
        );
    }
}
