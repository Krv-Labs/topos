"""
Pydantic schemas for the Topos MCP server.

Input models validate tool arguments; return models give FastMCP the
``outputSchema`` it emits to clients when structured output is enabled.
"""

from __future__ import annotations

from enum import StrEnum

from pydantic import BaseModel, ConfigDict, Field, model_validator

from topos.evaluation.policies.base import Priority
from topos.evaluation.preferences import Generator, UserPreferences


class LatticeElement(StrEnum):
    """String-valued mirror of ``EvaluationValue`` for MCP wire format.

    These are the 8 elements of the free Heyting algebra H(G_qual) on the
    three generators SIMPLE, COMPOSABLE, SECURE.  Mapped to the Medal Podium:
    IDEAL = 🥇 GOLD, SLOP = ❌ No Medal.
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


class PrioritySource(StrEnum):
    """How the MCP layer selected the scorer priority."""

    DEFAULT = "default"
    PREFERENCES = "preferences"
    EXPLICIT = "explicit"


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
    relaxation walk* — by convention 🥇 GOLD (IDEAL) is treated as infeasible
    and the default target becomes the meet of the top-two ranked
    generators (the 🥈 SILVER "ideal intersection", e.g. ``SIMPLE_SECURE``).

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

    def to_priority(self) -> Priority:
        """Use the top-ranked generator as the scorer priority."""
        first = self.ranking[0]
        return {
            Generator.SIMPLE: Priority.SIMPLE,
            Generator.COMPOSABLE: Priority.COMPOSABLE,
            Generator.SECURE: Priority.SECURE,
        }[first]


def resolve_priority(
    preferences: UserPreferencesInput | None,
) -> tuple[Priority, PrioritySource]:
    """Resolve MCP preference input to the legacy scorer priority."""
    if preferences is None:
        return Priority.SIMPLE, PrioritySource.DEFAULT
    return preferences.to_priority(), PrioritySource.PREFERENCES


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
            "'python', 'rust', 'javascript', 'typescript', 'cpp'."
        ),
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Strict total order on the three generators. The result includes "
            "a targeted relaxation walk toward the 'ideal intersection' "
            "(meet of the top-two ranked generators)."
        ),
    )
    verbose: bool = Field(
        default=False,
        description="Include raw probe metric floats under each file in the response.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description=(
            "One-off acknowledged dangerous-call patterns for this evaluation. "
            "Mirrors CLI --allow and is fully disclosed in the result."
        ),
    )


class EvaluateFileInput(_StrictModel):
    """Arguments for ``topos_evaluate_file``."""

    filepath: str = Field(
        ...,
        description="Path to the source file, relative to the project root.",
        min_length=1,
    )
    gitnexus_dir: str | None = Field(
        default=None,
        description=(
            "Path to a .gitnexus/ directory produced by `topos depgraph "
            "generate`. When provided, the ModuleDependencyGraph is "
            "attached so the COMPOSABLE generator can be scored. "
            "Defaults to <project_root>/.gitnexus if it exists."
        ),
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Strict total order on the three generators; see "
            "``topos://docs/preferences``."
        ),
    )
    include_security_findings: bool = Field(
        default=True,
        description=(
            "Include line/snippet diagnostics for dangerous API calls when "
            "SECURE fails."
        ),
    )
    allow: list[str] = Field(
        default_factory=list,
        description=(
            "One-off acknowledged dangerous-call patterns for this evaluation. "
            "Mirrors CLI --allow and is fully disclosed in the result."
        ),
    )
    verbose: bool = Field(
        default=False,
        description="Include raw probe metric floats under each file in the response.",
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
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Strict total order on the three generators; see "
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
    verbose: bool = Field(
        default=False,
        description="Include raw probe metric floats under each file in the response.",
    )
    include_security_findings: bool = Field(
        default=False,
        description=(
            "Include line/snippet diagnostics for SECURE failures in per-file entries. "
            "Defaults off for project scans to keep responses compact."
        ),
    )
    allow: list[str] = Field(
        default_factory=list,
        description=(
            "One-off acknowledged dangerous-call patterns for this evaluation. "
            "Mirrors CLI --allow and is fully disclosed in each affected file entry."
        ),
    )


class CompareCodeInput(_StrictModel):
    """Arguments for ``topos_compare_code``."""

    source_code: str = Field(..., min_length=1, description="Baseline code.")
    target_code: str = Field(..., min_length=1, description="Proposed/target code.")
    language: str = Field(default="python")


class CompareFilesInput(_StrictModel):
    """Arguments for ``topos_compare_files``."""

    source: str = Field(..., min_length=1, description="Baseline file path.")
    target: str = Field(..., min_length=1, description="Proposed file path.")


class AssessImprovementInput(_StrictModel):
    """Arguments for ``topos_assess_improvement``.

    Provide EITHER ``filepath`` (preferred — scores the COMPOSABLE
    generator against the cached ModuleDependencyGraph) OR
    ``current_code`` (AST + CFG + CPG only; COMPOSABLE unreachable).
    """

    proposed_code: str | None = Field(
        default=None, min_length=1, description="The refactored / proposed source."
    )
    proposed_filepath: str | None = Field(
        default=None,
        min_length=1,
        description=(
            "Path to a proposed version inside the project root. Use this to avoid "
            "sending full source text for large files."
        ),
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
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Strict total order on the three generators; see "
            "``topos://docs/preferences``."
        ),
    )
    gitnexus_dir: str | None = Field(default=None)
    include_security_findings: bool = Field(
        default=True,
        description=(
            "Include line/snippet diagnostics for dangerous API calls when "
            "SECURE fails."
        ),
    )
    allow: list[str] = Field(
        default_factory=list,
        description=(
            "One-off acknowledged dangerous-call patterns for this assessment. "
            "Mirrors CLI --allow and is fully disclosed in nested evaluations."
        ),
    )

    @model_validator(mode="after")
    def validate_inputs(self) -> AssessImprovementInput:
        proposed_count = sum(
            value is not None for value in (self.proposed_code, self.proposed_filepath)
        )
        if proposed_count != 1:
            raise ValueError(
                "Provide exactly one of `proposed_code` or `proposed_filepath`."
            )
        baseline_count = sum(
            value is not None for value in (self.filepath, self.current_code)
        )
        if baseline_count != 1:
            raise ValueError("Provide exactly one of `filepath` or `current_code`.")
        return self


class InspectCodeInput(_StrictModel):
    """Arguments for ``topos_inspect_code``."""

    code: str | None = Field(default=None, min_length=1)
    filepath: str | None = Field(
        default=None,
        min_length=1,
        description=(
            "Path to the source file inside the project root. Prefer this for "
            "large files."
        ),
    )
    language: str = Field(default="python")
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description=(
            "Strict total order on the three generators; see "
            "``topos://docs/preferences``."
        ),
    )
    top_n_functions: int = Field(
        default=10,
        ge=1,
        le=200,
        description=(
            "Return at most this many functions, sorted by descending "
            "cyclomatic complexity. Keeps agent context lean on large files."
        ),
    )
    verbose: bool = Field(
        default=False,
        description="Include raw probe metric floats under each file in the response.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description=(
            "One-off acknowledged dangerous-call patterns for this inspection. "
            "Mirrors CLI --allow and is fully disclosed in the evaluation."
        ),
    )

    @model_validator(mode="after")
    def validate_source(self) -> InspectCodeInput:
        source_count = sum(value is not None for value in (self.code, self.filepath))
        if source_count != 1:
            raise ValueError("Provide exactly one of `code` or `filepath`.")
        return self


class CalculateCoverageInput(_StrictModel):
    """Arguments for ``topos_calculate_coverage``."""

    put_files: list[str] = Field(
        ...,
        min_length=1,
        description="Paths to the program-under-test files (relative to project root).",
    )
    test_files: list[str] = Field(
        ...,
        min_length=0,
        description="Paths to the test suite files (relative to project root).",
    )
    language: str = Field(
        default="python",
        description="Programming language (for parsing).",
    )
    k: int = Field(
        default=3,
        ge=1,
        description="Length of kind n-grams for path recall.",
    )
    include_unknown: bool = Field(
        default=False,
        description="Whether to include Unknown UAST nodes in the analysis.",
    )
    coverage_threshold: float = Field(
        default=0.5,
        ge=0.0,
        le=1.0,
        description=(
            "Minimum threshold for declaration and topological coverage policies."
        ),
    )


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
            "first.  Required — supply an explicit permutation (no implicit default)."
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
    warnings: list[str] = Field(default_factory=list)
    error: str | None = None


class PreferenceWalk(BaseModel):
    """Targeted relaxation walk derived from a user preference ranking.

    Two-stage strategy:

    1. **Aim for 🥇 GOLD** (``target``) — try to beat the policy
       thresholds for all three generators.
    2. **Divert to the 🥈 SILVER "ideal intersection"** (``fallback_target``)
       when 🥇 GOLD plateaus — the meet of the top-two ranked
       generators per the preference ordering.

    Beyond the fallback the walk continues down through 🥉 BRONZE atoms toward
    ❌ ``SLOP``, in descending preference order.
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


