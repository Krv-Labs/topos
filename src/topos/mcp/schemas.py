"""
Pydantic schemas for the Topos MCP server.

Input models validate tool arguments; return models give FastMCP the
``outputSchema`` it emits to clients per MCP 2025-11-25 structured-output spec.
"""

from __future__ import annotations

from enum import StrEnum

from pydantic import BaseModel, ConfigDict, Field

from topos.logic.policies.base import Priority


class ResponseFormat(StrEnum):
    """Output format for tools that support human/machine presentation."""

    MARKDOWN = "markdown"
    JSON = "json"


class LatticeElement(StrEnum):
    """String-valued mirror of ``EvaluationValue`` for MCP wire format.

    These are the 8 elements of the free Heyting algebra H(G_qual) on the
    three generators SIMPLE, COMPOSABLE, SECURE.  Top = IDEAL, bottom = SLOP.
    """

    SLOP = "SLOP"
    SIMPLE = "SIMPLE"
    COMPOSABLE = "COMPOSABLE"
    SECURE = "SECURE"
    SIMPLE_COMPOSABLE = "SIMPLE_COMPOSABLE"
    SIMPLE_SECURE = "SIMPLE_SECURE"
    COMPOSABLE_SECURE = "COMPOSABLE_SECURE"
    IDEAL = "IDEAL"


class AssessmentStatus(StrEnum):
    """Outcome of comparing a proposed change to the baseline."""

    IMPROVEMENT = "IMPROVEMENT"
    IMPROVEMENT_SCORE = "IMPROVEMENT_SCORE"
    LATERAL_MOVE = "LATERAL_MOVE"
    REGRESSION = "REGRESSION"
    REGRESSION_SCORE = "REGRESSION_SCORE"
    # Anti-gaming flag (CodeScene 2026): scores moved meaningfully while the
    # AST barely changed — suspicious unless the agent explains why.
    SUSPICIOUS_NO_STRUCTURAL_CHANGE = "SUSPICIOUS_NO_STRUCTURAL_CHANGE"


# ---------------------------------------------------------------------------
# Input models
# ---------------------------------------------------------------------------


class _StrictModel(BaseModel):
    model_config = ConfigDict(
        str_strip_whitespace=True,
        validate_assignment=True,
        extra="forbid",
    )


class EvaluateCodeInput(_StrictModel):
    """Arguments for ``topos_evaluate_code``."""

    code: str = Field(
        ...,
        description="Raw source code to evaluate.",
        min_length=1,
    )
    language: str = Field(
        default="python",
        description=(
            "Programming language of the source. Supported via tree-sitter: "
            "'python', 'rust', 'javascript', 'cpp'."
        ),
    )
    priority: Priority = Field(
        default=Priority.BALANCED,
        description=(
            "Optimization priority shifting metric weights within each "
            "dimension: 'balanced', 'composable', or 'self_contained'."
        ),
    )
    response_format: ResponseFormat = Field(
        default=ResponseFormat.MARKDOWN,
        description=(
            "'markdown' for human/agent-readable, 'json' for structured "
            "programmatic use."
        ),
    )


class EvaluateFileInput(_StrictModel):
    """Arguments for ``topos_evaluate_file``."""

    filepath: str = Field(
        ...,
        description="Path to the source file, relative to the project root.",
        min_length=1,
    )
    priority: Priority = Field(default=Priority.BALANCED, description="Priority.")
    gitnexus_dir: str | None = Field(
        default=None,
        description=(
            "Path to a .gitnexus/ directory produced by `topos depgraph "
            "generate`. When provided, enables coupling-dimension scoring so "
            "COMPOSABLE/SOUND become reachable. Defaults to "
            "<project_root>/.gitnexus if it exists."
        ),
    )
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


class EvaluateProjectInput(_StrictModel):
    """Arguments for ``topos_evaluate_project``."""

    path: str = Field(
        ...,
        description=(
            "Directory to recursively evaluate. Must be inside the project root."
        ),
        min_length=1,
    )
    priority: Priority = Field(default=Priority.BALANCED, description="Priority.")
    gitnexus_dir: str | None = Field(
        default=None,
        description="Optional .gitnexus/ directory for per-file coupling scoring.",
    )
    limit: int = Field(
        default=25,
        ge=1,
        le=500,
        description="Maximum per-file entries to include in the response.",
    )
    offset: int = Field(
        default=0,
        ge=0,
        description="Number of per-file entries to skip (pagination).",
    )
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


class CompareCodeInput(_StrictModel):
    """Arguments for ``topos_compare_code``."""

    source_code: str = Field(..., min_length=1, description="Baseline code.")
    target_code: str = Field(..., min_length=1, description="Proposed/target code.")
    language: str = Field(default="python")
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


