"""Lightweight language constants for CLI option wiring (no parser backends)."""

from __future__ import annotations

SUPPORTED_LANGUAGES = frozenset({"python", "rust", "javascript", "typescript", "cpp"})

LANGUAGE_FILE_SUFFIXES: dict[str, tuple[str, ...]] = {
    "python": (".py",),
    "rust": (".rs",),
    "javascript": (".js", ".mjs", ".cjs"),
    "typescript": (".ts", ".tsx"),
    "cpp": (".cpp", ".cc", ".cxx", ".hpp", ".hh", ".hxx"),
}


def language_file_suffixes(language: str) -> tuple[str, ...]:
    """Return filename suffixes associated with *language* for source discovery."""
    if language not in SUPPORTED_LANGUAGES:
        raise ValueError(f"Language '{language}' not supported")
    return LANGUAGE_FILE_SUFFIXES[language]
