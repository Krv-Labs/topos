"""Regression tests for issue #153.

``calculate_function_complexities``/``calculate_function_complexity_entries``
used to find function nodes via Python-specific tree-sitter native node type
strings (``function_definition``/``async_function_definition``). For every
other language that query matched zero nodes, so
``calculate_max_function_complexity`` silently returned ``0`` -- always
within the ``<= 10.0`` SIMPLE gate, a vacuous pass rather than a real
evaluation.

These tests reproduce the bug for each language with an existing UAST
mapper (Rust, Go, JavaScript, TypeScript): a genuinely branchy function that
must NOT score complexity 1 (or, pre-fix, complexity 0 via
``calculate_max_function_complexity``).
"""

from __future__ import annotations

import pytest
from topos.core.morphism import ProgramMorphism
from topos.functors.probes.ast.complexity import (
    calculate_function_complexity_entries,
    calculate_max_function_complexity,
)
from topos.graphs.ast.object import ASTRepresentation

# A function with two `if`s, a `for` containing a nested `if`, and either a
# `match`/`switch` or a trailing `while` (decision-node coverage differs
# slightly per language's UAST mapper -- see complexity.py) -- six decision
# nodes total, so complexity should be 6, never 0 or 1.
_BRANCHY_SOURCES: dict[str, tuple[str, int]] = {
    "rust": (
        "fn classify(x: i32) -> i32 {\n"
        "    if x < 0 {\n"
        "        return -1;\n"
        "    }\n"
        "    if x == 0 {\n"
        "        return 0;\n"
        "    }\n"
        "    for i in 0..x {\n"
        "        if i % 2 == 0 {\n"
        "            continue;\n"
        "        }\n"
        "    }\n"
        "    match x {\n"
        "        1 => 1,\n"
        "        2 => 2,\n"
        "        _ => 3,\n"
        "    }\n"
        "}\n",
        6,
    ),
    "go": (
        "package main\n\n"
        "func classify(x int) int {\n"
        "\tif x < 0 {\n"
        "\t\treturn -1\n"
        "\t}\n"
        "\tif x == 0 {\n"
        "\t\treturn 0\n"
        "\t}\n"
        "\tfor i := 0; i < x; i++ {\n"
        "\t\tif i%2 == 0 {\n"
        "\t\t\tcontinue\n"
        "\t\t}\n"
        "\t}\n"
        "\tswitch x {\n"
        "\tcase 1:\n"
        "\t\treturn 1\n"
        "\tcase 2:\n"
        "\t\treturn 2\n"
        "\tdefault:\n"
        "\t\treturn 3\n"
        "\t}\n"
        "\treturn 4\n"
        "}\n",
        6,
    ),
    "javascript": (
        "function classify(x) {\n"
        "  if (x < 0) {\n"
        "    return -1;\n"
        "  }\n"
        "  if (x === 0) {\n"
        "    return 0;\n"
        "  }\n"
        "  for (let i = 0; i < x; i++) {\n"
        "    if (i % 2 === 0) {\n"
        "      continue;\n"
        "    }\n"
        "  }\n"
        "  while (x > 100) {\n"
        "    x -= 1;\n"
        "  }\n"
        "  return x;\n"
        "}\n",
        6,
    ),
    "typescript": (
        "function classify(x: number): number {\n"
        "  if (x < 0) {\n"
        "    return -1;\n"
        "  }\n"
        "  if (x === 0) {\n"
        "    return 0;\n"
        "  }\n"
        "  for (let i = 0; i < x; i++) {\n"
        "    if (i % 2 === 0) {\n"
        "      continue;\n"
        "    }\n"
        "  }\n"
        "  while (x > 100) {\n"
        "    x -= 1;\n"
        "  }\n"
        "  return x;\n"
        "}\n",
        6,
    ),
}


@pytest.mark.parametrize("language", sorted(_BRANCHY_SOURCES))
def test_branchy_function_is_not_vacuously_zero_or_one(language: str) -> None:
    source, expected_complexity = _BRANCHY_SOURCES[language]
    ast = ProgramMorphism(source=source, language=language).ast
    assert ast.uast_root is not None  # sanity: exercising the real UAST path

    entries = calculate_function_complexity_entries(ast)
    assert len(entries) == 1
    assert entries[0].name == "classify"
    assert entries[0].complexity == expected_complexity
    assert entries[0].complexity > 1

    max_complexity = calculate_max_function_complexity(ast)
    assert max_complexity == expected_complexity
    assert max_complexity > 1


