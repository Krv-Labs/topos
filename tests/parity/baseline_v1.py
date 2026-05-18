"""
Baseline implementations from v1.0.0 (Pure Python).
Used for implementation parity monitoring against the Rust backend.
"""

from __future__ import annotations

import zlib
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.cfg.object import ControlFlowGraph

# --- CFG Probes (v1.0.0) ---


def _connected_components(cfg: ControlFlowGraph) -> int:
    parent: dict[int, int] = {bid: bid for bid in cfg.blocks}

    def find(x: int) -> int:
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    def union(a: int, b: int) -> None:
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[ra] = rb

    for edge in cfg.edges:
        if edge.source in parent and edge.target in parent:
            union(edge.source, edge.target)

    roots = {find(b) for b in parent}
    return len(roots) or 1


def cyclomatic_complexity_v1(cfg: ControlFlowGraph) -> int:
    n = len(cfg.blocks)
    e = len(cfg.edges)
    p = _connected_components(cfg)
    return max(1, e - n + 2 * p)


def essential_complexity_v1(cfg: ControlFlowGraph) -> int:
    from topos.graphs.cfg.models import EdgeKind

    unstructured = 0
    for edge in cfg.edges:
        if edge.kind in {EdgeKind.BREAK, EdgeKind.CONTINUE, EdgeKind.RETURN}:
            unstructured += 1
    return max(1, unstructured + 1)


def max_nesting_depth_v1(cfg: ControlFlowGraph) -> int:
    from topos.graphs.cfg.models import EdgeKind

    nesting_edge_kinds = {EdgeKind.TRUE, EdgeKind.SWITCH_CASE}
    depth: dict[int, int] = {cfg.entry_id: 0}
    changed = True
    iterations = 0
    while changed and iterations < len(cfg.blocks) * 2:
        changed = False
        for edge in cfg.edges:
            if edge.source not in depth:
                continue
            inc = 1 if edge.kind in nesting_edge_kinds else 0
            candidate = depth[edge.source] + inc
            if candidate > depth.get(edge.target, -1):
                depth[edge.target] = candidate
                changed = True
        iterations += 1
    return max(depth.values()) if depth else 0


def longest_acyclic_path_v1(cfg: ControlFlowGraph) -> int:
    from topos.graphs.cfg.models import EdgeKind

    adj: dict[int, list[int]] = {bid: [] for bid in cfg.blocks}
    for edge in cfg.edges:
        if edge.kind is EdgeKind.LOOP_BACK:
            continue
        adj.setdefault(edge.source, []).append(edge.target)

    best = 0
    sys_block_count = len(cfg.blocks)

    def dfs(node: int, length: int, visited: set[int]) -> None:
        nonlocal best
        if node == cfg.exit_id:
            best = max(best, length)
            return
        if length > sys_block_count:
            return
        for nxt in adj.get(node, []):
            if nxt in visited:
                continue
            visited.add(nxt)
            dfs(nxt, length + 1, visited)
            visited.remove(nxt)

    dfs(cfg.entry_id, 0, {cfg.entry_id})
    return best


# --- AST Probes (v1.0.0) ---


def calculate_kolmogorov_proxy_v1(source: str) -> float:
    if not source:
        return 0.0
    source_bytes = source.encode("utf-8")
    compressed = zlib.compress(source_bytes, level=9)
    return len(compressed) / len(source_bytes)


# --- Profunctors (v1.0.0) ---


def compute_sequence_distance_v1(
    source: list[str], target: list[str]
) -> tuple[int, dict[str, int]]:
    m, n = len(source), len(target)
    dp = [[0] * (n + 1) for _ in range(m + 1)]
    for i in range(m + 1):
        dp[i][0] = i
    for j in range(n + 1):
        dp[0][j] = j
    for i in range(1, m + 1):
        for j in range(1, n + 1):
            if source[i - 1] == target[j - 1]:
                dp[i][j] = dp[i - 1][j - 1]
            else:
                dp[i][j] = 1 + min(dp[i - 1][j], dp[i][j - 1], dp[i - 1][j - 1])
    insertions = 0
    deletions = 0
    substitutions = 0
    i, j = m, n
    while i > 0 or j > 0:
        if i > 0 and j > 0 and source[i - 1] == target[j - 1]:
            i -= 1
            j -= 1
        elif i > 0 and j > 0 and dp[i][j] == dp[i - 1][j - 1] + 1:
            substitutions += 1
            i -= 1
            j -= 1
        elif j > 0 and dp[i][j] == dp[i][j - 1] + 1:
            insertions += 1
            j -= 1
        elif i > 0:
            deletions += 1
            i -= 1
        else:
            break
    return dp[m][n], {
        "insertions": insertions,
        "deletions": deletions,
        "substitutions": substitutions,
    }
