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
    """Strict ranking over simple, composable, and secure."""

    ranking: list[Generator] = Field(
        ...,
        description="Permutation of simple/composable/secure, best first.",
        min_length=3,
        max_length=3,
    )
    target: LatticeElement | None = Field(
        default=None,
        description="Optional explicit target verdict.",
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
        description="Source code to evaluate.",
        min_length=1,
    )
    language: str = Field(
        default="python",
        description="Language: python, rust, javascript, typescript, or cpp.",
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description="Optional generator ranking.",
    )
    verbose: bool = Field(
        default=False,
        description="Include raw metrics.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
    )


class EvaluateFileInput(_StrictModel):
    """Arguments for ``topos_evaluate_file``."""

    filepath: str = Field(
        ...,
        description="Source file path.",
        min_length=1,
    )
    gitnexus_dir: str | None = Field(
        default=None,
        description=".gitnexus directory for COMPOSABLE scoring.",
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description="Optional generator ranking.",
    )
    include_security_findings: bool = Field(
        default=True,
        description="Include SECURE findings.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
    )
    verbose: bool = Field(
        default=False,
        description="Include raw metrics.",
    )


class EvaluateProjectInput(_StrictModel):
    """Arguments for ``topos_evaluate_project``."""

    path: str = Field(
        ...,
        description="Directory to evaluate.",
        min_length=1,
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description="Optional generator ranking.",
    )
    gitnexus_dir: str | None = Field(
        default=None, description=".gitnexus directory for COMPOSABLE scoring."
    )
    limit: int = Field(
        default=25,
        ge=1,
        le=500,
        description="Page size.",
    )
    offset: int = Field(
        default=0,
        ge=0,
        description="Pagination offset.",
    )
    verbose: bool = Field(
        default=False,
        description="Include raw metrics.",
    )
    include_security_findings: bool = Field(
        default=False,
        description="Include per-file SECURE findings.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
    )


class CompareCodeInput(_StrictModel):
    """Arguments for ``topos_compare_code``."""

    source_code: str = Field(..., min_length=1, description="Baseline code.")
    target_code: str = Field(..., min_length=1, description="Proposed/target code.")
    language: str = Field(
        default="python",
        description="python, rust, javascript, typescript, or cpp.",
    )


class CompareFilesInput(_StrictModel):
    """Arguments for ``topos_compare_files``."""

    source: str = Field(..., min_length=1, description="Baseline file path.")
    target: str = Field(..., min_length=1, description="Comparison file path.")


