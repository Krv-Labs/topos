from topos.graphs.ast.dispatch import parse_source


def test_rust_filter_cfg_test_module():
    source = """
fn main() {
    println!("Hello, world!");
}
    
#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
"""
    result = parse_source(source, language="rust")

    # Debug: Print the S-expression or structure to see if it is still there
    from topos.utils.tree_sitter import node_to_sexp

    print(f"DEBUG: SEXP: {node_to_sexp(result.root)}")

    assert result.uast_root is not None

    # Walk the UAST by native node kind rather than span text: every node's
    # span still reports its true byte range (a File node always spans the
    # whole document), so slicing `source[span]` on an ancestor trivially
    # contains "mod tests" regardless of what was filtered from its
    # children. Checking which native kinds survived is the precise signal.
    def collect_node_kinds(node):
        kinds = {node.native.node_kind}
        for child in node.children:
            kinds |= collect_node_kinds(child)
        return kinds

    kinds = collect_node_kinds(result.uast_root)
    assert "mod_item" not in kinds
    assert "declaration_list" not in kinds


def test_rust_keeps_main_function():
    source = """
fn main() {
    println!("Hello, world!");
}
"""
    result = parse_source(source, language="rust")
    assert result.uast_root is not None

    # Check if main function is still present
    text = source[result.uast_root.span.start_byte : result.uast_root.span.end_byte]
    assert "fn main" in text