class PillarResult(BaseModel):
    """Lean per-generator summary (achieved + score).

    The full per-metric detail and interpretation strings live once in the
    parent's flat ``raw_metrics`` / ``interpretation`` maps (namespaced by
    representation prefix: ``cfg.``/``ast.`` -> simple, ``mdg.`` -> composable,
    ``cpg.`` -> secure, ``pdg.`` -> unprefixed structural). They are NOT
    duplicated here.
    """

    achieved: bool = Field(
        ..., description="Whether the generator's threshold was met."
    )
    score: float = Field(..., description="Normalized quality score in [0, 100].")


class SecurityFinding(BaseModel):
    """Actionable SECURE diagnostic for an agent."""

    kind: str = Field(..., description="Finding kind, e.g. dangerous_call.")
    line: int = Field(..., ge=1, description="1-based source line.")
    snippet: str = Field(..., description="Source snippet for the finding.")
    callee: str | None = Field(default=None, description="Detected dangerous callee.")
    source: str | None = Field(default=None, description="Taint source snippet.")
    sink: str | None = Field(default=None, description="Taint sink snippet.")


class AcknowledgedRisk(BaseModel):
    """A disclosed security finding acknowledged by project config or input."""

    callee: str | None = None
    kind: str
    line: int = Field(..., ge=1)
    snippet: str
    reason: str
    scope: str = "**"