class AssessImprovementInput(_StrictModel):
    """Side-by-side assessment. In-place edits use worktree/snapshot tools."""

    proposed_code: str | None = Field(
        default=None, min_length=1, description="Proposed source."
    )
    proposed_filepath: str | None = Field(
        default=None,
        min_length=1,
        description="Proposed file path.",
    )
    filepath: str | None = Field(
        default=None,
        description="Baseline file path for side-by-side assessment.",
    )
    current_code: str | None = Field(
        default=None,
        description="Inline baseline source; COMPOSABLE is unavailable.",
    )
    language: str = Field(default="python")
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description="Optional generator ranking.",
    )
    gitnexus_dir: str | None = Field(default=None)
    include_security_findings: bool = Field(
        default=True,
        description="Include SECURE findings.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
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


class BeginRefactorInput(_StrictModel):
    """Capture a dirty/untracked baseline before editing."""

    filepath: str = Field(
        ...,
        min_length=1,
        description="File path to snapshot.",
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description="Optional generator ranking.",
    )
    gitnexus_dir: str | None = Field(
        default=None,
        description=".gitnexus directory.",
    )


class AssessSnapshotInput(_StrictModel):
    """Assess current file against a captured baseline."""

    snapshot_id: str = Field(
        ...,
        min_length=1,
        description="Snapshot id from topos_begin_refactor.",
    )
    filepath: str = Field(
        ...,
        min_length=1,
        description="Edited file path.",
    )
    include_security_findings: bool = Field(
        default=True,
        description="Include SECURE findings.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
    )


class AssessWorktreeChangeInput(_StrictModel):
    """Assess an in-place edit against a git baseline."""

    filepath: str = Field(
        ...,
        min_length=1,
        description="Edited file path.",
    )
    baseline_ref: str = Field(
        default="HEAD",
        min_length=1,
        description="Git baseline ref.",
    )
    preferences: UserPreferencesInput | None = Field(
        default=None,
        description="Optional generator ranking.",
    )
    gitnexus_dir: str | None = Field(default=None)
    include_security_findings: bool = Field(
        default=True,
        description="Include SECURE findings.",
    )
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
    )


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
    """Arguments for ``topos_preference_walk``."""

    ranking: list[Generator] = Field(
        ...,
        description=(
            "Permutation of {simple, composable, secure}, most-preferred first."
        ),
        min_length=3,
        max_length=3,
    )
    current: LatticeElement | None = Field(
        default=None,
        description=(
            "Optional current verdict; truncates the walk to steps strictly "
            "above it and sets ``next_step``. Defaults to the full walk."
        ),
    )
    target: LatticeElement | None = Field(
        default=None,
        description="Optional aspirational-target override; defaults to IDEAL.",
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
        description="Divert target when the aspirational target plateaus.",
    )
    current: LatticeElement | None = Field(
        default=None,
        description="The verdict the walk was computed against, if any.",
    )
    next_step: LatticeElement | None = Field(
        default=None,
        description=(
            "Smallest improvement above ``current``; null when at/beyond target."
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
        description="Steps from the target down to just above ``current``.",
    )
    induced_order: list[WalkStep] = Field(
        default_factory=list,
        description="All 8 verdicts ranked by descending preference.",
    )
    warnings: list[str] = Field(default_factory=list)
    error: str | None = None


class PreferenceWalk(BaseModel):
    """Targeted relaxation walk for a preference ranking."""

    ranking: list[Generator] = Field(
        ..., description="The preference ranking, most-preferred first."
    )
    target: LatticeElement = Field(
        ...,
        description="Aspirational target.",
    )
    fallback_target: LatticeElement = Field(
        ...,
        description="Divert target when IDEAL stalls.",
    )
    walk: list[LatticeElement] = Field(
        default_factory=list,
        description="Preference-ordered verdict path above current.",
    )
    next_step: LatticeElement | None = Field(
        default=None,
        description="Immediate next verdict above current.",
    )
    progress: float = Field(
        ...,
        ge=0.0,
        le=1.0,
        description="Progress toward target in [0, 1].",
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
    """Function-level complexity diagnostic.

    ``name``/``line``/``complexity`` are the legacy fields. The remaining
    optional fields let an agent map a failing complexity gate back to a
    concrete AST location (see ``EvaluationResult.metric_locations``).
    """

    name: str
    line: int = Field(..., ge=1)
    complexity: int
    qualified_name: str | None = Field(
        default=None, description="Dotted scope path, e.g. 'Cls.method.closure'."
    )
    kind: str | None = Field(
        default=None,
        description="function | async_function | method | closure | module.",
    )
    start_line: int | None = Field(default=None, ge=1)
    end_line: int | None = Field(default=None, ge=1)
    metric_source: str | None = Field(
        default=None,
        description="Which probe produced the complexity: 'ast' or 'cfg'.",
    )
    includes_nested: bool | None = Field(
        default=None,
        description="True when the count includes nested callables' decisions.",
    )


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
    metric_locations: dict[str, list[FunctionEntry]] = Field(
        default_factory=dict,
        description=(
            "Source locations for failing complexity gates, keyed by metric "
            "(e.g. 'ast.max_function_complexity'). A single entry with "
            "kind='module' means the metric is module-level only, not "
            "attributable to a function."
        ),
    )
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


class SnapshotResult(BaseModel):
    """Result of ``topos_begin_refactor`` — a captured baseline handle."""

    snapshot_id: str = Field(
        ...,
        description="Opaque handle for this capture; pass to topos_assess_snapshot.",
    )
    filepath: str = Field(..., description="The file the baseline was captured from.")
    baseline_hash: str = Field(..., description="sha256 of the baseline source.")
    created_at: float = Field(..., description="Unix timestamp the snapshot was taken.")
    warnings: list[str] = Field(default_factory=list)
    agent_contract: AgentContract | None = None
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
    # sha256 of the baseline and current (proposed) source. Let snapshot/worktree
    # callers confirm exactly which revisions were compared without re-sending
    # source. None only on the error path.
    baseline_hash: str | None = None
    current_hash: str | None = None
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


class DepgraphState(StrEnum):
    """Structured state of the ``.gitnexus`` dependency-graph index."""

    MISSING = "missing"
    PRESENT = "present"
    STALE = "stale"
    LOAD_ERROR = "load_error"
    SCHEMA_MISMATCH = "schema_mismatch"


class DepgraphStatusInput(_StrictModel):
    """Arguments for ``topos_depgraph_status``."""

    gitnexus_dir: str | None = Field(
        default=None,
        description="Override .gitnexus directory (default: <root>/.gitnexus).",
    )


class DepgraphStatusResult(BaseModel):
    """Result of ``topos_depgraph_status`` — read-only graph state."""

    state: DepgraphState
    gitnexus_dir: str | None = None
    gitnexus_mtime: float | None = None
    git_head_mtime: float | None = None
    coupling_available: bool = Field(
        ...,
        description="True only when the graph loads and is not stale.",
    )
    detail: str | None = None
    recommended_next_action: str
    agent_contract: AgentContract | None = None
    error: str | None = None


class GenerateDepgraphInput(_StrictModel):
    """Arguments for ``topos_generate_depgraph`` (side-effecting)."""

    directory: str | None = Field(
        default=None,
        description="Repository root to analyze (default: the MCP file root).",
    )


class GenerateDepgraphResult(BaseModel):
    """Result of ``topos_generate_depgraph``."""

    ok: bool
    returncode: int
    gitnexus_dir: str | None = None
    message: str
    agent_contract: AgentContract | None = None
    error: str | None = None


class AssessChangesetInput(_StrictModel):
    """Arguments for ``topos_assess_changeset`` (multi-file refactor)."""

    files: list[str] = Field(
        ...,
        min_length=1,
        description="Edited file paths (working tree) that make up the changeset.",
    )
    baseline_ref: str = Field(
        default="HEAD",
        min_length=1,
        description="Git baseline ref each file is compared against.",
    )
    preferences: UserPreferencesInput | None = Field(
        default=None, description="Optional generator ranking."
    )
    gitnexus_dir: str | None = Field(default=None)
    refresh_depgraph: bool = Field(
        default=False,
        description=(
            "Regenerate .gitnexus before assessing. Side-effecting and "
            "approval-gated; leave false for a read-only assessment."
        ),
    )
    include_security_findings: bool = Field(default=True)
    allow: list[str] = Field(
        default_factory=list,
        description="One-off acknowledged dangerous-call patterns.",
    )


class ChangesetFileEntry(BaseModel):
    """Per-file before/after summary inside a changeset assessment."""

    filepath: str
    status: AssessmentStatus
    is_new: bool = Field(
        default=False, description="True when the file did not exist at baseline_ref."
    )
    baseline_verdict: LatticeElement | None = None
    current_verdict: LatticeElement | None = None
    score_deltas: dict[str, float] = Field(default_factory=dict)
    metric_deltas: dict[str, float] = Field(default_factory=dict)
    complexity_relocated_within_file: bool = Field(
        default=False,
        description=(
            "True when max function complexity improved but file cyclomatic "
            "complexity worsened — extraction stayed inside one module."
        ),
    )
    warnings: list[str] = Field(default_factory=list)
    blocked_by: str | None = None
    error: str | None = None


class ChangesetResult(BaseModel):
    """Result of ``topos_assess_changeset`` — multi-file rollup."""

    baseline_ref: str
    files: list[ChangesetFileEntry] = Field(default_factory=list)
    project_before: dict[str, LatticeElement] = Field(default_factory=dict)
    project_after: dict[str, LatticeElement] = Field(default_factory=dict)
    project_scores_before: dict[str, float] = Field(default_factory=dict)
    project_scores_after: dict[str, float] = Field(default_factory=dict)
    aggregate_before: LatticeElement = LatticeElement.SLOP
    aggregate_after: LatticeElement = LatticeElement.SLOP
    project_regression: bool = False
    complexity_relocated_files: list[str] = Field(default_factory=list)
    coupling_available: bool = False
    depgraph_refreshed: bool = False
    priority: Priority
    priority_source: PrioritySource = PrioritySource.DEFAULT
    warnings: list[str] = Field(default_factory=list)
    agent_contract: AgentContract | None = None
    error: str | None = None
