"""Tests for topos.evaluation.file_roles, notably the new
`is_stable_leaf_module` predicate (issue #124)."""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.file_roles import is_stable_leaf_module


def test_declarations_only_module_is_stable_leaf():
    morphism = ProgramMorphism(
        source="pub const X: i32 = 5;\npub const Y: i32 = 10;\n",
        language="rust",
    )
    assert is_stable_leaf_module(morphism) is True


def test_module_with_branching_control_flow_is_not_stable_leaf():
    morphism = ProgramMorphism(
        source="fn f(x: i32) -> i32 { if x > 0 { x } else { -x } }",
        language="rust",
    )
    assert is_stable_leaf_module(morphism) is False


def test_trivial_return_only_function_is_still_stable_leaf():
    # CallExpr/ReturnStmt are deliberately excluded from the disqualifying
    # set — a trivial accessor shouldn't disqualify an otherwise frozen
    # leaf module.
    morphism = ProgramMorphism(
        source="def get_x():\n    return compute_default()\n",
        language="python",
    )
    assert is_stable_leaf_module(morphism) is True


def test_module_with_loop_is_not_stable_leaf():
    source = (
        "def f(xs):\n"
        "    total = 0\n"
        "    for x in xs:\n"
        "        total += x\n"
        "    return total\n"
    )
    morphism = ProgramMorphism(source=source, language="python")
    assert is_stable_leaf_module(morphism) is False
