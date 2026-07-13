"""
Abstractness Representation
----------------------------
Adapts Martin's Abstractness metric (fraction of a module's type
declarations that are abstract) to the :class:`~topos.graphs.base.Representation`
protocol, so it merges into the same ``composable``-dimension metrics dict
as :class:`~topos.graphs.mdg.object.ModuleDependencyGraph`.

Unlike ``ModuleDependencyGraph`` (built from GitNexus graph data),
Abstractness is purely a property of a single file's UAST — no dependency
graph is required, so it is available for any file in a language whose
UAST mapper classifies type declarations as abstract/concrete
(``typeKind``) — see ``mapper_rust.py``, ``mapper_python.py``,
``mapper_go.py``, ``mapper_typescript.py``.

Languages without that classification (JavaScript has no abstract-class/
interface concept at all; C++'s mapper has an independent, pre-existing
bug — see issue #124 discussion) are deliberately excluded here rather
than silently reporting ``0.0`` for every file, which would be
indistinguishable from "this file happens to declare no types."
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.uast.models import UASTNode

# Languages whose UAST mappers populate the `typeKind` attribute needed to
# classify type declarations as abstract vs. concrete.
_ABSTRACTNESS_SUPPORTED_LANGUAGES = frozenset({"python", "rust", "go", "typescript"})


@dataclass
class AbstractnessRepresentation:
    """Representation adapter exposing ``mdg.abstractness``."""

    uast_root: UASTNode

    @property
    def name(self) -> str:
        return "abstractness"

    @property
    def dimension(self) -> str:
        # Merges into the same raw-metrics dict as ModuleDependencyGraph so
        # Φ_COMPOSABLE can pair instability with abstractness (issue #124).
        return "composable"

    def metrics(self) -> dict[str, float]:
        if self.uast_root.lang not in _ABSTRACTNESS_SUPPORTED_LANGUAGES:
            return {}
        from topos.functors.probes.uast.abstractness import calculate_abstractness

        return {"mdg.abstractness": calculate_abstractness(self.uast_root)}
