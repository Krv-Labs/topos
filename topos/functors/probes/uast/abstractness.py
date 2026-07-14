"""
UAST Abstractness Probe
------------------------
Martin's Abstractness (A): the fraction of a module's type declarations
that are abstract (trait / interface / protocol / abstract class) rather
than concrete (struct / class / enum / union / type alias).

Paired with Instability (``mdg.instability``), this drives the Distance
from the Main Sequence gate (``mdg.main_sequence_distance``,
:mod:`topos.evaluation.policies.composable`) instead of gating raw
instability against a fixed band — see issue #124.
"""

from __future__ import annotations

from collections.abc import Iterable

_ABSTRACT_TYPE_KINDS = frozenset({"trait", "interface", "abstractClass", "protocol"})
_CONCRETE_TYPE_KINDS = frozenset({"class", "struct", "enum", "union", "typeAlias"})


def _walk(root) -> Iterable[object]:
    yield root
    for child in getattr(root, "children", []):
        yield from _walk(child)


def calculate_abstractness(root) -> float:
    """
    Fraction of classifiable ``TypeDecl`` nodes in *root* that are abstract.

    Args:
        root: A UAST root node (duck-typed: exposes ``kind``, ``children``,
            ``attributes``).

    Returns:
        ``abstract_count / (abstract_count + concrete_count)``, or ``0.0``
        when the module declares zero classifiable type declarations. This
        is a real, meaningful value — a functions-only module (e.g. a
        typical ``main.rs`` orchestrator with no struct/trait/enum) is
        legitimately 0% abstract, not "undefined." Whether Abstractness is
        *applicable at all* for a given file is a language-support
        question, decided by the caller
        (:class:`~topos.graphs.uast.object.AbstractnessRepresentation`),
        not by this function. ``TypeDecl`` nodes with no recognized
        ``typeKind`` attribute (e.g. Rust ``impl_item`` blocks, Go type
        aliases to primitives) are excluded from both the numerator and
        the denominator.
    """
    abstract_count = 0
    concrete_count = 0
    for node in _walk(root):
        if getattr(node, "kind", None) != "TypeDecl":
            continue
        type_kind = getattr(node, "attributes", {}).get("typeKind")
        if type_kind in _ABSTRACT_TYPE_KINDS:
            abstract_count += 1
        elif type_kind in _CONCRETE_TYPE_KINDS:
            concrete_count += 1

    total = abstract_count + concrete_count
    if total == 0:
        return 0.0
    return abstract_count / total
