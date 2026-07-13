"""Regression tests for the C++ UAST mapper (issue #158).

Before this fix, `_DECLARATION_TYPES` used node names that don't exist in
the real `tree-sitter-cpp` grammar (copy-pasted from the Python/Rust
mappers), so every C++ class/struct/enum/union fell through to `Unknown`
instead of `TypeDecl`. These tests pin the correct classification against
the real grammar so that regression can't silently reappear.
"""

from __future__ import annotations

from topos.graphs.uast.mapper_cpp import map_cpp_tree_to_uast
from topos.utils.tree_sitter import parse_cpp


def _walk(root):
    yield root
    for child in root.children:
        yield from _walk(child)


def _kinds(root) -> list[str]:
    return [n.kind for n in _walk(root)]


def _type_kinds(root) -> list[str]:
    return [n.attributes["typeKind"] for n in _walk(root) if "typeKind" in n.attributes]


def test_class_struct_enum_union_are_type_decls_not_unknown():
    src = """
class Shape { public: virtual double area() const = 0; };
struct Circle : public Shape { double r; double area() const override { return 1.0; } };
enum class Color { Red, Blue };
union Value { int i; float f; };
"""
    root = map_cpp_tree_to_uast(parse_cpp(src))
    kinds = _kinds(root)
    assert kinds.count("TypeDecl") == 4
    assert "Unknown" not in kinds or kinds.count("Unknown") < len(kinds)


def test_pure_virtual_method_marks_class_abstract():
    src = "class Shape { public: virtual double area() const = 0; };"
    root = map_cpp_tree_to_uast(parse_cpp(src))
    assert _type_kinds(root) == ["abstractClass"]


def test_concrete_class_without_pure_virtual_is_not_abstract():
    src = "class Point { public: double x; double y; };"
    root = map_cpp_tree_to_uast(parse_cpp(src))
    assert _type_kinds(root) == ["class"]


def test_subclass_of_abstract_base_is_still_concrete():
    # Per-declaration classification, like the other language mappers:
    # Circle overrides the pure-virtual method with a real body, so it has
    # no pure-virtual of its own.
    src = """
class Shape { public: virtual double area() const = 0; };
struct Circle : public Shape { double area() const override { return 1.0; } };
"""
    root = map_cpp_tree_to_uast(parse_cpp(src))
    assert _type_kinds(root) == ["abstractClass", "struct"]


def test_enum_and_union_are_concrete():
    src = "enum class Color { Red, Blue }; union Value { int i; float f; };"
    root = map_cpp_tree_to_uast(parse_cpp(src))
    assert _type_kinds(root) == ["enum", "union"]


def test_forward_declaration_is_function_decl_not_var_decl():
    src = "void forward_decl(int x);"
    root = map_cpp_tree_to_uast(parse_cpp(src))
    assert _kinds(root).count("FunctionDecl") == 1
    assert "VarDecl" not in _kinds(root)


def test_global_variable_declaration_is_var_decl():
    src = "int global_x = 5;"
    root = map_cpp_tree_to_uast(parse_cpp(src))
    assert _kinds(root).count("VarDecl") == 1
    assert "FunctionDecl" not in _kinds(root)


def test_data_member_is_var_decl_pure_virtual_signature_is_function_decl():
    src = "class Shape { public: virtual double area() const = 0; double cached; };"
    root = map_cpp_tree_to_uast(parse_cpp(src))
    kinds = _kinds(root)
    assert kinds.count("FunctionDecl") == 1  # the pure-virtual signature
    assert kinds.count("VarDecl") == 1  # `double cached;`


def test_control_flow_and_expression_kinds_are_recognized():
    src = """
void f(int x) {
    if (x > 0) {
        for (int i = 0; i < x; i++) {
            x += i;
        }
    }
    obj.method();
    arr[0] = 1;
}
"""
    root = map_cpp_tree_to_uast(parse_cpp(src))
    kinds = _kinds(root)
    assert "IfStmt" in kinds
    assert "ForStmt" in kinds
    assert "AssignExpr" in kinds
    assert "CallExpr" in kinds
    assert "MemberExpr" in kinds  # both field_expression and subscript_expression
