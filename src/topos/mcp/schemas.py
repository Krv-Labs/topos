"""
Pydantic schemas for the Topos MCP server.

Input models validate tool arguments; return models give FastMCP the
``outputSchema`` it emits to clients per MCP 2025-11-25 structured-output spec.
"""

from __future__ import annotations

from enum import StrEnum

from pydantic import BaseModel, ConfigDict, Field

from topos.evaluation.policies.base import Priority
from topos.evaluation.preferences import Generator, UserPreferences


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


class UserPreferencesInput(_StrictModel):
    """A strict total order on the three quality generators.

    Stronger than ``priority`` (which only upweights one generator):
    this is a full ranking that induces a total order on the 8-element
    lattice Ω.  Agents use the induced order to pick a *targeted
    relaxation walk* — by convention IDEAL is treated as infeasible
    and the default target becomes the meet of the top-two ranked
    generators (the "ideal intersection", e.g. ``SIMPLE_SECURE``).

    Example:
        ``ranking=["secure", "simple", "composable"]`` ⟹ default
        target ``SIMPLE_SECURE``; walk descends through it down
        toward ``SLOP``.
    """

    ranking: list[Generator] = Field(
        ...,
        description=(
            "Permutation of {simple, composable, secure}, most-preferred "
            "first.  Length must be exactly 3."
        ),
        min_length=3,
        max_length=3,
    )
    target: LatticeElement | None = Field(
        default=None,
        description=(
            "Optional explicit target verdict.  Defaults to the meet of "
            "the top-two ranked generators (the 'ideal intersection'). "
            "Pass IDEAL only if you really want to aim there — it is "
            "treated as infeasible by convention."
        ),
    )

    def to_preferences(self) -> UserPreferences:
        """Convert into the domain-layer ``UserPreferences``."""
        from .formatting import str_to_lattice

        target_value = str_to_lattice(self.target) if self.target is not None else None
        return UserPreferences.from_iterable(self.ranking, target=target_value)


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
        default=Priority.SECURE,
        description=(
            "Optimization priority — the top-ranked generator.  Shifts "
            "metric weights within each policy translator Φᵢ.  One of "
            "'simple', 'composable', or 'secure'.  For full strict "
            "orderings, pass ``preferences.ranking`` instead."
        ),
    )
    response_format: ResponseFormat = Field(
        default=ResponseFormat.MARKDOWN,
        description=(
            "'markdown' for human/agent-readable, 'json' for structured "
            "programmatic use."
        ),
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Optional strict total order on the three generators. When "
            "provided, the result includes a targeted relaxation walk "
            "toward the 'ideal intersection' (meet of the top-two "
            "ranked generators)."
        ),
    )


class EvaluateFileInput(_StrictModel):
    """Arguments for ``topos_evaluate_file``."""

    filepath: str = Field(
        ...,
        description="Path to the source file, relative to the project root.",
        min_length=1,
    )
    priority: Priority = Field(default=Priority.SECURE, description="Priority.")
    gitnexus_dir: str | None = Field(
        default=None,
        description=(
            "Path to a .gitnexus/ directory produced by `topos depgraph "
            "generate`. When provided, the ModuleDependencyGraph is "
            "attached so the COMPOSABLE generator can be scored. "
            "Defaults to <project_root>/.gitnexus if it exists."
        ),
    )
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Optional strict total order on the three generators; see "
            "``topos://docs/preferences``."
        ),
    )


