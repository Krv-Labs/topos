"""CPG danger probes — thin wrappers over the Rust engine."""

from __future__ import annotations

from topos.topos_functors import (
    callee_from_text_py as _callee_from_text,
)
from topos.topos_functors import (
    dangerous_api_reachable_py as dangerous_api_reachable,
)
from topos.topos_functors import (
    effective_registry_py as effective_registry,
)
from topos.topos_functors import (
    match_registry_key_py as match_registry_key,
)
from topos.topos_functors import (
    matches_registry_py as _matches_registry,
)
from topos.topos_functors import (
    matches_registry_py as matches_registry,
)

_LANGUAGES = ("python", "javascript", "typescript", "rust", "cpp", "go")

DANGEROUS_APIS: dict[str, set[str]] = {
    lang: effective_registry(lang, None) for lang in _LANGUAGES
}

__all__ = [
    "DANGEROUS_APIS",
    "dangerous_api_reachable",
    "effective_registry",
    "match_registry_key",
    "matches_registry",
    "_callee_from_text",
    "_matches_registry",
]