class FunctionEntry(BaseModel):
    """Function-level complexity diagnostic."""

    name: str
    line: int = Field(..., ge=1)
    complexity: int


class Suggestion(BaseModel):
    """One actionable, refactor-focused next step.

    Wire mirror of ``topos.evaluation.suggestions.Suggestion`` — the pure
    engine emits the frozen dataclass; this is its Pydantic shape so the same
    suggestions reach MCP agents that the CLI already shows.
    """

    pillar: str = Field(..., description="simple | composable | secure | coverage.")
    metric: str | None = Field(
        default=None, description="Raw-metric key, or null for finding-derived."
    )
    severity: str = Field(
        ..., description="'fix' (gate failed) | 'improve' (advisory)."
    )
    message: str = Field(..., description="Imperative instruction to act on.")


class AgentContract(BaseModel):
    """Compact loop-control packet for agentic harnesses."""

    next_tool: str | None = Field(
        default=None,
        description="Recommended next MCP tool for the agent loop, if any.",
    )
    next_actions: list[str] = Field(default_factory=list)
    blocked_by: list[str] = Field(default_factory=list)
    verification_gates: list[str] = Field(default_factory=list)
    risk_flags: list[str] = Field(default_factory=list)


class EvaluationResult(BaseModel):
    """Result of a single-unit evaluation on the Medal Podium."""

    is_parseable: bool
    lattice_element: LatticeElement
    lattice_symbol: str
    lattice_description: str
    dimensions: dict[str, LatticeElement]
    scores: dict[str, float] = Field(
        ..., description="Per-dimension normalized score in [0, 100]."
    )
    pillars: dict[str, PillarResult] = Field(
        default_factory=dict,
        description="Per-pillar breakdown (simple, composable, secure).",
    )
    priority: Priority
    priority_source: PrioritySource = Field(
        default=PrioritySource.DEFAULT,
        description=(
            "Whether priority was defaulted, inferred from preferences, or explicit."
        ),
    )
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
    warnings: list[str] = Field(default_factory=list)
    agent_contract: AgentContract | None = None
    security_findings: list[SecurityFinding] = Field(default_factory=list)
    acknowledged_risks: list[AcknowledgedRisk] = Field(default_factory=list)
    raw_lattice_element: LatticeElement | None = Field(
        default=None,
        description="Canonical raw verdict before acknowledged-risk overlay.",
    )
    adjusted_lattice_element: LatticeElement | None = Field(
        default=None,
        description="Verdict after acknowledged-risk overlay and grade cap.",
    )
    secure_raw: bool | None = Field(
        default=None, description="Raw SECURE gate before acknowledged-risk overlay."
    )
    secure_adjusted: bool | None = Field(
        default=None, description="SECURE gate after acknowledged-risk overlay."
    )
    grade_capped: bool = Field(
        default=False,
        description="True when acknowledged risk prevents a top IDEAL grade.",
    )
    suggestions: list[Suggestion] = Field(
        default_factory=list,
        description=(
            "Actionable, refactor-focused next steps derived from the failing "
            "policy gates and active security findings."
        ),
    )
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
    pillars: dict[str, PillarResult] = Field(default_factory=dict)
    raw_metrics: dict[str, float] = Field(default_factory=dict)
    warnings: list[str] = Field(default_factory=list)
    security_findings: list[SecurityFinding] = Field(default_factory=list)
    acknowledged_risks: list[AcknowledgedRisk] = Field(default_factory=list)
    raw_lattice_element: LatticeElement | None = None
    adjusted_lattice_element: LatticeElement | None = None
    secure_raw: bool | None = None
    secure_adjusted: bool | None = None
    grade_capped: bool = False
    is_parseable: bool = True