class EvaluateProjectInput(_StrictModel):
    """Arguments for ``topos_evaluate_project``."""

    path: str = Field(
        ...,
        description=(
            "Directory to recursively evaluate. Must be inside the project root."
        ),
        min_length=1,
    )
    priority: Priority = Field(default=Priority.SECURE, description="Priority.")
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Optional strict total order on the three generators; see "
            "``topos://docs/preferences``."
        ),
    )
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

    Provide EITHER ``filepath`` (preferred — scores the COMPOSABLE
    generator against the cached ModuleDependencyGraph) OR
    ``current_code`` (AST + CFG + CPG only; COMPOSABLE unreachable).
    """

    proposed_code: str = Field(
        ..., min_length=1, description="The refactored / proposed source."
    )
    filepath: str | None = Field(
        default=None,
        description=(
            "Path to the current file on disk. When provided, baseline is "
            "loaded from disk and coupling is scored against the cached "
            "ModuleDependencyGraph. STRONGLY PREFERRED for real refactor loops."
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
    priority: Priority = Field(default=Priority.SECURE)
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Optional strict total order on the three generators; see "
            "``topos://docs/preferences``."
        ),
    )
    gitnexus_dir: str | None = Field(default=None)
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


class InspectCodeInput(_StrictModel):
    """Arguments for ``topos_inspect_code``."""

    code: str = Field(..., min_length=1)
    language: str = Field(default="python")
    priority: Priority = Field(default=Priority.SECURE)
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


class PreferenceWalkInput(_StrictModel):
    """Arguments for ``topos_preference_walk``.

    Convert a strict total order on the three generators into a
    concrete relaxation walk on Ω — the sequence of verdicts an agent
    should aim for, in descending order of preference.
    """

    ranking: list[Generator] = Field(
        ...,
        description=(
            "Permutation of {simple, composable, secure}, most-preferred "
            "first.  Required — there is no 'balanced' fallback."
        ),
        min_length=3,
        max_length=3,
    )
    current: LatticeElement | None = Field(
        default=None,
        description=(
            "Optional current verdict (e.g. from a previous "
            "``topos_evaluate_file`` call).  When provided, the walk is "
            "truncated to entries strictly above ``current`` in the "
            "preference order, and ``next_step`` is the smallest "
            "improvement to aim for next.  Defaults to no truncation."
        ),
    )
    target: LatticeElement | None = Field(
        default=None,
        description=(
            "Optional override of the aspirational target.  Defaults to "
            "``IDEAL``; callers who know IDEAL is unreachable can pin "
            "the target lower."
        ),
    )
    response_format: ResponseFormat = Field(default=ResponseFormat.MARKDOWN)


# ---------------------------------------------------------------------------
# Return models (structured output)
# ---------------------------------------------------------------------------


class WalkStep(BaseModel):
    """One verdict on the relaxation walk.

    Annotated with the satisfied-generator set so an agent can see at
    a glance *what changing to this verdict requires* — e.g. "this
    step adds COMPOSABLE" or "this step drops SECURE".
    """

    verdict: LatticeElement = Field(..., description="The Ω element for this step.")
    preference_score: int = Field(
        ..., description="Lex-preference score (higher = more preferred)."
    )
    generators_satisfied: list[Generator] = Field(
        default_factory=list,
        description="Generators the verdict satisfies (bit-decoded from the verdict).",
    )


class PreferenceWalkResult(BaseModel):
    """Result of ``topos_preference_walk`` — the agent's concrete walk.

    The walk lets an agent plan a refactor without re-running an
    evaluation: it converts the ranking into an explicit list of
    "aim-for" goals, each labelled with what generators it commits the
    code to.
    """

    ranking: list[Generator]
    aspirational_target: LatticeElement = Field(
        ...,
        description="What the agent should aim for first (default ``IDEAL``).",
    )
    fallback_target: LatticeElement = Field(
        ...,
        description=(
            "Where to divert if the aspirational target plateaus — the "
            "meet of the top-two ranked generators."
        ),
    )
    current: LatticeElement | None = Field(
        default=None,
        description="The verdict the walk was computed against, if any.",
    )
    next_step: LatticeElement | None = Field(
        default=None,
        description=(
            "Smallest improvement above ``current``.  ``None`` when "
            "already at or beyond the aspirational target."
        ),
    )
    progress: float = Field(
        ...,
        ge=0.0,
        le=1.0,
        description="Fractional progress from SLOP to the aspirational target.",
    )
    walk: list[WalkStep] = Field(
        default_factory=list,
        description=(
            "Descending preference-ordered walk from the aspirational "
            "target down to (but not including) ``current``.  Empty "
            "when ``current`` is at or beyond target."
        ),
    )
    induced_order: list[WalkStep] = Field(
        default_factory=list,
        description=(
            "All 8 Ω elements ranked by descending preference.  Useful "
            "for clients that want to render the full lattice in "
            "user-preferred order rather than just the walk."
        ),
    )
    error: str | None = None


class PreferenceWalk(BaseModel):
    """Targeted relaxation walk derived from a user preference ranking.

    Two-stage strategy:

    1. **Aim for IDEAL** (``target``) — try to beat the policy
       thresholds for all three generators.
    2. **Divert to the "ideal intersection"** (``fallback_target``)
       when IDEAL plateaus — the meet of the top-two ranked
       generators per the preference ordering.

    Beyond the fallback the walk continues down through atoms toward
    ``SLOP``, in descending preference order.
    """

    ranking: list[Generator] = Field(
        ..., description="The preference ranking, most-preferred first."
    )
    target: LatticeElement = Field(
        ...,
        description=(
            "Aspirational target.  Defaults to ``IDEAL`` — try beating "
            "all three thresholds first."
        ),
    )
    fallback_target: LatticeElement = Field(
        ...,
        description=(
            "Pragmatic divert-point when IDEAL plateaus — the 'ideal "
            "intersection', i.e. the meet of the top-two ranked "
            "generators.  For ranking [composable, secure, simple] "
            "this is ``COMPOSABLE_SECURE``."
        ),
    )
    walk: list[LatticeElement] = Field(
        default_factory=list,
        description=(
            "Descending sequence of verdicts the agent should aim for, "
            "from the aspirational target (``IDEAL`` by default) down "
            "to just above the current verdict.  The **second** "
            "element is ``fallback_target`` — the natural divert-point "
            "when IDEAL stalls.  Empty when at or beyond target."
        ),
    )
    next_step: LatticeElement | None = Field(
        default=None,
        description=(
            "The immediate next achievable verdict above the current "
            "one — the smallest improvement that still respects the "
            "preference order.  ``None`` when at or beyond target."
        ),
    )
    progress: float = Field(
        ...,
        ge=0.0,
        le=1.0,
        description=(
            "Fractional progress from SLOP to the aspirational target, in [0.0, 1.0]."
        ),
    )


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
            "True only when a ModuleDependencyGraph was provided. When false, "
            "any verdict containing COMPOSABLE (including IDEAL) is "
            "unreachable for this evaluation."
        ),
    )
    raw_metrics: dict[str, float] = Field(default_factory=dict)
    interpretation: dict[str, str] = Field(default_factory=dict)
    preference_walk: PreferenceWalk | None = Field(
        default=None,
        description=(
            "Present only when the caller supplied ``preferences``.  "
            "Encodes the targeted relaxation walk toward the ideal "
            "intersection."
        ),
    )
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
