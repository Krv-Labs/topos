"""I/O-preserving structural noise transforms for the Self-Contained axis.

Each transform parses the source, appends synthetic statements whose names do
not collide with the originals, and unparses. By construction:

- The original module body is never rewritten in place; transforms only
  append new top-level statements.
- New bindings use the prefix ``_topos_<kind>_<i>``, which keeps them clear
  of typical user identifiers.
- ``intensity = 0`` is the no-op pass-through that establishes the
  "unparsed baseline" against which other intensities are compared.

Expected effect on the structural metrics (per
``topos.metrics.ast.complexity``):

==================================  ===================================
Transform                            Δ complexity per unit intensity
==================================  ===================================
inject_dead_branches                +1 (``if False:`` adds one decision)
add_boolean_chains                  +2 (``if`` + boolean ``and``)
duplicate_function_body             +(C-1), C = clone's own complexity
pad_with_noops                       0 (assignment only; moves entropy)
==================================  ===================================
"""

from __future__ import annotations

import ast
import copy

__all__ = [
    "inject_dead_branches",
    "add_boolean_chains",
    "duplicate_function_body",
    "pad_with_noops",
    "TRANSFORMS",
]


def _baseline_unparse(source: str) -> str:
    """Round-trip through ``ast.unparse`` so intensity=0 matches other cells."""
    return ast.unparse(ast.parse(source))


def inject_dead_branches(source: str, intensity: int) -> str:
    """Append ``intensity`` ``if False:`` blocks at module scope.

    Each appended ``If`` node adds exactly one ``if_statement`` decision
    point. The body of each branch is a fresh assignment that cannot be
    executed at runtime, so observable behavior is unchanged.
    """
    tree = ast.parse(source)
    if intensity <= 0:
        return ast.unparse(tree)
    for i in range(intensity):
        node = ast.If(
            test=ast.Constant(value=False),
            body=[
                ast.Assign(
                    targets=[ast.Name(id=f"_topos_dead_{i}", ctx=ast.Store())],
                    value=ast.Constant(value=i),
                )
            ],
            orelse=[],
        )
        ast.fix_missing_locations(node)
        tree.body.append(node)
    return ast.unparse(tree)


def add_boolean_chains(source: str, intensity: int) -> str:
    """Append ``intensity`` ``if True and True:`` blocks.

    Each appended block adds an ``if`` decision point plus a boolean
    ``and`` operator, raising complexity by 2.
    """
    tree = ast.parse(source)
    if intensity <= 0:
        return ast.unparse(tree)
    for i in range(intensity):
        node = ast.If(
            test=ast.BoolOp(
                op=ast.And(),
                values=[ast.Constant(value=True), ast.Constant(value=True)],
            ),
            body=[
                ast.Assign(
                    targets=[ast.Name(id=f"_topos_chain_{i}", ctx=ast.Store())],
                    value=ast.Constant(value=i),
                )
            ],
            orelse=[],
        )
        ast.fix_missing_locations(node)
        tree.body.append(node)
    return ast.unparse(tree)


def duplicate_function_body(source: str, intensity: int) -> str:
    """Clone the last top-level function ``intensity`` times under fresh names.

    Each clone preserves the original's internal decision points, so the
    complexity contribution scales with the clone's intrinsic complexity.
    Duplication makes the source highly compressible, dragging entropy
    toward 0.
    """
    tree = ast.parse(source)
    if intensity <= 0:
        return ast.unparse(tree)
    functions = [
        node
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    ]
    if not functions:
        return ast.unparse(tree)
    target = functions[-1]
    for i in range(intensity):
        clone = copy.deepcopy(target)
        clone.name = f"{target.name}__topos_dup_{i}"
        clone.decorator_list = []
        ast.fix_missing_locations(clone)
        tree.body.append(clone)
    return ast.unparse(tree)


def pad_with_noops(source: str, intensity: int) -> str:
    """Append ``intensity`` no-op assignments. Does not change complexity."""
    tree = ast.parse(source)
    if intensity <= 0:
        return ast.unparse(tree)
    for i in range(intensity):
        node = ast.Assign(
            targets=[ast.Name(id=f"_topos_noop_{i}", ctx=ast.Store())],
            value=ast.Constant(value=i),
        )
        ast.fix_missing_locations(node)
        tree.body.append(node)
    return ast.unparse(tree)


def baseline(source: str) -> str:
    """Alias for the intensity-zero pass-through. Useful for runners."""
    return _baseline_unparse(source)


TRANSFORMS: dict[str, callable] = {
    "inject_dead_branches": inject_dead_branches,
    "add_boolean_chains": add_boolean_chains,
    "duplicate_function_body": duplicate_function_body,
    "pad_with_noops": pad_with_noops,
}
