"""Print PyInstaller ``--hidden-import`` module paths from ``topos._LAZY_EXPORTS``."""

from __future__ import annotations

import importlib

init = importlib.import_module("topos")
lazy_exports: dict[str, tuple[str, str]] = init._LAZY_EXPORTS  # noqa: SLF001

seen: set[str] = set()
for module_name, _ in lazy_exports.values():
    if module_name not in seen:
        seen.add(module_name)
        print(module_name)
