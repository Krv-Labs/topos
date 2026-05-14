"""Package-level noise transforms for the Composable axis.

Each transform mutates the package directory in place. The caller is
responsible for staging a fresh copy under an isolated git root before
applying noise; the transforms here do not enforce that.

"I/O preservation" for these transforms means parseability of every
``.py`` file plus the absence of import-time syntax errors — the
benchmark consumes the *dependency graph*, not runtime behavior of the
package. Each transform exclusively adds new files or appends new lines;
existing source bodies are never rewritten.

Expected effect on the coupling metrics (per
``topos.metrics.mdg.coupling``):

============================  =========================================
Transform                      Δ behavior per unit intensity
============================  =========================================
add_spurious_imports           +1 Ce on every existing module;
                               +N Ca on each placeholder (N=#modules).
add_indirection_facades        +1 Ca on a target module; +1 Ce on the
                               new facade.
split_largest_module           +1 Ce on the largest module; +1 Ca on
                               the new shard module.
add_dependency_chain           Creates an N-deep import chain; raises
                               dep_depth on the chain head.
============================  =========================================
"""

from __future__ import annotations

from pathlib import Path

__all__ = [
    "add_spurious_imports",
    "add_indirection_facades",
    "split_largest_module",
    "add_dependency_chain",
    "TRANSFORMS",
]


def _python_files(pkg_dir: Path) -> list[Path]:
    """Top-level ``.py`` files of a package (no submodule traversal).

    Tests subdirectories are skipped because they bloat the graph without
    being part of the "library surface" we want to measure.
    """
    return sorted(p for p in pkg_dir.glob("*.py") if not p.name.startswith("_topos_"))


def _largest_module(pkg_dir: Path) -> Path | None:
    candidates = _python_files(pkg_dir)
    if not candidates:
        return None
    return max(candidates, key=lambda p: p.stat().st_size)


def _append(path: Path, lines: list[str]) -> None:
    with path.open("a", encoding="utf-8") as f:
        f.write("\n\n# --- topos sensitivity noise ---\n")
        for line in lines:
            f.write(line + "\n")


def add_spurious_imports(pkg_dir: Path, intensity: int) -> None:
    """Add ``intensity`` placeholder modules, imported by every existing module.

    Each existing module gains ``intensity`` outgoing IMPORTS edges; each
    placeholder gains afferent edges from every existing module.
    """
    if intensity <= 0:
        return
    existing = _python_files(pkg_dir)
    for i in range(intensity):
        placeholder = pkg_dir / f"_topos_dep_{i}.py"
        placeholder.write_text(
            f'"""Topos sensitivity placeholder #{i}."""\n_TOPOS_DEP_{i} = {i}\n',
            encoding="utf-8",
        )

    imports = [f"from . import _topos_dep_{i}" for i in range(intensity)]
    for module in existing:
        _append(module, imports)


def add_indirection_facades(pkg_dir: Path, intensity: int) -> None:
    """Create ``intensity`` facade modules that re-export existing modules.

    Round-robins across the existing modules so afferent coupling rises
    fairly evenly across the package.
    """
    if intensity <= 0:
        return
    existing = _python_files(pkg_dir)
    if not existing:
        return

    for i in range(intensity):
        target = existing[i % len(existing)]
        target_stem = target.stem
        facade = pkg_dir / f"_topos_facade_{i}.py"
        facade.write_text(
            f'"""Topos sensitivity facade #{i} for ``{target_stem}``."""\n'
            f"from . import {target_stem} as _facade_target\n"
            f"__all__ = ['_facade_target']\n",
            encoding="utf-8",
        )


def split_largest_module(pkg_dir: Path, intensity: int) -> None:
    """Append ``intensity`` shard modules and have the largest module import them.

    Mimics the effect of partially splitting a fat module — the original
    becomes a re-exporter of fresh shards. Internal complexity of the
    original is untouched; only its fan-out grows.
    """
    if intensity <= 0:
        return
    target = _largest_module(pkg_dir)
    if target is None:
        return

    shard_names: list[str] = []
    for i in range(intensity):
        shard_stem = f"{target.stem}_topos_shard_{i}"
        shard_path = pkg_dir / f"{shard_stem}.py"
        shard_path.write_text(
            f'"""Topos sensitivity shard #{i} of ``{target.stem}``."""\n'
            f"def _shard_{i}() -> int:\n"
            f"    return {i}\n",
            encoding="utf-8",
        )
        shard_names.append(shard_stem)

    _append(target, [f"from . import {name}" for name in shard_names])


def add_dependency_chain(pkg_dir: Path, intensity: int) -> None:
    """Create a linear N-deep import chain of fresh modules.

    Each ``_topos_chain_{i+1}`` imports ``_topos_chain_{i}``. The chain
    head accumulates afferent depth proportional to ``intensity``.
    """
    if intensity <= 0:
        return
    head = pkg_dir / "_topos_chain_0.py"
    head.write_text(
        '"""Topos sensitivity chain head."""\n_CHAIN_HEAD = 0\n',
        encoding="utf-8",
    )
    for i in range(1, intensity + 1):
        link = pkg_dir / f"_topos_chain_{i}.py"
        link.write_text(
            f'"""Topos sensitivity chain link #{i}."""\n'
            f"from . import _topos_chain_{i - 1}\n"
            f"_CHAIN_LINK_{i} = {i}\n",
            encoding="utf-8",
        )


TRANSFORMS: dict[str, callable] = {
    "add_spurious_imports": add_spurious_imports,
    "add_indirection_facades": add_indirection_facades,
    "split_largest_module": split_largest_module,
    "add_dependency_chain": add_dependency_chain,
}
