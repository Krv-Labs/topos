from topos.graphs.ast.dispatch import parse_source


def test_python_filter_dunder_main_guard():
    source = """
def main():
    print("Hello, world!")


if __name__ == "__main__":
    main()
"""
    result = parse_source(source, language="python")

    assert result.uast_root is not None

    # Walk the UAST by native node kind rather than span text: every node's
    # span still reports its true byte range (a File node always spans the
    # whole document), so slicing `source[span]` on an ancestor trivially
    # contains "__main__" regardless of what was filtered from its children.
    # Checking which native kinds survived is the precise signal (this
    # guards against the same class of bug #126 fixed: a filter that only
    # looks like it worked because a substring check on the root span still
    # passes).
    def collect_node_kinds(node):
        kinds = {node.native.node_kind}
        for child in node.children:
            kinds |= collect_node_kinds(child)
        return kinds

    kinds = collect_node_kinds(result.uast_root)
    assert "if_statement" not in kinds
    assert "comparison_operator" not in kinds


def test_python_keeps_main_function():
    source = """
def main():
    print("Hello, world!")


if __name__ == "__main__":
    main()
"""
    result = parse_source(source, language="python")
    assert result.uast_root is not None

    def collect_node_kinds(node):
        kinds = {node.native.node_kind}
        for child in node.children:
            kinds |= collect_node_kinds(child)
        return kinds

    kinds = collect_node_kinds(result.uast_root)
    assert "function_definition" in kinds

    text = source[result.uast_root.span.start_byte : result.uast_root.span.end_byte]
    assert "def main" in text


def test_python_keeps_unrelated_if_statements():
    """Only the `__name__ == "__main__"` guard is test-only; ordinary
    `if` statements — including ones that reference `__name__` or
    `"__main__"` individually, or compare against other values — must
    survive unfiltered."""
    source = """
def describe(value):
    if value == "__main__":
        return "entrypoint-like string"
    return "other"


def check_module_name(name):
    if name != __name__:
        return False
    return True
"""
    result = parse_source(source, language="python")
    assert result.uast_root is not None

    def collect_node_kinds(node):
        kinds = {node.native.node_kind}
        for child in node.children:
            kinds |= collect_node_kinds(child)
        return kinds

    kinds = collect_node_kinds(result.uast_root)
    assert "if_statement" in kinds
    assert "comparison_operator" in kinds


def test_python_filter_reversed_operand_order():
    """The guard is sometimes written with operands reversed:
    `"__main__" == __name__`. The predicate must still catch it."""
    source = """
def main():
    print("Hello, world!")


if "__main__" == __name__:
    main()
"""
    result = parse_source(source, language="python")
    assert result.uast_root is not None

    def collect_node_kinds(node):
        kinds = {node.native.node_kind}
        for child in node.children:
            kinds |= collect_node_kinds(child)
        return kinds

    kinds = collect_node_kinds(result.uast_root)
    assert "if_statement" not in kinds
    assert "comparison_operator" not in kinds
