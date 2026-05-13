from __future__ import annotations

from pathlib import Path

from topos.core.morphism import ProgramMorphism
from topos.graphs.ast.dispatch import get_capability_matrix, parse_source


def _walk_uast(node):
    yield node
    for child in node.children:
        yield from _walk_uast(child)


def test_parse_source_python_hybrid_has_native_and_uast():
    result = parse_source("def add(a, b):\n    return a + b\n", language="python")
    assert result.native_ast is not None
    assert result.uast_root is not None
    assert result.provenance.parser == "cpython-ast"


def test_parse_source_rust_hybrid_falls_back_to_tree_sitter():
    result = parse_source(
        'fn main() { let x = 1; if x > 0 { println!("ok"); } }',
        language="rust",
    )
    assert result.native_ast is None
    assert result.uast_root is not None
    assert result.provenance.parser == "tree-sitter-rust"


def test_parse_source_typescript_hybrid():
    result = parse_source("const x: number = 1;\n", language="typescript")
    assert result.native_ast is None
    assert result.uast_root is not None
    assert result.provenance.parser == "tree-sitter-typescript"


def test_parse_source_tsx_file_uses_tsx_grammar():
    result = parse_source(
        "export const el = <div />;\n",
        language="typescript",
        file="component.tsx",
    )
    assert result.uast_root is not None
    assert result.root.type == "program"
    assert not result.root.has_error


def test_program_morphism_supports_multiple_languages():
    rust_morphism = ProgramMorphism(source="fn main() {}", language="rust")
    js_morphism = ProgramMorphism(
        source="function x() { return 1; }",
        language="javascript",
    )
    cpp_morphism = ProgramMorphism(source="int main() { return 0; }", language="cpp")
    ts_morphism = ProgramMorphism(
        source="interface A { x: number }\n",
        language="typescript",
    )

    assert rust_morphism.ast is not None
    assert js_morphism.ast is not None
    assert cpp_morphism.ast is not None
    assert ts_morphism.ast is not None
    assert rust_morphism.ast.language == "rust"
    assert js_morphism.ast.language == "javascript"
    assert cpp_morphism.ast.language == "cpp"
    assert ts_morphism.ast.language == "typescript"


def test_capability_matrix_tracks_native_and_uast_support():
    matrix = get_capability_matrix()
    assert matrix["python"]["supports_native"] is True
    assert matrix["rust"]["supports_native"] is False
    assert matrix["javascript"]["supports_uast"] is True
    assert matrix["typescript"]["supports_uast"] is True
    assert matrix["cpp"]["supports_tree_sitter"] is True


def test_native_ref_parser_identity_per_language():
    cases = [
        ("python", "def add(a, b):\n    return a + b\n", "cpython-ast"),
        ("rust", "fn main() {}", "tree-sitter-rust"),
        ("javascript", "function x() { return 1; }", "tree-sitter-javascript"),
        ("typescript", "const x: number = 1;", "tree-sitter-typescript"),
        ("cpp", "int main() { return 0; }", "tree-sitter-cpp"),
    ]
    for language, source, expected_parser in cases:
        result = parse_source(source, language=language)
        assert result.provenance.parser == expected_parser
        assert result.provenance.parser_version
        assert result.provenance.parser_version != "unknown"
        parsers_seen = {node.native.parser for node in _walk_uast(result.uast_root)}
        # UAST nodes always carry the tree-sitter grammar identity, even when
        # the top-level provenance reflects the native parser (e.g. cpython-ast).
        expected_uast_parser = (
            "tree-sitter-python" if language == "python" else expected_parser
        )
        assert expected_uast_parser in parsers_seen


def test_deeply_nested_source_does_not_hit_recursion_limit():
    # Generates a Python expression with 200 levels of nesting, well above the
    # default recursion limit (~1000 frames) once parser overhead is counted.
    # Validates that the iterative mapper_common implementation handles deep trees.
    depth = 200
    source = "x = " + "(" * depth + "1" + ")" * depth + "\n"
    result = parse_source(source, language="python")
    assert result.uast_root is not None
    node_count = sum(1 for _ in _walk_uast(result.uast_root))
    assert node_count > 0


def test_parse_result_has_errors_is_false_for_valid_source():
    result = parse_source("def f(): return 1\n", language="python", backend="tree-sitter")
    assert result.has_errors is False


def test_parse_result_has_errors_is_true_for_invalid_source():
    # tree-sitter is error-tolerant and recovers, but sets has_error on the root.
    result = parse_source("def (((:\n", language="python", backend="tree-sitter")
    assert result.has_errors is True


def test_binarytrees_sources_produce_minimum_uast_invariants():
    src_dir = Path("demos/binarytrees/src")
    extension_map = {
        ".py": "python",
        ".js": "javascript",
        ".rs": "rust",
        ".cpp": "cpp",
    }

    for file in src_dir.iterdir():
        language = extension_map.get(file.suffix)
        if language is None:
            continue
        source = file.read_text(encoding="utf-8")
        parsed = parse_source(source=source, language=language, file=str(file))
        assert parsed.uast_root is not None
        for node in _walk_uast(parsed.uast_root):
            assert node.kind
            assert node.lang == language
            assert node.span.start_byte <= node.span.end_byte
            assert node.native.parser
            assert node.native.node_kind
