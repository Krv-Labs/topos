from topos.utils.tree_sitter import (
    PythonParser,
    find_errors,
    node_text,
    node_to_sexp,
    parse_python,
)


def test_parse_python():
    source = "x = 1 + 2"
    root = parse_python(source)
    assert root.type == "module"
    assert not root.has_error


def test_node_text():
    source = "def foo(): pass"
    root = parse_python(source)

    # The first child of module is usually the statement
    stmt = root.children[0]
    text = node_text(stmt, source)
    assert text == "def foo(): pass"


def test_find_errors():
    source = "def broken(:"
    root = parse_python(source)

    errors = find_errors(root)
    assert len(errors) > 0
    assert any(e.type == "ERROR" or e.is_missing for e in errors)


def test_node_to_sexp():
    source = "x = 1"
    root = parse_python(source)
    sexp = node_to_sexp(root)

    assert "module" in sexp
    assert "expression_statement" in sexp
    assert "assignment" in sexp


def test_python_parser_instance():
    parser = PythonParser()
    source = "y = 42"
    root = parser.parse(source)
    assert root.type == "module"

    root_bytes = parser.parse_bytes(source.encode("utf-8"))
    assert root_bytes.type == "module"