@pytest.mark.parametrize("language", sorted(_BRANCHY_SOURCES))
def test_ast_representation_metric_is_not_vacuously_zero(language: str) -> None:
    """End-to-end: the exact gate metric consumed by the SIMPLE evaluator."""
    source, expected_complexity = _BRANCHY_SOURCES[language]
    morphism = ProgramMorphism(source=source, language=language)
    representation = ASTRepresentation(program_object=morphism.ast, source=source)
    metrics = representation.metrics()
    assert metrics["ast.max_function_complexity"] == float(expected_complexity)


_METHOD_SOURCES: dict[str, str] = {
    "rust": (
        "struct Foo;\n\n"
        "impl Foo {\n"
        "    fn bar(&self, x: i32) -> i32 {\n"
        "        if x > 0 {\n"
        "            return 1;\n"
        "        }\n"
        "        return 0;\n"
        "    }\n"
        "}\n"
    ),
    "go": (
        "package main\n\n"
        "type Foo struct{}\n\n"
        "func (f Foo) Bar(x int) int {\n"
        "\tif x > 0 {\n"
        "\t\treturn 1\n"
        "\t}\n"
        "\treturn 0\n"
        "}\n"
    ),
    "javascript": (
        "class Foo {\n"
        "  bar(x) {\n"
        "    if (x > 0) {\n"
        "      return 1;\n"
        "    }\n"
        "    return 0;\n"
        "  }\n"
        "}\n"
    ),
    "typescript": (
        "class Foo {\n"
        "  bar(x: number): number {\n"
        "    if (x > 0) {\n"
        "      return 1;\n"
        "    }\n"
        "    return 0;\n"
        "  }\n"
        "}\n"
    ),
}


@pytest.mark.parametrize("language", sorted(_METHOD_SOURCES))
def test_method_classified_across_languages(language: str) -> None:
    source = _METHOD_SOURCES[language]
    ast = ProgramMorphism(source=source, language=language).ast
    entries = calculate_function_complexity_entries(ast)
    assert len(entries) == 1
    entry = entries[0]
    expected_name = "Bar" if language == "go" else "bar"
    assert entry.name == expected_name
    assert entry.kind == "method"
    assert entry.complexity == 2  # one `if`


_ASYNC_SOURCES: dict[str, str] = {
    "rust": "async fn bar() -> i32 {\n    return 1;\n}\n",
    "javascript": "async function bar() {\n  return 1;\n}\n",
    "typescript": "async function bar(): Promise<number> {\n  return 1;\n}\n",
}


@pytest.mark.parametrize("language", sorted(_ASYNC_SOURCES))
def test_async_function_classified_across_languages(language: str) -> None:
    source = _ASYNC_SOURCES[language]
    ast = ProgramMorphism(source=source, language=language).ast
    entries = calculate_function_complexity_entries(ast)
    assert len(entries) == 1
    assert entries[0].name == "bar"
    assert entries[0].kind == "async_function"


def test_nested_closure_classified_for_javascript() -> None:
    source = (
        "function outer(x) {\n"
        "  function inner(y) {\n"
        "    if (y) {\n"
        "      return 1;\n"
        "    }\n"
        "    return 0;\n"
        "  }\n"
        "  return inner(x);\n"
        "}\n"
    )
    ast = ProgramMorphism(source=source, language="javascript").ast
    entries = {e.qualified_name: e for e in calculate_function_complexity_entries(ast)}
    assert entries["outer"].kind == "function"
    assert entries["outer.inner"].kind == "closure"
    assert entries["outer.inner"].complexity == 2


def test_cpp_function_name_is_recovered_through_declarator() -> None:
    source = "int classify(int x) { if (x > 0) { return 1; } return 0; }\n"
    ast = ProgramMorphism(source=source, language="cpp").ast
    entries = calculate_function_complexity_entries(ast)
    assert len(entries) == 1
    assert entries[0].name == "classify"
    assert entries[0].complexity == 2
    assert calculate_max_function_complexity(ast) == 2