class ProjectEvaluationResult(BaseModel):
    """Result of a directory-wide evaluation."""

    root: str
    file_count: int
    parse_failures: int
    rolled_up_dimensions: dict[str, LatticeElement]
    rolled_up_scores: dict[str, float]
    aggregate_floor_verdict: LatticeElement
    aggregate_explanation: str
    worst_file_verdict: LatticeElement | None = None
    worst_files: list[ProjectFileEntry] = Field(default_factory=list)
    guidance: str = ""
    priority: Priority
    priority_source: PrioritySource = PrioritySource.DEFAULT
    coupling_available: bool
    warnings: list[str] = Field(default_factory=list)
    agent_contract: AgentContract | None = None
    count: int = Field(..., description="Entries in this page.")
    offset: int
    total: int
    has_more: bool
    next_offset: int | None
    files: list[ProjectFileEntry]
    verbose: bool = False
    error: str | None = None


class ComparisonResult(BaseModel):
    """Result of AST-distance comparison between two programs."""

    raw_distance: float
    normalized_distance: float
    similarity: float
    operations: dict[str, int]
    source_valid: bool
    target_valid: bool
    warnings: list[str] = Field(default_factory=list)
    error: str | None = None


class AssessmentResult(BaseModel):
    """Result of ``topos_assess_improvement``."""

    status: AssessmentStatus
    priority: Priority
    priority_source: PrioritySource = PrioritySource.DEFAULT
    current: EvaluationResult
    proposed: EvaluationResult
    score_deltas: dict[str, float] = Field(
        ..., description="Change in pillar scores (proposed - current)."
    )
    metric_deltas: dict[str, float] = Field(
        default_factory=dict,
        description=(
            "Change in raw metrics (proposed - current). "
            "Useful for tracking progress against specific thresholds."
        ),
    )
    structural_distance: float | None = None
    similarity: float | None = None
    coupling_available_for_proposed: bool
    warnings: list[str] = Field(default_factory=list)
    agent_contract: AgentContract | None = None
    # Anti-gaming: populated when scores moved but the tree barely changed.
    suspicion_reason: str | None = None
    # On a regression: a function-scoped unified diff of the single worst
    # function (largest adverse complexity increase), so the LLM can pinpoint
    # what got worse instead of diffing two full metric trees.
    regression_diff: str | None = None
    error: str | None = None


class InspectionResult(BaseModel):
    """Result of ``topos_inspect_code`` — full breakdown."""

    evaluation: EvaluationResult
    functions: dict[str, int] = Field(
        default_factory=dict,
        description="Deprecated: function_name -> cyclomatic_complexity (top N only).",
    )
    function_entries: list[FunctionEntry] = Field(
        default_factory=list,
        description="Top-N functions with line numbers and cyclomatic complexity.",
    )
    total_functions: int
    entropy_compression_ratio: float | None = None
    entropy_interpretation: str | None = None
    error: str | None = None


class TopologicalCoverageResult(BaseModel):
    """ECT-based topological semantic coverage (optional extra)."""

    unavailable: bool = False
    reason: str | None = None
    distance: float | None = None
    coverage_score: float | None = None
    tested_functions: list[str] = Field(default_factory=list)
    untested_functions: list[str] = Field(default_factory=list)
    put_node_count: int | None = None
    test_node_count: int | None = None
    scoped_node_count: int | None = None
    achieved: bool | None = None
    threshold: float | None = None
    interpretation: dict[str, str] = Field(default_factory=dict)


class CoverageResult(BaseModel):
    """Structural test coverage report (v2)."""

    mean_declaration_coverage: float = Field(
        ..., description="[0, 1] average recall across PUT declarations."
    )
    best_declaration_recall: list[float] = Field(
        ..., description="Recall per declaration in the PUT."
    )
    declaration_locations: list[str] = Field(
        ..., description="Source location (file:line) for each declaration."
    )
    stmt_recall: float = Field(
        ..., description="[0, 1] multiset recall of Statement kinds."
    )
    expr_recall: float = Field(
        ..., description="[0, 1] multiset recall of Expression kinds."
    )
    mean_test_precision: float = Field(
        ..., description="[0, 1] average precision across test declarations."
    )
    f2_score: float = Field(..., description="F2 score favoring recall over precision.")
    declaration_path_recall_kgram: float = Field(
        ..., description="[0, 1] kind n-gram path recall."
    )
    uncovered_declarations: list[str] = Field(
        ..., description="Locations of declarations with incomplete test coverage."
    )
    put_declaration_count: int
    test_declaration_count: int
    topological_coverage: TopologicalCoverageResult | None = None
    warnings: list[str] = Field(default_factory=list)
    error: str | None = None
