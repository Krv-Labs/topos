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
    from topos.graphs.base import Representation
    from topos.logic.lattice import EvaluationValue


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
    filepath: Path | None = None
    ast: ProgramObject | None = field(default=None, repr=False)
    representations: list[Representation] = field(default_factory=list, repr=False)

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
        from topos.utils.tree_sitter import parse_python

        if self.language != "python":
            raise ValueError(f"Language '{self.language}' not yet supported")

        root = parse_python(self.source)
        return ProgramObject(root=root, source=self.source, language=self.language)

    @classmethod
    def from_file(
        cls, filepath: str | Path, language: str = "python"
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
        return cls(source=source, language=language, filepath=path)

    @property
    def is_valid(self) -> bool:
        """Check if the morphism represents syntactically valid code."""
        return self.ast is not None and self.ast.is_valid

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
        from topos.logic.omega import SubobjectClassifier

        classifier = SubobjectClassifier()
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
