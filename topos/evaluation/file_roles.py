"""
File roles
==========

Predicates that classify what *role* a source file plays, so the
characteristic morphism can relax or skip specific quality gates when a
file is structurally special rather than ordinary logic.

The first role is the **import/export-only entrypoint module** —
``__init__.py``, ``mod.rs``/``lib.rs``, ``index.ts``, and friends — which
are trivial re-export hubs and should not be penalized for low entropy or
high instability.  Additional roles (generated code, vendored code, test
files, …) can be added here as further predicates over the same
:class:`~topos.core.morphism.ProgramMorphism`.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism


def is_entrypoint_module(morphism: ProgramMorphism) -> bool:
    """True iff *morphism* is an import/export-only entrypoint module."""
    if morphism.filepath is None:
        return False
    if not _entrypoint_filename_hint(morphism.filepath, morphism.language):
        return False
    return _is_entrypoint_source_only(morphism.source, morphism.language)


def _entrypoint_filename_hint(path: Path, language: str) -> bool:
    filename = path.name
    lower_name = filename.lower()
    if language == "python":
        return filename == "__init__.py"
    if language == "rust":
        return filename in {"mod.rs", "lib.rs"}
    if language == "typescript":
        return lower_name in {"index.ts", "index.tsx"}
    if language == "javascript":
        return lower_name in {"index.js", "index.mjs", "index.cjs"}
    if language == "cpp":
        return path.suffix.lower() in {".hpp", ".hh", ".hxx"}
    return False


def _is_entrypoint_source_only(source: str, language: str) -> bool:
    lines = [line.strip() for line in source.splitlines()]
    lines = [line for line in lines if line]
    if not lines:
        return False

    if language == "python":
        return _python_entrypoint_only(lines)
    if language in {"typescript", "javascript"}:
        return all(
            line.startswith("import ")
            or line.startswith("export *")
            or line.startswith("export {")
            or line.startswith("export type ")
            or line.startswith("export interface ")
            or line.startswith("//")
            or line.startswith("#!")
            or line.startswith("/*")
            or line.startswith("*")
            or line.endswith("*/")
            for line in lines
        )
    if language == "rust":
        return all(
            line.startswith("use ")
            or line.startswith("pub use ")
            or line.startswith("pub mod ")
            or line.startswith("mod ")
            or line.startswith("extern crate ")
            or line.startswith("#!")
            or line.startswith("#[")
            or line.startswith("//")
            or line.startswith("/*")
            or line.startswith("*")
            or line.endswith("*/")
            for line in lines
        )
    if language == "cpp":
        return all(
            line.startswith("#include")
            or line.startswith("#pragma once")
            or line.startswith("//")
            or line.startswith("/*")
            or line.startswith("*")
            or line.endswith("*/")
            for line in lines
        )
    return False


def _python_entrypoint_only(lines: list[str]) -> bool:
    # Track open brackets so multiline ``from x import (...)`` and
    # ``__all__ = [...]`` continuation lines (e.g. ``assess,``) are accepted.
    depth = 0
    for line in lines:
        if depth > 0:
            depth += _bracket_delta(line)
            continue
        if not (
            line.startswith("#")
            or line.startswith("import ")
            or line.startswith("from ")
            or line.startswith("__all__")
            or line in {"[", "]", "(", ")"}
            or line.startswith(("'", '"'))
        ):
            return False
        if line.startswith(("import ", "from ", "__all__")):
            depth += _bracket_delta(line)
    return True


def _bracket_delta(line: str) -> int:
    opens = line.count("(") + line.count("[")
    closes = line.count(")") + line.count("]")
    return opens - closes
