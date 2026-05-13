from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from topos.graphs.ast.providers.native_provider import NativeAstProvider
from topos.graphs.ast.providers.tree_sitter_provider import TreeSitterProvider
from topos.graphs.ast.types import ParseResult

AstBackend = Literal["tree-sitter", "native", "hybrid"]

SUPPORTED_LANGUAGES = frozenset({"python", "rust", "javascript", "typescript", "cpp"})

# Suffixes used by ``topos evaluate`` when collecting files from paths.
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


@dataclass
class ParserDispatch:
    tree_sitter_provider: TreeSitterProvider
    native_provider: NativeAstProvider

    @classmethod
    def default(cls) -> ParserDispatch:
        return cls(
            tree_sitter_provider=TreeSitterProvider(),
            native_provider=NativeAstProvider(),
        )

    def parse(
        self,
        source: str,
        language: str,
        backend: AstBackend = "hybrid",
        file: str | None = None,
    ) -> ParseResult:
        if language not in SUPPORTED_LANGUAGES:
            raise ValueError(f"Language '{language}' not supported")

        if backend == "tree-sitter":
            return self.tree_sitter_provider.parse(source, language=language, file=file)
        if backend in {"native", "hybrid"}:
            return self.native_provider.parse(source, language=language, file=file)
        raise ValueError(f"Unknown backend '{backend}'")

    def capability_matrix(self) -> dict[str, dict[str, bool]]:
        matrix: dict[str, dict[str, bool]] = {}
        for language in sorted(SUPPORTED_LANGUAGES):
            matrix[language] = {
                "supports_tree_sitter": self.tree_sitter_provider.supports(language),
                "supports_native": self.native_provider.supports(language),
                "supports_uast": self.tree_sitter_provider.supports(language),
            }
        return matrix


_DEFAULT_DISPATCH: ParserDispatch | None = None


def get_dispatch() -> ParserDispatch:
    global _DEFAULT_DISPATCH
    if _DEFAULT_DISPATCH is None:
        _DEFAULT_DISPATCH = ParserDispatch.default()
    return _DEFAULT_DISPATCH


def reset_dispatch() -> None:
    """Reset the module-level singleton. Intended for tests that need a clean dispatch."""
    global _DEFAULT_DISPATCH
    _DEFAULT_DISPATCH = None


def parse_source(
    source: str,
    language: str,
    backend: AstBackend = "hybrid",
    file: str | None = None,
) -> ParseResult:
    return get_dispatch().parse(
        source=source,
        language=language,
        backend=backend,
        file=file,
    )


def get_capability_matrix() -> dict[str, dict[str, bool]]:
    return get_dispatch().capability_matrix()
