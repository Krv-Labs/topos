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
    
    # Recursively check the UAST for nodes that might belong to the tests module
    def has_test_module(node, source):
        text = source[node.span.start_byte:node.span.end_byte]
        if "mod tests" in text:
            return True
        for child in node.children:
            if has_test_module(child, source):
                return True
        return False
    
    assert not has_test_module(result.uast_root, source)

def test_rust_keeps_main_function():
    source = """
fn main() {
    println!("Hello, world!");
}
"""
    result = parse_source(source, language="rust")
    assert result.uast_root is not None
    
    # Check if main function is still present
    text = source[result.uast_root.span.start_byte:result.uast_root.span.end_byte]
    assert "fn main" in text