class CompareFilesInput(_StrictModel):
    """Arguments for ``topos_compare_files``."""

    source: str = Field(..., min_length=1, description="Baseline file path.")
    target: str = Field(..., min_length=1, description="Proposed file path.")
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


class AssessImprovementInput(_StrictModel):
    """Arguments for ``topos_assess_improvement``.

    Provide EITHER ``filepath`` (preferred — enables coupling-dimension scoring
    against the cached DependencyGraph) OR ``current_code`` (legacy, AST-only).
    """

    proposed_code: str = Field(
        ..., min_length=1, description="The refactored / proposed source."
    )
    filepath: str | None = Field(
        default=None,
        description=(
            "Path to the current file on disk. When provided, baseline is "
            "loaded from disk and coupling is scored against the cached "
            "DependencyGraph. STRONGLY PREFERRED for real refactor loops."
        ),
    )
    current_code: str | None = Field(
        default=None,
        description=(
            "Baseline source as a string. Used only when `filepath` is not "
            "given. Coupling will NOT be computed (AST-only)."
        ),
    )
    language: str = Field(default="python")
    priority: Priority = Field(default=Priority.BALANCED)
    gitnexus_dir: str | None = Field(default=None)
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


class InspectCodeInput(_StrictModel):
    """Arguments for ``topos_inspect_code``."""

    code: str = Field(..., min_length=1)
    language: str = Field(default="python")
    priority: Priority = Field(default=Priority.BALANCED)
    top_n_functions: int = Field(
        default=10,
        ge=1,
        le=200,
        description=(
            "Return at most this many functions, sorted by descending "
            "cyclomatic complexity. Keeps agent context lean on large files."
        ),
    )
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


# ---------------------------------------------------------------------------
# Return models (structured output)
# ---------------------------------------------------------------------------


class EvaluationResult(BaseModel):
    """Result of a single-unit evaluation."""

    is_parseable: bool
    lattice_element: LatticeElement
    lattice_symbol: str
    lattice_description: str
    dimensions: dict[str, LatticeElement]
    scores: dict[str, float] = Field(
        ..., description="Per-dimension normalized score in [0, 100]."
    )
    priority: Priority
    guidance: str = Field(..., description="Next-step hint for the agent.")
    coupling_available: bool = Field(
        ...,
        description=(
            "True only when a DependencyGraph was provided. When false, "
            "COMPOSABLE/SOUND are unreachable for this evaluation."
        ),
    )
    raw_metrics: dict[str, float] = Field(default_factory=dict)
    interpretation: dict[str, str] = Field(default_factory=dict)
    error: str | None = None


class ProjectFileEntry(BaseModel):
    filepath: str
    lattice_element: LatticeElement
    scores: dict[str, float]
    raw_metrics: dict[str, float] = Field(default_factory=dict)
    is_parseable: bool = True


class ProjectEvaluationResult(BaseModel):
    """Result of a directory-wide evaluation."""

    root: str
    file_count: int
    parse_failures: int
    rolled_up_dimensions: dict[str, LatticeElement]
    rolled_up_scores: dict[str, float]
    overall: LatticeElement
    priority: Priority
    coupling_available: bool
    count: int = Field(..., description="Entries in this page.")
    offset: int
    total: int
    has_more: bool
    next_offset: int | None
    files: list[ProjectFileEntry]
    error: str | None = None


class ComparisonResult(BaseModel):
    """Result of AST-distance comparison between two programs."""

    raw_distance: float
    normalized_distance: float
    similarity: float
    operations: dict[str, int]
    source_valid: bool
    target_valid: bool
    error: str | None = None


class AssessmentResult(BaseModel):
    """Result of ``topos_assess_improvement``."""

    status: AssessmentStatus
    priority: Priority
    current: EvaluationResult
    proposed: EvaluationResult
    score_deltas: dict[str, float]
    complexity_delta: float
    structural_distance: float | None = None
    similarity: float | None = None
    coupling_available_for_proposed: bool
    # Anti-gaming: populated when scores moved but the tree barely changed.
    suspicion_reason: str | None = None
    error: str | None = None


class InspectionResult(BaseModel):
    """Result of ``topos_inspect_code`` — full breakdown."""

    evaluation: EvaluationResult
    functions: dict[str, int] = Field(
        default_factory=dict,
        description="function_name -> cyclomatic_complexity (top N only).",
    )
    total_functions: int
    entropy_compression_ratio: float | None = None
    entropy_interpretation: str | None = None
    error: str | None = None
