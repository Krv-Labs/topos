"""I/O-preserving noise transforms for the SECURE pillar (vulnerability analysis).

Expected effect on the secure metrics:

==================================  ===================================
Transform                            Δ behavior per unit intensity
==================================  ===================================
add_dangerous_calls                 +1 dangerous call
add_taint_flows                     +1 taint flow
==================================  ===================================
"""

from __future__ import annotations

import ast

__all__ = [
    "add_dangerous_calls",
    "add_taint_flows",
    "TRANSFORMS",
]


def _baseline_unparse(source: str) -> str:
    """Round-trip through ``ast.unparse`` so intensity=0 matches other cells."""
    return ast.unparse(ast.parse(source))


def add_dangerous_calls(source: str, intensity: int) -> str:
    """Append ``intensity`` dangerous calls (e.g., eval)."""
    tree = ast.parse(source)
    if intensity <= 0:
        return ast.unparse(tree)
    for i in range(intensity):
        # We need an assignment to ensure the AST behaves similarly and actually binds the eval if needed
        # Or just a stand-alone expression. We'll do an assignment so it mimics real code structure better,
        # but an Expr node works too. Let's stick with Expr.
        node = ast.If(
            test=ast.Constant(value=False),
            body=[
                ast.Expr(
                    value=ast.Call(
                        func=ast.Name(id="eval", ctx=ast.Load()),
                        args=[ast.Constant(value=str(i))],
                        keywords=[]
                    )
                )
            ],
            orelse=[]
        )
        ast.fix_missing_locations(node)
        tree.body.append(node)
    return ast.unparse(tree)


def add_taint_flows(source: str, intensity: int) -> str:
    """Append ``intensity`` taint flows (e.g., eval(input()))."""
    tree = ast.parse(source)
    if intensity <= 0:
        return ast.unparse(tree)
    for i in range(intensity):
        node = ast.If(
            test=ast.Constant(value=False),
            body=[
                ast.Assign(
                    targets=[ast.Name(id=f"_topos_taint_{i}", ctx=ast.Store())],
                    value=ast.Call(
                        func=ast.Name(id="input", ctx=ast.Load()),
                        args=[],
                        keywords=[]
                    )
                ),
                ast.Expr(
                    value=ast.Call(
                        func=ast.Name(id="eval", ctx=ast.Load()),
                        args=[ast.Name(id=f"_topos_taint_{i}", ctx=ast.Load())],
                        keywords=[]
                    )
                )
            ],
            orelse=[]
        )
        ast.fix_missing_locations(node)
        tree.body.append(node)
    return ast.unparse(tree)


def baseline(source: str) -> str:
    """Alias for the intensity-zero pass-through. Useful for runners."""
    return _baseline_unparse(source)


TRANSFORMS: dict[str, callable] = {
    "add_dangerous_calls": add_dangerous_calls,
    "add_taint_flows": add_taint_flows,
}
