"""
Structural test coverage (UAST)
-------------------------------
PUT-directed recall: how much of the program-under-test's UAST structure
is represented in the test suite's UAST, using kind histograms,
control-flow profiles, and optional k-gram path overlap.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Sequence
from dataclasses import dataclass

from topos.functors.probes.uast.signature import (
    CONTROL_FLOW_KINDS,
    control_flow_profile,
    uast_dfs_kind_sequence,
    uast_kind_histogram,
)

_STMT_KINDS: frozenset[str] = frozenset(
    {
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "ReturnStmt",
        "BreakStmt",
        "ContinueStmt",
        "ThrowStmt",
        "TryStmt",
        "ExprStmt",
    }
)

_EXPR_KINDS: frozenset[str] = frozenset(
    {
        "AssignExpr",
        "BinaryExpr",
        "UnaryExpr",
        "CallExpr",
        "MemberExpr",
    }
)

_DECL_KINDS: frozenset[str] = frozenset({"FunctionDecl", "MethodDecl"})


def merge_uast_kind_histograms(
    roots: Sequence[object],
    *,
    include_unknown: bool = False,
) -> dict[str, int]:
    """Sum ``uast_kind_histogram`` across multiple UAST roots."""
    merged: Counter[str] = Counter()
    for root in roots:
        merged.update(uast_kind_histogram(root, include_unknown=include_unknown))
    return dict(merged)


def merge_control_flow_profiles(roots: Sequence[object]) -> dict[str, int]:
    """Sum ``control_flow_profile`` across multiple UAST roots."""
    merged = {kind: 0 for kind in CONTROL_FLOW_KINDS}
    for root in roots:
        prof = control_flow_profile(root)
        for kind in CONTROL_FLOW_KINDS:
            merged[kind] += prof[kind]
    return merged


def _multiset_recall(counts_put: dict[str, int], counts_test: dict[str, int]) -> float:
    """
    sum_k min(n_P(k), n_T(k)) / sum_k n_P(k).

    Vacuous denominator (empty PUT multiset) yields 1.0.
    """
    denom = sum(counts_put.values())
    if denom == 0:
        return 1.0
    num = sum(min(counts_put[k], counts_test.get(k, 0)) for k in counts_put)
    return num / denom


def _kgrams_from_sequence(seq: list[str], k: int) -> Counter[tuple[str, ...]]:
    if k < 1:
        return Counter()
    c: Counter[tuple[str, ...]] = Counter()
    if len(seq) < k:
        return c
    for i in range(len(seq) - k + 1):
        c[tuple(seq[i : i + k])] += 1
    return c


def merge_kgram_counters(
    roots: Sequence[object],
    *,
    k: int,
    include_unknown: bool = False,
) -> Counter[tuple[str, ...]]:
    """
    Multiset of length-``k`` kind n-grams, aggregated per root then summed.

    Each root is DFS-sequenced independently; k-grams never span file boundaries.
    """
    total: Counter[tuple[str, ...]] = Counter()
    for root in roots:
        seq = uast_dfs_kind_sequence(root, include_unknown=include_unknown)
        total.update(_kgrams_from_sequence(seq, k))
    return total


def _kgram_recall(
    c_put: Counter[tuple[str, ...]],
    c_test: Counter[tuple[str, ...]],
) -> float:
    denom = sum(c_put.values())
    if denom == 0:
        return 1.0
    num = sum(min(c_put[g], c_test[g]) for g in c_put)
    return num / denom


@dataclass(frozen=True)
class StructuralTestCoverageReport:
    """Structural overlap from aggregated tests toward the PUT."""

    kind_recall: float
    control_flow_recall: float
    composite_v0: float
    path_recall_kgram: float
    k: int
    include_unknown: bool
    put_kind_nodes: int
    test_kind_nodes: int
    put_cf_nodes: int
    test_cf_nodes: int
    put_kgram_mass: int
    test_kgram_mass: int


def structural_test_coverage(
    put_roots: Sequence[object],
    test_roots: Sequence[object],
    *,
    k: int = 3,
    include_unknown: bool = False,
) -> StructuralTestCoverageReport:
    """
    Compute v0 (kind + control-flow recall) and v1 (k-gram path recall).

    Args:
        put_roots: One or more UAST roots for the program-under-test.
        test_roots: Zero or more UAST roots for tests (histograms merged).
        k: Length of each kind n-gram for ``path_recall_kgram``.
        include_unknown: Passed through to histograms and DFS sequences.

    Returns:
        Recall scores in ``[0, 1]``. Vacuous PUT (no counted kinds / no CF /
        no k-grams) yields 1.0 for the corresponding component.
    """
    if k < 1:
        msg = f"k must be >= 1, got {k}"
        raise ValueError(msg)

    h_put = merge_uast_kind_histograms(put_roots, include_unknown=include_unknown)
    h_test = merge_uast_kind_histograms(test_roots, include_unknown=include_unknown)
    cf_put = merge_control_flow_profiles(put_roots)
    cf_test = merge_control_flow_profiles(test_roots)

    kind_recall = _multiset_recall(h_put, h_test)
    control_flow_recall = _multiset_recall(cf_put, cf_test)
    composite_v0 = 0.5 * kind_recall + 0.5 * control_flow_recall

    kg_put = merge_kgram_counters(put_roots, k=k, include_unknown=include_unknown)
    kg_test = merge_kgram_counters(test_roots, k=k, include_unknown=include_unknown)
    path_recall_kgram = _kgram_recall(kg_put, kg_test)

    return StructuralTestCoverageReport(
        kind_recall=kind_recall,
        control_flow_recall=control_flow_recall,
        composite_v0=composite_v0,
        path_recall_kgram=path_recall_kgram,
        k=k,
        include_unknown=include_unknown,
        put_kind_nodes=sum(h_put.values()),
        test_kind_nodes=sum(h_test.values()),
        put_cf_nodes=sum(cf_put.values()),
        test_cf_nodes=sum(cf_test.values()),
        put_kgram_mass=sum(kg_put.values()),
        test_kgram_mass=sum(kg_test.values()),
    )


# ---------------------------------------------------------------------------
# v2: Declaration-level bipartite coverage
# ---------------------------------------------------------------------------


def extract_declarations(root: object) -> list[object]:
    """Return all FunctionDecl/MethodDecl UASTNodes via DFS (includes nested)."""
    results: list[object] = []
    stack = [root]
    while stack:
        node = stack.pop()
        if getattr(node, "kind", "") in _DECL_KINDS:
            results.append(node)
        stack.extend(reversed(list(getattr(node, "children", []))))
    return results


def _decl_fingerprint(decl_node: object, *, include_unknown: bool) -> Counter[str]:
    """Kind histogram of declaration subtree with root kind removed.

    Stripping the root kind prevents every PUT/test pair from getting
    a free floor score from the shared FunctionDecl/MethodDecl node.
    """
    hist = Counter(uast_kind_histogram(decl_node, include_unknown=include_unknown))
    root_kind = getattr(decl_node, "kind", "")
    if root_kind in hist:
        hist[root_kind] -= 1
        if hist[root_kind] <= 0:
            del hist[root_kind]
    return hist


def _location_str(node: object) -> str:
    span = getattr(node, "span", None)
    if span is None:
        return "<unknown>"
    file = getattr(span, "file", None) or ""
    line = getattr(span, "start_line", "?")
    return f"{file}:{line}" if file else f"line:{line}"


@dataclass(frozen=True)
class DeclarationCoverageReport:
    """Declaration-level bipartite structural coverage (v2).

    Each FunctionDecl/MethodDecl in the PUT is matched against the test
    suite's declarations via greedy best-match recall. Scores are not
    inflated by adding unrelated test code (not monotone with corpus size).
    """

    mean_declaration_coverage: float
    best_declaration_recall: tuple[float, ...]
    declaration_locations: tuple[str, ...]
    stmt_recall: float
    expr_recall: float
    mean_test_precision: float
    declaration_path_recall_kgram: float
    k: int
    put_declaration_count: int
    test_declaration_count: int
    include_unknown: bool

    @property
    def declaration_coverage_rate(self) -> float:
        """Alias for mean_declaration_coverage (backward compatibility)."""
        return self.mean_declaration_coverage

    @property
    def f2_score(self) -> float:
        """F2 score favoring recall over precision."""
        p = self.mean_test_precision
        r = self.mean_declaration_coverage
        if p + r == 0:
            return 0.0
        return 5 * (p * r) / (4 * p + r)

    @property
    def uncovered_declarations(self) -> list[str]:
        """Locations of PUT declarations with incomplete test coverage."""
        return [
            loc
            for loc, recall in zip(
                self.declaration_locations, self.best_declaration_recall, strict=False
            )
            if recall < 0.999  # Precision-safe 1.0
        ]


def declaration_coverage(
    put_roots: Sequence[object],
    test_roots: Sequence[object],
    *,
    k: int = 3,
    include_unknown: bool = False,
) -> DeclarationCoverageReport:
    """
    Declaration-level bipartite structural coverage (v2).

    For each FunctionDecl/MethodDecl in the PUT, finds the best-matching
    test declaration by multiset recall of body kind histograms. Addresses
    the five weaknesses of v0/v1:

    - Pooled histograms replaced by per-declaration matching (localizable gaps)
    - CF/kind double-counting replaced by disjoint stmt/expr category recall
    - DFS k-grams scoped to declaration subtrees (semantically bounded)
    - Not monotone: unrelated test functions do not inflate PUT coverage
    - Precision signal added: F2 score penalizes bloated test suites

    Args:
        put_roots: One or more UAST roots for the program-under-test.
        test_roots: Zero or more UAST roots for the test suite.
        k: Length of each kind n-gram for ``declaration_path_recall_kgram``.
        include_unknown: Whether to include Unknown kinds.

    Returns:
        ``DeclarationCoverageReport``. Vacuous PUT (no declarations) yields
        1.0 for all recall scores with ``put_declaration_count = 0``.
    """
    if k < 1:
        msg = f"k must be >= 1, got {k}"
        raise ValueError(msg)

    put_decls = [d for r in put_roots for d in extract_declarations(r)]
    test_decls = [d for r in test_roots for d in extract_declarations(r)]

    put_fps = [_decl_fingerprint(d, include_unknown=include_unknown) for d in put_decls]
    test_fps = [
        _decl_fingerprint(d, include_unknown=include_unknown) for d in test_decls
    ]

    best_recall: list[float] = []
    for pf in put_fps:
        best = max((_multiset_recall(pf, tf) for tf in test_fps), default=0.0)
        best_recall.append(best)

    mean_decl_cov = sum(best_recall) / len(best_recall) if put_decls else 1.0

    # Category-stratified recall — disjoint Stmt vs Expr subsets, no double-counting
    h_put = merge_uast_kind_histograms(put_roots, include_unknown=include_unknown)
    h_test = merge_uast_kind_histograms(test_roots, include_unknown=include_unknown)
    stmt_recall = _multiset_recall(
        {kk: v for kk, v in h_put.items() if kk in _STMT_KINDS},
        {kk: v for kk, v in h_test.items() if kk in _STMT_KINDS},
    )
    expr_recall = _multiset_recall(
        {kk: v for kk, v in h_put.items() if kk in _EXPR_KINDS},
        {kk: v for kk, v in h_test.items() if kk in _EXPR_KINDS},
    )

    best_prec: list[float] = []
    for tf in test_fps:
        best_prec.append(max((_multiset_recall(tf, pf) for pf in put_fps), default=0.0))
    mean_test_prec = sum(best_prec) / len(best_prec) if test_decls else 0.0

    put_kg: Counter[tuple[str, ...]] = Counter()
    for d in put_decls:
        put_kg.update(
            _kgrams_from_sequence(
                uast_dfs_kind_sequence(d, include_unknown=include_unknown),
                k,
            )
        )
    test_kg: Counter[tuple[str, ...]] = Counter()
    for d in test_decls:
        test_kg.update(
            _kgrams_from_sequence(
                uast_dfs_kind_sequence(d, include_unknown=include_unknown),
                k,
            )
        )

    return DeclarationCoverageReport(
        mean_declaration_coverage=mean_decl_cov,
        best_declaration_recall=tuple(best_recall),
        declaration_locations=tuple(_location_str(d) for d in put_decls),
        stmt_recall=stmt_recall,
        expr_recall=expr_recall,
        mean_test_precision=mean_test_prec,
        declaration_path_recall_kgram=_kgram_recall(put_kg, test_kg),
        k=k,
        put_declaration_count=len(put_decls),
        test_declaration_count=len(test_decls),
        include_unknown=include_unknown,
    )
