"""
Academic Program Dependence Graph (Ferrante/Ottenstein style).
==============================================================

An intra-procedural Program Dependence Graph composes two edge families
over the procedure's statement nodes:

    Data Dependence Graph (DDG):
        u -DDG-> v   iff   u defines a variable later read by v
                            (no intervening redefinition).

    Control Dependence Graph (CDG):
        u -CDG-> v   iff   v executes only when u takes a particular
                            branch (computed from the CFG post-dominator
                            tree).

The fused graph is what slicing, taint, and program-aware diff use.  We
expose it through the ``Representation`` protocol so the rest of Topos can
treat it uniformly.  This v1 PDG is intentionally **not** a generator
source for the lattice (it has no ``Φ`` of its own); it is consumed by the
CPG builder.  Its ``dimension`` is therefore set to a neutral
``"composable"`` value so the dispatcher ignores it when there are no
dedicated PDG metrics — but the metrics it emits are still surfaced in
``ClassificationResult.raw_metrics`` for diagnostics.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from topos.graphs.cfg.object import ControlFlowGraph
from topos.graphs.uast.models import UASTNode


class DependenceKind(StrEnum):
    DATA = "data"
    CONTROL = "control"


@dataclass(frozen=True)
class DependenceEdge:
    """A typed dependence edge between two UAST nodes by stable id."""

    source: str
    target: str
    kind: DependenceKind
    var: str = ""  # for DATA edges: the variable name carrying the dependence


@dataclass
class ProgramDependenceGraph:
    """
    Intra-procedural Program Dependence Graph.

    Attributes:
        statements: All UAST statement nodes contributing to dependence.
        edges:      Typed dependence edges (DATA / CONTROL).
        cfg:        The CFG from which control dependence was derived.
    """

    statements: list[UASTNode] = field(default_factory=list)
    edges: list[DependenceEdge] = field(default_factory=list)
    cfg: ControlFlowGraph | None = None

    @property
    def name(self) -> str:
        return "pdg"

    @property
    def dimension(self) -> str:
        # The PDG does not own a generator; it surfaces diagnostic
        # metrics under the COMPOSABLE generator (which already covers
        # inter-statement structure for module-level dep graphs).
        return "composable"

    @classmethod
    def from_uast(cls, uast_root: UASTNode, source: str = "") -> ProgramDependenceGraph:
        """Construct DDG ∪ CDG using a freshly-built CFG.

        ``source`` is optional and defaults to ``""`` for backward
        compatibility; when supplied it lets data-dependence recover real
        identifier text (see ``_identifier_name``) instead of falling back
        to each occurrence's own node id.
        """
        cfg = ControlFlowGraph.from_uast(uast_root)
        statements: list[UASTNode] = []
        for block in cfg.blocks.values():
            statements.extend(block.statements)

        edges: list[DependenceEdge] = []
        edges.extend(_compute_data_dependence(statements, source))
        edges.extend(_compute_control_dependence(cfg))

        return cls(statements=statements, edges=edges, cfg=cfg)

    # ------------------------------------------------------------------
    # Diagnostics
    # ------------------------------------------------------------------

    def metrics(self) -> dict[str, float]:
        data = sum(1 for e in self.edges if e.kind is DependenceKind.DATA)
        control = sum(1 for e in self.edges if e.kind is DependenceKind.CONTROL)
        n = max(1, len(self.statements))
        return {
            "pdg.data_deps": float(data),
            "pdg.control_deps": float(control),
            "pdg.density": float((data + control) / n),
        }


# ---------------------------------------------------------------------------
# Internals
# ---------------------------------------------------------------------------


def _compute_data_dependence(
    statements: list[UASTNode],
    source: str = "",
) -> list[DependenceEdge]:
    """
    Approximate reaching-definitions data dependence.

    Walks the statement list in textual order; for each statement records
    the variables it defines (any ``Identifier`` child of an ``AssignExpr``
    in the left-hand side position) and the variables it uses (any other
    ``Identifier`` descendant).  A dependence edge ``u -DDG-> v[var]`` is
    emitted when the most recent definer of ``var`` is ``u``.

    This is intentionally coarse — no alias analysis, no SSA, no flow
    sensitivity.  Sufficient for the security probes in v1.
    """
    edges: list[DependenceEdge] = []
    last_def: dict[str, str] = {}

    for stmt in statements:
        defs, uses = _defs_and_uses(stmt, source)
        for var in uses:
            if var in last_def and last_def[var] != stmt.id:
                edges.append(
                    DependenceEdge(
                        source=last_def[var],
                        target=stmt.id or "<anon>",
                        kind=DependenceKind.DATA,
                        var=var,
                    )
                )
        for var in defs:
            last_def[var] = stmt.id or "<anon>"

    return edges


def _compute_control_dependence(cfg: ControlFlowGraph) -> list[DependenceEdge]:
    """
    Control dependence: every statement in a TRUE/FALSE/SWITCH_CASE
    successor block is control-dependent on the predicate statement in
    the source block.

    This is a structural shortcut around the canonical post-dominator
    algorithm.  Good enough for the v1 CPG; refine later if needed.
    """
    from topos.graphs.cfg.models import EdgeKind

    branching = {EdgeKind.TRUE, EdgeKind.FALSE, EdgeKind.SWITCH_CASE}
    edges: list[DependenceEdge] = []

    for edge in cfg.edges:
        if edge.kind not in branching:
            continue
        predicate_block = cfg.blocks.get(edge.source)
        successor_block = cfg.blocks.get(edge.target)
        if predicate_block is None or successor_block is None:
            continue
        if not predicate_block.statements:
            continue
        predicate_id = predicate_block.statements[-1].id or "<anon>"
        for dep_stmt in successor_block.statements:
            target = dep_stmt.id or "<anon>"
            if target == predicate_id:
                continue
            edges.append(
                DependenceEdge(
                    source=predicate_id,
                    target=target,
                    kind=DependenceKind.CONTROL,
                )
            )
    return edges


def _defs_and_uses(stmt: UASTNode, source: str = "") -> tuple[set[str], set[str]]:
    """Return ``(defs, uses)`` — variable names defined / used by ``stmt``."""
    defs: set[str] = set()
    uses: set[str] = set()

    def walk(node: UASTNode, in_lhs: bool) -> None:
        if node.kind == "AssignExpr":
            children = node.children
            if children:
                walk(children[0], True)
                for c in children[1:]:
                    walk(c, False)
            return
        if node.kind == "Identifier":
            name = _identifier_name(node, source)
            if name:
                (defs if in_lhs else uses).add(name)
            return
        for child in node.children:
            walk(child, in_lhs)

    walk(stmt, False)
    return defs, uses


def _identifier_name(node: UASTNode, source: str = "") -> str:
    """Best-effort recovery of an identifier's textual name.

    The UAST mappers don't carry token text as an attribute, so when
    ``source`` is available we slice the node's own byte span to recover
    the real variable name (e.g. ``"x"``) — this is what lets two distinct
    occurrences of the same variable be recognized as the same dependence
    key. Without ``source`` we fall back to the node's own id, which is
    unique per span; that keeps identifiers at distinct spans from being
    spuriously conflated, at the cost of never matching a reused variable
    across statements.
    """
    name_attr = node.attributes.get("name")
    if isinstance(name_attr, str) and name_attr:
        return name_attr
    if source:
        text = _node_text(node, source)
        if text:
            return text
    return node.id or ""


def _node_text(node: UASTNode, source: str) -> str:
    """Slice ``source`` by ``node``'s byte span (best-effort)."""
    span = node.span
    encoded = source.encode("utf-8")
    if span.start_byte < 0 or span.end_byte > len(encoded):
        return ""
    return (
        encoded[span.start_byte : span.end_byte]
        .decode("utf-8", errors="replace")
        .strip()
    )
