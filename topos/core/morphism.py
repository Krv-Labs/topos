"""
Morphism Module
---------------
In the category of Programs, we treat source code not as static text, but as
a 'Morphism' (f: A → B) between computational states.

Mathematical Inspiration:
    A program is a map that transforms an input domain (Object A) into an
    output codomain (Object B). As code becomes a 'commodity' via LLMs, the
    identity of the morphism matters less than its structural invariants.

    Two morphisms f, g: A → B are considered equivalent if they produce the
    same observable behavior—but in our Topos, we also care about their
    internal structure. A morphism that is 'commodity code' may compute
    correctly but lack the structural integrity of a 'verified' morphism.

    The characteristic map χ: Morphism → Ω assigns each morphism an evaluation
    value in our Heyting Algebra, capturing this nuanced view of correctness.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

from topos.core.object import ProgramObject

if TYPE_CHECKING:
    from topos.core.omega import EvaluationValue
    from topos.graphs.ast.dispatch import AstBackend
    from topos.graphs.base import Representation
    from topos.graphs.cfg.object import ControlFlowGraph
    from topos.graphs.cpg.object import CodePropertyGraph
    from topos.graphs.pdg.object import ProgramDependenceGraph
    from topos.graphs.uast.object import AbstractnessRepresentation


@dataclass
class ProgramMorphism:
    """
    A program viewed as a transformation between computational states.

    The ProgramMorphism is the central abstraction of topos. It encapsulates
    source code along with its parsed AST representation, providing the
    foundation for evaluation by the Subobject Classifier.

    Attributes:
        source: The raw source code as a string.
        language: The programming language (default: 'python').
        filepath: Optional path to the source file.
        ast: The ProgramObject (AST) representation.
        representations: Additional representations (depgraph, etc.)
            attached to this morphism for multi-axis evaluation.

    Categorical Interpretation:
        In category theory, a morphism f: A → B is an arrow between objects.
        Here, the source code IS the morphism -- it defines how to transform
        inputs (domain) into outputs (codomain). The AST captures the
        'internal structure' of this transformation; additional
        representations capture inter-module structure.
    """

    source: str
    language: str = "python"
    parser_backend: AstBackend = "hybrid"
    filepath: Path | None = None
    ast: ProgramObject | None = field(default=None, repr=False)
    representations: list[Representation] = field(default_factory=list, repr=False)
    _cfg: ControlFlowGraph | None = field(default=None, repr=False, compare=False)
    _pdg: ProgramDependenceGraph | None = field(default=None, repr=False, compare=False)
    _cpg: CodePropertyGraph | None = field(default=None, repr=False, compare=False)
    _abstractness: AbstractnessRepresentation | None = field(
        default=None, repr=False, compare=False
    )

    def __post_init__(self) -> None:
        """Parse the source code into an AST if not provided."""
        if self.ast is None:
            self.ast = self._parse()

    def _parse(self) -> ProgramObject:
        """
        Parse source code into a ProgramObject.

        Returns:
            A ProgramObject wrapping the parsed AST.

        Raises:
            ValueError: If the language is not supported.
        """
        from topos.graphs.ast.dispatch import parse_source

        parse_result = parse_source(
            source=self.source,
            language=self.language,
            backend=self.parser_backend,
            file=str(self.filepath) if self.filepath else None,
        )
        return ProgramObject(
            root=parse_result.root,
            source=self.source,
            language=self.language,
            native_ast=parse_result.native_ast,
            uast_root=parse_result.uast_root,
            parser_name=parse_result.provenance.parser,
            parser_version=parse_result.provenance.parser_version,
            native_node_kind=parse_result.provenance.node_kind,
        )

    @classmethod
    def from_file(
        cls,
        filepath: str | Path,
        language: str = "python",
        parser_backend: AstBackend = "hybrid",
    ) -> ProgramMorphism:
        """
        Create a ProgramMorphism from a source file.

        Args:
            filepath: Path to the source file.
            language: Programming language of the source.

        Returns:
            A new ProgramMorphism instance.
        """
        path = Path(filepath)
        source = path.read_text(encoding="utf-8")
        return cls(
            source=source,
            language=language,
            parser_backend=parser_backend,
            filepath=path,
        )

    @property
    def is_valid(self) -> bool:
        """Check if the morphism represents syntactically valid code."""
        return self.ast is not None and self.ast.is_valid

    # ------------------------------------------------------------------
    # Translational-functor factory methods
    # ------------------------------------------------------------------
    # Each method builds (and caches) one of the structural representations
    # R: Lang → E required by the math spec.  All three are derived from
    # the UAST built during parsing.

    def build_cfg(self) -> ControlFlowGraph | None:
        """Build (and cache) the Control Flow Graph representation."""
        if self._cfg is not None:
            return self._cfg
        if self.ast is None or self.ast.uast_root is None:
            return None
        from topos.graphs.cfg.object import ControlFlowGraph

        self._cfg = ControlFlowGraph.from_uast(self.ast.uast_root)
        return self._cfg

    def build_pdg(self) -> ProgramDependenceGraph | None:
        """Build (and cache) the academic Program Dependence Graph."""
        if self._pdg is not None:
            return self._pdg
        if self.ast is None or self.ast.uast_root is None:
            return None
        from topos.graphs.pdg.object import ProgramDependenceGraph

        self._pdg = ProgramDependenceGraph.from_uast(self.ast.uast_root)
        return self._pdg

    def build_cpg(self) -> CodePropertyGraph | None:
        """Build (and cache) the Code Property Graph."""
        if self._cpg is not None:
            return self._cpg
        if self.ast is None or self.ast.uast_root is None:
            return None
        from topos.graphs.cpg.object import CodePropertyGraph

        self._cpg = CodePropertyGraph.from_uast(self.ast.uast_root, source=self.source)
        return self._cpg

    def build_abstractness(self) -> AbstractnessRepresentation | None:
        """Build (and cache) the Abstractness representation."""
        if self._abstractness is not None:
            return self._abstractness
        if self.ast is None or self.ast.uast_root is None:
            return None
        from topos.graphs.uast.object import AbstractnessRepresentation

        self._abstractness = AbstractnessRepresentation(uast_root=self.ast.uast_root)
        return self._abstractness

    @property
    def name(self) -> str:
        """A human-readable identifier for this morphism."""
        if self.filepath:
            return self.filepath.name
        return f"<morphism:{hash(self.source) % 10000:04d}>"

    def classify(self) -> EvaluationValue:
        """
        Evaluate this morphism using the Subobject Classifier.

        If additional representations are attached they will be included
        in the evaluation.

        Returns:
            An EvaluationValue from the Heyting Algebra representing the
            code's position in the evaluation lattice.
        """
        from topos.evaluation.characteristic_morphism import CharacteristicMorphism

        classifier = CharacteristicMorphism()
        result = classifier.classify_detailed(
            self,
            representations=self.representations or None,
        )
        return result.summary()

    def __hash__(self) -> int:
        """Hash based on source content."""
        return hash((self.source, self.language))

    def __eq__(self, other: object) -> bool:
        """Equality based on source and language."""
        if not isinstance(other, ProgramMorphism):
            return NotImplemented
        return self.source == other.source and self.language == other.language
