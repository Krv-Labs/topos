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
``mapper_go.py``, ``mapper_typescript.py``, and ``mapper_cpp.py``.

One language is deliberately excluded from
``_ABSTRACTNESS_SUPPORTED_LANGUAGES`` below rather than silently reporting
``0.0`` for every file, which would be indistinguishable from "this file
happens to declare no types":

- **JavaScript** — not a mapper gap, a language fact. Plain JS has no
  ``interface``/``abstract class`` syntax at all (that's a TypeScript-only
  extension to the shared grammar); there is nothing in a ``.js`` file's
  grammar to ever classify as abstract, so Abstractness is not a
  meaningful metric for it, full stop. Nothing to fix here.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.uast.models import UASTNode

# Languages whose UAST mappers populate the `typeKind` attribute needed to
# classify type declarations as abstract vs. concrete. JavaScript is
# permanently absent (no abstract-type concept in the language — see
# module docstring).
_ABSTRACTNESS_SUPPORTED_LANGUAGES = frozenset(
    {"python", "rust", "go", "typescript", "cpp"}
)


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
