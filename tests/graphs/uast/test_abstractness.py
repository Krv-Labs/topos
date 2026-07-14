"""Tests for Martin's Abstractness metric (issue #124): per-language
`typeKind` extraction in the UAST mappers, and `calculate_abstractness`
aggregation over a parsed tree.
"""

from __future__ import annotations

from topos.functors.probes.uast.abstractness import calculate_abstractness
from topos.graphs.uast.mapper_cpp import map_cpp_tree_to_uast
from topos.graphs.uast.mapper_go import map_go_tree_to_uast
from topos.graphs.uast.mapper_python import map_python_tree_to_uast
from topos.graphs.uast.mapper_rust import map_rust_tree_to_uast
from topos.graphs.uast.mapper_typescript import map_typescript_tree_to_uast
from topos.utils.tree_sitter import (
    parse_cpp,
    parse_go,
    parse_python,
    parse_rust,
    parse_typescript,
)


def _type_kinds(root) -> list[str]:
    kinds: list[str] = []

    def walk(node):
        tk = node.attributes.get("typeKind")
        if tk is not None:
            kinds.append(tk)
        for child in node.children:
            walk(child)

    walk(root)
    return kinds


def test_rust_trait_is_abstract_struct_enum_are_concrete():
    src = """
trait Shape { fn area(&self) -> f64; }
struct Circle { r: f64 }
enum Color { Red, Blue }
impl Shape for Circle { fn area(&self) -> f64 { 1.0 } }
"""
    root = map_rust_tree_to_uast(parse_rust(src))
    assert sorted(_type_kinds(root)) == ["enum", "struct", "trait"]
    assert calculate_abstractness(root) == 1 / 3


def test_rust_impl_block_not_double_counted():
    # A trait + its impl for one struct should count exactly 2 TypeDecls
    # (trait, struct), not 3 — impl_item is not a type declaration.
    src = """
trait Shape { fn area(&self) -> f64; }
struct Circle { r: f64 }
impl Shape for Circle { fn area(&self) -> f64 { 1.0 } }
"""
    root = map_rust_tree_to_uast(parse_rust(src))
    assert len(_type_kinds(root)) == 2


def test_rust_orchestrator_with_no_types_is_fully_concrete():
    src = "fn main() { let x = foo(); bar(x); }"
    root = map_rust_tree_to_uast(parse_rust(src))
    assert _type_kinds(root) == []
    assert calculate_abstractness(root) == 0.0


def test_python_abc_and_protocol_are_abstract():
    src = """
import abc
from abc import ABC
from typing import Protocol

class Shape(ABC):
    @abc.abstractmethod
    def area(self): ...

class Circle(Shape):
    def area(self): return 1.0

class Reader(Protocol):
    def read(self) -> str: ...

class Plain:
    x: int = 1
"""
    root = map_python_tree_to_uast(parse_python(src))
    kinds = _type_kinds(root)
    assert kinds.count("abstractClass") == 2  # Shape, Reader
    assert kinds.count("class") == 2  # Circle, Plain
    assert calculate_abstractness(root) == 0.5


def test_python_subclass_of_abstract_is_still_concrete():
    # Concrete-ness is per-declaration, not inherited: Circle subclasses an
    # ABC but has no abstract markers of its own.
    src = """
from abc import ABC
class Shape(ABC):
    pass
class Circle(Shape):
    pass
"""
    root = map_python_tree_to_uast(parse_python(src))
    kinds = _type_kinds(root)
    assert kinds == ["abstractClass", "class"]


def test_go_interface_is_abstract_struct_is_concrete():
    src = """
package main
type Shape interface { Area() float64 }
type Circle struct { R float64 }
type Alias = int
"""
    root = map_go_tree_to_uast(parse_go(src))
    kinds = _type_kinds(root)
    assert kinds == ["interface", "struct"]  # Alias -> no typeKind, excluded
    assert calculate_abstractness(root) == 0.5


def test_typescript_interface_and_abstract_class_are_abstract():
    src = """
interface Shape { area(): number }
abstract class Base { abstract area(): number }
class Circle implements Shape { area() { return 1.0 } }
enum Color { Red, Blue }
"""
    root = map_typescript_tree_to_uast(parse_typescript(src))
    kinds = _type_kinds(root)
    assert kinds.count("interface") == 1
    assert kinds.count("abstractClass") == 1
    assert kinds.count("class") == 1
    assert kinds.count("enum") == 1
    assert calculate_abstractness(root) == 0.5


def test_cpp_pure_virtual_class_is_abstract_function_pointer_is_not_type():
    src = """
class Shape { public: virtual double area() const = 0; };
struct Circle { double r; };
void (*handler)(int);
"""
    root = map_cpp_tree_to_uast(parse_cpp(src))
    kinds = _type_kinds(root)
    assert kinds == ["abstractClass", "struct"]
    assert calculate_abstractness(root) == 0.5
