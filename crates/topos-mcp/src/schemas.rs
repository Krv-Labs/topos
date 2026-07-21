//! Wire schemas for the Topos MCP server.
//!
//! Input models validate tool arguments (via `schemars`-derived JSON
//! schemas); output models are the `structured_content` channel mirrored
//! from `topos/mcp/schemas.py`.

use std::collections::HashMap;

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use topos_core::core::omega::EvaluationValue;
use topos_core::evaluation::policies::base::Priority;
use topos_core::evaluation::preferences::{Generator, UserPreferences};

/// The 8 elements of the free Heyting algebra H(G_qual) on the three
/// generators SIMPLE, COMPOSABLE, SECURE, mirroring `EvaluationValue` on
/// the MCP wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[allow(non_camel_case_types)]
pub enum LatticeElement {
    #[default]
    SLOP,
    SIMPLE,
    COMPOSABLE,
    SECURE,
    SIMPLE_COMPOSABLE,
    SIMPLE_SECURE,
    COMPOSABLE_SECURE,
    IDEAL,
}

impl LatticeElement {
    pub fn as_str(self) -> &'static str {
        match self {
            LatticeElement::SLOP => "SLOP",
            LatticeElement::SIMPLE => "SIMPLE",
            LatticeElement::COMPOSABLE => "COMPOSABLE",
            LatticeElement::SECURE => "SECURE",
            LatticeElement::SIMPLE_COMPOSABLE => "SIMPLE_COMPOSABLE",
            LatticeElement::SIMPLE_SECURE => "SIMPLE_SECURE",
            LatticeElement::COMPOSABLE_SECURE => "COMPOSABLE_SECURE",
            LatticeElement::IDEAL => "IDEAL",
        }
    }
}

pub fn lattice_to_str(value: EvaluationValue) -> LatticeElement {
    match value {
        EvaluationValue::Slop => LatticeElement::SLOP,
        EvaluationValue::Simple => LatticeElement::SIMPLE,
        EvaluationValue::Composable => LatticeElement::COMPOSABLE,
        EvaluationValue::Secure => LatticeElement::SECURE,
        EvaluationValue::SimpleComposable => LatticeElement::SIMPLE_COMPOSABLE,
        EvaluationValue::SimpleSecure => LatticeElement::SIMPLE_SECURE,
        EvaluationValue::ComposableSecure => LatticeElement::COMPOSABLE_SECURE,
        EvaluationValue::Ideal => LatticeElement::IDEAL,
    }
}

pub fn str_to_lattice(value: LatticeElement) -> EvaluationValue {
    match value {
        LatticeElement::SLOP => EvaluationValue::Slop,
        LatticeElement::SIMPLE => EvaluationValue::Simple,
        LatticeElement::COMPOSABLE => EvaluationValue::Composable,
        LatticeElement::SECURE => EvaluationValue::Secure,
        LatticeElement::SIMPLE_COMPOSABLE => EvaluationValue::SimpleComposable,
        LatticeElement::SIMPLE_SECURE => EvaluationValue::SimpleSecure,
        LatticeElement::COMPOSABLE_SECURE => EvaluationValue::ComposableSecure,
        LatticeElement::IDEAL => EvaluationValue::Ideal,
    }
}

/// Outcome of comparing a proposed change to the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[allow(non_camel_case_types)]
pub enum AssessmentStatus {
    IMPROVEMENT,
    IMPROVEMENT_SCORE,
    LATERAL_MOVE,
    REGRESSION,
    REGRESSION_SCORE,
    /// Anti-gaming flag: scores moved meaningfully while the AST barely
    /// changed — suspicious unless the agent explains why.
    SUSPICIOUS_NO_STRUCTURAL_CHANGE,
}

impl AssessmentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AssessmentStatus::IMPROVEMENT => "IMPROVEMENT",
            AssessmentStatus::IMPROVEMENT_SCORE => "IMPROVEMENT_SCORE",
            AssessmentStatus::LATERAL_MOVE => "LATERAL_MOVE",
            AssessmentStatus::REGRESSION => "REGRESSION",
            AssessmentStatus::REGRESSION_SCORE => "REGRESSION_SCORE",
            AssessmentStatus::SUSPICIOUS_NO_STRUCTURAL_CHANGE => "SUSPICIOUS_NO_STRUCTURAL_CHANGE",
        }
    }
}

/// How the MCP layer selected the scorer priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrioritySource {
    #[default]
    Default,
    Preferences,
    Explicit,
}

/// Serialize `Priority` the way the Python wire did (lowercase value).
pub fn priority_str(priority: Priority) -> &'static str {
    match priority {
        Priority::Simple => "simple",
        Priority::Composable => "composable",
        Priority::Secure => "secure",
    }
}

/// Wire form of a generator name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorInput {
    Simple,
    Composable,
    Secure,
}

impl GeneratorInput {
    pub fn to_generator(self) -> Generator {
        match self {
            GeneratorInput::Simple => Generator::Simple,
            GeneratorInput::Composable => Generator::Composable,
            GeneratorInput::Secure => Generator::Secure,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GeneratorInput::Simple => "simple",
            GeneratorInput::Composable => "composable",
            GeneratorInput::Secure => "secure",
        }
    }
}

// ---------------------------------------------------------------------------
// Input models
// ---------------------------------------------------------------------------

/// Strict ranking over simple, composable, and secure.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserPreferencesInput {
    /// Permutation of simple/composable/secure, best first.
    pub ranking: Vec<GeneratorInput>,
    /// Optional explicit target verdict.
    #[serde(default)]
    pub target: Option<LatticeElement>,
}

impl UserPreferencesInput {
    /// Convert into the domain-layer `UserPreferences`.
    pub fn to_preferences(&self) -> Result<UserPreferences, String> {
        let ranking: Vec<Generator> = self.ranking.iter().map(|g| g.to_generator()).collect();
        let arr: [Generator; 3] = ranking
            .try_into()
            .map_err(|_| "ranking must contain exactly simple, composable, secure".to_string())?;
        let base = UserPreferences::new(arr).map_err(|e| e.to_string())?;
        match self.target {
            Some(t) => UserPreferences::with_target(arr, Some(str_to_lattice(t)))
                .map_err(|e| e.to_string()),
            None => Ok(base),
        }
    }

    /// Use the top-ranked generator as the scorer priority.
    pub fn to_priority(&self) -> Priority {
        match self.ranking.first() {
            Some(GeneratorInput::Simple) | None => Priority::Simple,
            Some(GeneratorInput::Composable) => Priority::Composable,
            Some(GeneratorInput::Secure) => Priority::Secure,
        }
    }
}

/// Resolve MCP preference input to the legacy scorer priority.
pub fn resolve_priority(preferences: Option<&UserPreferencesInput>) -> (Priority, PrioritySource) {
    match preferences {
        None => (Priority::Simple, PrioritySource::Default),
        Some(p) => (p.to_priority(), PrioritySource::Preferences),
    }
}

fn default_language() -> String {
    "python".to_string()
}

fn default_true() -> bool {
    true
}

fn default_baseline_ref() -> String {
    "HEAD".to_string()
}

/// Arguments for `topos_evaluate_code`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateCodeInput {
    /// Source code to evaluate.
    pub code: String,
    /// Language: python, rust, javascript, typescript, cpp, or go.
    #[serde(default = "default_language")]
    pub language: String,
    /// Optional generator ranking.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    /// Include raw metrics.
    #[serde(default)]
    pub verbose: bool,
    /// One-off acknowledged dangerous-call patterns.
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Arguments for `topos_evaluate_file`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateFileInput {
    /// Source file path.
    pub filepath: String,
    /// .gitnexus directory for COMPOSABLE scoring.
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
    /// Optional generator ranking.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    /// Include SECURE findings.
    #[serde(default = "default_true")]
    pub include_security_findings: bool,
    /// One-off acknowledged dangerous-call patterns.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Include raw metrics.
    #[serde(default)]
    pub verbose: bool,
    /// Ranked edit targets to return (0 = off, max 25).
    #[serde(default)]
    pub refactor_targets: usize,
}

fn default_project_limit() -> usize {
    25
}

/// Arguments for `topos_evaluate_project`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateProjectInput {
    /// Directory to evaluate, walked recursively. Must resolve inside the
    /// trusted file root; paths outside it are refused. All supported
    /// languages are autodetected — no language argument is needed.
    pub path: String,
    /// Optional ranking of simple/composable/secure (best first). The
    /// top-ranked generator sets scorer priority; omit to default to
    /// SIMPLE priority.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    /// Path to a `.gitnexus` dependency-graph directory, required for
    /// COMPOSABLE scoring. When omitted, it is auto-detected from the
    /// project root; if none is found, COMPOSABLE is reported as
    /// unavailable rather than failing.
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
    /// Per-file rows to return per page (1–500, default 25).
    #[serde(default = "default_project_limit")]
    pub limit: usize,
    /// Zero-based row offset for pagination; pass the response's
    /// `next_offset` to fetch the next page.
    #[serde(default)]
    pub offset: usize,
    /// When true, include each file's raw metric values alongside scores.
    #[serde(default)]
    pub verbose: bool,
    /// When true, attach per-file SECURE findings to each entry; off by
    /// default to keep responses compact.
    #[serde(default)]
    pub include_security_findings: bool,
    /// Dangerous-call patterns to acknowledge for this run only.
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Arguments for `topos_compare_code`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareCodeInput {
    /// Baseline code.
    pub source_code: String,
    /// Proposed/target code.
    pub target_code: String,
    /// python, rust, javascript, typescript, cpp, or go.
    #[serde(default = "default_language")]
    pub language: String,
}

/// Arguments for `topos_compare_files`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareFilesInput {
    /// Baseline file path.
    pub source: String,
    /// Comparison file path.
    pub target: String,
}

/// Side-by-side assessment. In-place edits use worktree/snapshot tools.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessImprovementInput {
    /// Proposed source.
    #[serde(default)]
    pub proposed_code: Option<String>,
    /// Proposed file path.
    #[serde(default)]
    pub proposed_filepath: Option<String>,
    /// Baseline file path for side-by-side assessment.
    #[serde(default)]
    pub filepath: Option<String>,
    /// Inline baseline source; COMPOSABLE is unavailable.
    #[serde(default)]
    pub current_code: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    /// Optional generator ranking.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
    /// Include SECURE findings.
    #[serde(default = "default_true")]
    pub include_security_findings: bool,
    /// One-off acknowledged dangerous-call patterns.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl AssessImprovementInput {
    /// Mirror of the pydantic model validator.
    pub fn validate(&self) -> Result<(), String> {
        let proposed = self.proposed_code.is_some() as u8 + self.proposed_filepath.is_some() as u8;
        if proposed != 1 {
            return Err("Provide exactly one of `proposed_code` or `proposed_filepath`.".into());
        }
        let baseline = self.filepath.is_some() as u8 + self.current_code.is_some() as u8;
        if baseline != 1 {
            return Err("Provide exactly one of `filepath` or `current_code`.".into());
        }
        Ok(())
    }
}

/// Capture a dirty/untracked baseline before editing.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BeginRefactorInput {
    /// File path to snapshot.
    pub filepath: String,
    /// Optional generator ranking.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    /// .gitnexus directory.
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
}

/// Assess current file against a captured baseline.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessSnapshotInput {
    /// Snapshot id from topos_begin_refactor.
    pub snapshot_id: String,
    /// Edited file path.
    pub filepath: String,
    /// Include SECURE findings.
    #[serde(default = "default_true")]
    pub include_security_findings: bool,
    /// One-off acknowledged dangerous-call patterns.
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Assess an in-place edit against a git baseline.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessWorktreeChangeInput {
    /// Edited file path.
    pub filepath: String,
    /// Git baseline ref.
    #[serde(default = "default_baseline_ref")]
    pub baseline_ref: String,
    /// Optional generator ranking.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
    /// Include SECURE findings.
    #[serde(default = "default_true")]
    pub include_security_findings: bool,
    /// One-off acknowledged dangerous-call patterns.
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Arguments for `topos_assess_changeset` (multi-file refactor).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssessChangesetInput {
    /// Edited file paths (working tree) that make up the changeset.
    pub files: Vec<String>,
    /// Git baseline ref each file is compared against.
    #[serde(default = "default_baseline_ref")]
    pub baseline_ref: String,
    /// Optional generator ranking.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
    #[serde(default = "default_true")]
    pub include_security_findings: bool,
    /// One-off acknowledged dangerous-call patterns.
    #[serde(default)]
    pub allow: Vec<String>,
}

fn default_top_n_functions() -> usize {
    10
}

/// Arguments for `topos_inspect_code`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectCodeInput {
    #[serde(default)]
    pub code: Option<String>,
    /// Path to the source file inside the project root. Prefer this for
    /// large files.
    #[serde(default)]
    pub filepath: Option<String>,
    /// Language for inline `code`; ignored for `filepath`, which is
    /// autodetected from the file extension.
    #[serde(default = "default_language")]
    pub language: String,
    /// Strict total order on the three generators; see
    /// `topos://docs/preferences`.
    #[serde(default)]
    pub preferences: Option<UserPreferencesInput>,
    /// Return at most this many functions, sorted by descending cyclomatic
    /// complexity. Keeps agent context lean on large files.
    #[serde(default = "default_top_n_functions")]
    pub top_n_functions: usize,
    /// Include raw probe metric floats under each file in the response.
    #[serde(default)]
    pub verbose: bool,
    /// One-off acknowledged dangerous-call patterns for this inspection.
    #[serde(default)]
    pub allow: Vec<String>,
}

fn default_k() -> usize {
    3
}

fn default_coverage_threshold() -> f64 {
    0.5
}

/// Arguments for `topos_calculate_coverage`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalculateCoverageInput {
    /// Paths to the program-under-test files (relative to project root).
    pub put_files: Vec<String>,
    /// Paths to the test suite files (relative to project root).
    pub test_files: Vec<String>,
    /// Programming language (for parsing).
    #[serde(default = "default_language")]
    pub language: String,
    /// Length of kind n-grams for path recall.
    #[serde(default = "default_k")]
    pub k: usize,
    /// Whether to include Unknown UAST nodes in the analysis.
    #[serde(default)]
    pub include_unknown: bool,
    /// Minimum threshold for the declaration coverage policy.
    #[serde(default = "default_coverage_threshold")]
    pub coverage_threshold: f64,
}

/// Arguments for `topos_preference_walk`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreferenceWalkInput {
    /// Permutation of {simple, composable, secure}, most-preferred first.
    pub ranking: Vec<GeneratorInput>,
    /// Optional current verdict; truncates the walk to steps strictly above
    /// it and sets `next_step`. Defaults to the full walk.
    #[serde(default)]
    pub current: Option<LatticeElement>,
    /// Optional aspirational-target override; defaults to IDEAL.
    #[serde(default)]
    pub target: Option<LatticeElement>,
}

/// Arguments for `topos_depgraph_status`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DepgraphStatusInput {
    /// Override .gitnexus directory (default: <root>/.gitnexus).
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
}

/// Arguments for `topos_generate_depgraph` (side-effecting).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateDepgraphInput {
    /// Repo root (default: MCP file root).
    #[serde(default)]
    pub directory: Option<String>,
    /// Regenerate even when current.
    #[serde(default)]
    pub force: bool,
}

fn default_refactor_limit() -> usize {
    5
}

/// Arguments for `topos_refactor`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefactorInput {
    /// cycles | dependencies | process | graphify
    pub target: RefactorTargetKind,
    pub filepath: String,
    #[serde(default)]
    pub gitnexus_dir: Option<String>,
    #[serde(default)]
    pub graphify_dir: Option<String>,
    #[serde(default = "default_refactor_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RefactorTargetKind {
    Cycles,
    Dependencies,
    Process,
    Graphify,
}

impl RefactorTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefactorTargetKind::Cycles => "cycles",
            RefactorTargetKind::Dependencies => "dependencies",
            RefactorTargetKind::Process => "process",
            RefactorTargetKind::Graphify => "graphify",
        }
    }
}

/// Topic for `topos_get_doc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DocTopic {
    AgentContract,
    Lattice,
    Metrics,
    Preferences,
    Priority,
    Workflows,
}

/// Arguments for `topos_get_doc`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDocInput {
    /// agent-contract | lattice | metrics | preferences | priority | workflows
    pub topic: DocTopic,
}

// ---------------------------------------------------------------------------
// Return models (structured output)
// ---------------------------------------------------------------------------

/// One verdict on the relaxation walk, annotated with the
/// satisfied-generator set so an agent can see at a glance what changing to
/// this verdict requires.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WalkStep {
    /// The Ω element for this step.
    pub verdict: LatticeElement,
    /// Lex-preference score (higher = more preferred).
    pub preference_score: u32,
    /// Generators the verdict satisfies (bit-decoded from the verdict).
    pub generators_satisfied: Vec<GeneratorInput>,
}

/// Result of `topos_preference_walk` — the agent's concrete walk.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PreferenceWalkResult {
    pub ranking: Vec<GeneratorInput>,
    /// What the agent should aim for first (default `IDEAL`).
    pub aspirational_target: LatticeElement,
    /// Divert target when the aspirational target plateaus.
    pub fallback_target: LatticeElement,
    /// The verdict the walk was computed against, if any.
    pub current: Option<LatticeElement>,
    /// Smallest improvement above `current`; null when at/beyond target.
    pub next_step: Option<LatticeElement>,
    /// Fractional progress from SLOP to the aspirational target.
    pub progress: f64,
    /// Steps from the target down to just above `current`.
    pub walk: Vec<WalkStep>,
    /// All 8 verdicts ranked by descending preference.
    pub induced_order: Vec<WalkStep>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// Targeted relaxation walk for a preference ranking (embedded form).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PreferenceWalk {
    /// The preference ranking, most-preferred first.
    pub ranking: Vec<GeneratorInput>,
    /// Aspirational target.
    pub target: LatticeElement,
    /// Divert target when IDEAL stalls.
    pub fallback_target: LatticeElement,
    /// Preference-ordered verdict path above current.
    pub walk: Vec<LatticeElement>,
    /// Immediate next verdict above current.
    pub next_step: Option<LatticeElement>,
    /// Progress toward target in [0, 1].
    pub progress: f64,
}

/// Lean per-generator summary (achieved + score).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PillarResult {
    /// Whether the generator's threshold was met.
    pub achieved: bool,
    /// Normalized quality score in [0, 100].
    pub score: f64,
}

/// Actionable SECURE diagnostic for an agent.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SecurityFinding {
    /// Finding kind, e.g. dangerous_call.
    pub kind: String,
    /// 1-based source line.
    pub line: u32,
    /// Source snippet for the finding.
    pub snippet: String,
    /// Detected dangerous callee.
    pub callee: Option<String>,
    /// Taint source snippet.
    pub source: Option<String>,
    /// Taint sink snippet.
    pub sink: Option<String>,
}

impl SecurityFinding {
    pub fn from_core(f: &topos_core::evaluation::security_guidance::SecurityFinding) -> Self {
        SecurityFinding {
            kind: f.kind.clone(),
            line: f.line,
            snippet: f.snippet.clone(),
            callee: f.callee.clone(),
            source: f.source.clone(),
            sink: f.sink.clone(),
        }
    }

    pub fn to_core(&self) -> topos_core::evaluation::security_guidance::SecurityFinding {
        topos_core::evaluation::security_guidance::SecurityFinding {
            kind: self.kind.clone(),
            line: self.line,
            snippet: self.snippet.clone(),
            callee: self.callee.clone(),
            source: self.source.clone(),
            sink: self.sink.clone(),
        }
    }
}

/// A disclosed security finding acknowledged by project config or input.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AcknowledgedRisk {
    pub callee: Option<String>,
    pub kind: String,
    pub line: u32,
    pub snippet: String,
    pub reason: String,
    pub scope: String,
}

/// Function-level complexity diagnostic.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FunctionEntry {
    pub name: String,
    pub line: usize,
    pub complexity: i64,
    /// Dotted scope path, e.g. 'Cls.method.closure'.
    pub qualified_name: Option<String>,
    /// function | async_function | method | closure | module.
    pub kind: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    /// Which probe produced the complexity: 'ast' or 'cfg'.
    pub metric_source: Option<String>,
    /// True when the count includes nested callables' decisions.
    pub includes_nested: Option<bool>,
}

/// One actionable, refactor-focused next step.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Suggestion {
    /// simple | composable | secure | coverage.
    pub pillar: String,
    /// Raw-metric key, or null for finding-derived.
    pub metric: Option<String>,
    /// 'fix' (gate failed) | 'improve' (advisory).
    pub severity: String,
    /// Imperative instruction to act on.
    pub message: String,
}

/// Compact loop-control packet for agentic harnesses.
#[derive(Debug, Clone, Serialize, JsonSchema, Default)]
pub struct AgentContract {
    /// Recommended next MCP tool for the agent loop, if any.
    pub next_tool: Option<String>,
    pub next_actions: Vec<String>,
    pub blocked_by: Vec<String>,
    pub verification_gates: Vec<String>,
    pub risk_flags: Vec<String>,
}

/// One concrete source location for an agent refactor loop.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RefactorTarget {
    pub target_id: String,
    /// function | module | security_call
    pub kind: String,
    pub filepath: String,
    pub symbol: Option<String>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub failing_generators: Vec<String>,
    pub metric: String,
    pub current_value: Option<f64>,
    pub threshold: Option<f64>,
    /// fix | improve
    pub severity: String,
    pub recommended_operations: Vec<String>,
    pub constraints: Vec<String>,
    pub evidence: HashMap<String, Value>,
}

/// Result of a single-unit evaluation on the Medal Podium.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EvaluationResult {
    pub is_parseable: bool,
    pub lattice_element: LatticeElement,
    pub lattice_symbol: String,
    pub lattice_description: String,
    pub dimensions: HashMap<String, LatticeElement>,
    /// Per-dimension normalized score in [0, 100].
    pub scores: HashMap<String, f64>,
    /// Per-pillar breakdown (simple, composable, secure).
    pub pillars: HashMap<String, PillarResult>,
    pub priority: String,
    /// Whether priority was defaulted, inferred from preferences, or explicit.
    pub priority_source: PrioritySource,
    /// Next-step hint for the agent.
    pub guidance: String,
    /// True only when a ModuleDependencyGraph was provided. When false, any
    /// verdict containing COMPOSABLE (including IDEAL) is unreachable.
    pub coupling_available: bool,
    pub raw_metrics: HashMap<String, f64>,
    pub interpretation: HashMap<String, String>,
    /// Source locations for failing complexity gates, keyed by metric.
    pub metric_locations: HashMap<String, Vec<FunctionEntry>>,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
    pub security_findings: Vec<SecurityFinding>,
    pub acknowledged_risks: Vec<AcknowledgedRisk>,
    /// Canonical raw verdict before acknowledged-risk overlay.
    pub raw_lattice_element: Option<LatticeElement>,
    /// Verdict after acknowledged-risk overlay and grade cap.
    pub adjusted_lattice_element: Option<LatticeElement>,
    /// Raw SECURE gate before acknowledged-risk overlay.
    pub secure_raw: Option<bool>,
    /// SECURE gate after acknowledged-risk overlay.
    pub secure_adjusted: Option<bool>,
    /// True when acknowledged risk prevents a top IDEAL grade.
    pub grade_capped: bool,
    /// Actionable, refactor-focused next steps.
    pub suggestions: Vec<Suggestion>,
    /// Present only when the caller supplied `preferences`.
    pub preference_walk: Option<PreferenceWalk>,
    /// Optional ranked edit targets, populated only when requested.
    pub refactor_targets: Vec<RefactorTarget>,
    pub error: Option<String>,
}

impl EvaluationResult {
    /// The Python error-path constructor (SLOP everything).
    pub fn error_result(
        description: &str,
        priority: Priority,
        priority_source: PrioritySource,
        error: String,
    ) -> Self {
        EvaluationResult {
            is_parseable: false,
            lattice_element: LatticeElement::SLOP,
            lattice_symbol: "⊥".to_string(),
            lattice_description: description.to_string(),
            dimensions: HashMap::new(),
            scores: HashMap::new(),
            pillars: HashMap::new(),
            priority: priority_str(priority).to_string(),
            priority_source,
            guidance: String::new(),
            coupling_available: false,
            raw_metrics: HashMap::new(),
            interpretation: HashMap::new(),
            metric_locations: HashMap::new(),
            warnings: Vec::new(),
            agent_contract: None,
            security_findings: Vec::new(),
            acknowledged_risks: Vec::new(),
            raw_lattice_element: None,
            adjusted_lattice_element: None,
            secure_raw: None,
            secure_adjusted: None,
            grade_capped: false,
            suggestions: Vec::new(),
            preference_walk: None,
            refactor_targets: Vec::new(),
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectFileEntry {
    pub filepath: String,
    /// Detected language used to evaluate this file.
    pub language: String,
    pub lattice_element: LatticeElement,
    pub scores: HashMap<String, f64>,
    pub pillars: HashMap<String, PillarResult>,
    pub raw_metrics: HashMap<String, f64>,
    pub warnings: Vec<String>,
    pub security_findings: Vec<SecurityFinding>,
    pub acknowledged_risks: Vec<AcknowledgedRisk>,
    pub raw_lattice_element: Option<LatticeElement>,
    pub adjusted_lattice_element: Option<LatticeElement>,
    pub secure_raw: Option<bool>,
    pub secure_adjusted: Option<bool>,
    pub grade_capped: bool,
    pub is_parseable: bool,
}

/// Per-language project rollup for polyglot directory evaluation.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectLanguageRollup {
    pub language: String,
    pub file_count: usize,
    pub parse_failures: usize,
    pub rolled_up_dimensions: HashMap<String, LatticeElement>,
    pub rolled_up_scores: HashMap<String, f64>,
    pub aggregate_floor_verdict: LatticeElement,
    pub worst_file_path: Option<String>,
    pub worst_file_verdict: Option<LatticeElement>,
}

/// Result of a directory-wide evaluation.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectEvaluationResult {
    pub root: String,
    pub file_count: usize,
    pub parse_failures: usize,
    pub rolled_up_dimensions: HashMap<String, LatticeElement>,
    pub rolled_up_scores: HashMap<String, f64>,
    pub aggregate_floor_verdict: LatticeElement,
    pub language_rollups: Vec<ProjectLanguageRollup>,
    pub aggregate_explanation: String,
    pub worst_file_verdict: Option<LatticeElement>,
    pub worst_files: Vec<ProjectFileEntry>,
    pub guidance: String,
    pub priority: String,
    pub priority_source: PrioritySource,
    pub coupling_available: bool,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
    /// Entries in this page.
    pub count: usize,
    pub offset: usize,
    pub total: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
    pub files: Vec<ProjectFileEntry>,
    pub verbose: bool,
    pub error: Option<String>,
}

/// Result of AST-distance comparison between two programs.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ComparisonResult {
    pub raw_distance: f64,
    pub normalized_distance: f64,
    pub similarity: f64,
    pub operations: HashMap<String, i64>,
    pub source_valid: bool,
    pub target_valid: bool,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// Result of `topos_begin_refactor` — a captured baseline handle.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SnapshotResult {
    /// Opaque handle for this capture; pass to topos_assess_snapshot.
    pub snapshot_id: String,
    /// The file the baseline was captured from.
    pub filepath: String,
    /// sha256 of the baseline source.
    pub baseline_hash: String,
    /// Unix timestamp the snapshot was taken.
    pub created_at: f64,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
    pub error: Option<String>,
}

/// Result of `topos_assess_improvement`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AssessmentResult {
    pub status: AssessmentStatus,
    pub priority: String,
    pub priority_source: PrioritySource,
    pub current: EvaluationResult,
    pub proposed: EvaluationResult,
    /// Change in pillar scores (proposed - current).
    pub score_deltas: HashMap<String, f64>,
    /// Change in raw metrics (proposed - current).
    pub metric_deltas: HashMap<String, f64>,
    pub structural_distance: Option<f64>,
    pub similarity: Option<f64>,
    pub coupling_available_for_proposed: bool,
    /// sha256 of the baseline and current (proposed) source.
    pub baseline_hash: Option<String>,
    pub current_hash: Option<String>,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
    /// Anti-gaming: populated when scores moved but the tree barely changed.
    pub suspicion_reason: Option<String>,
    /// On a regression: a function-scoped unified diff of the single worst
    /// function (largest adverse complexity increase).
    pub regression_diff: Option<String>,
    pub error: Option<String>,
}

/// Result of `topos_inspect_code` — full breakdown.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InspectionResult {
    pub evaluation: EvaluationResult,
    /// Deprecated: function_name -> cyclomatic_complexity (top N only).
    pub functions: HashMap<String, i64>,
    /// Top-N functions with line numbers and cyclomatic complexity.
    pub function_entries: Vec<FunctionEntry>,
    pub total_functions: usize,
    pub entropy_compression_ratio: Option<f64>,
    pub entropy_interpretation: Option<String>,
    pub error: Option<String>,
}

/// Structural test coverage report (v2).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CoverageResult {
    /// [0, 1] average recall across PUT declarations.
    pub mean_declaration_coverage: f64,
    /// Recall per declaration in the PUT.
    pub best_declaration_recall: Vec<f64>,
    /// Source location (file:line) for each declaration.
    pub declaration_locations: Vec<String>,
    /// [0, 1] multiset recall of Statement kinds.
    pub stmt_recall: f64,
    /// [0, 1] multiset recall of Expression kinds.
    pub expr_recall: f64,
    /// [0, 1] average precision across test declarations.
    pub mean_test_precision: f64,
    /// F2 score favoring recall over precision.
    pub f2_score: f64,
    /// [0, 1] kind n-gram path recall.
    pub declaration_path_recall_kgram: f64,
    /// Locations of declarations with incomplete test coverage.
    pub uncovered_declarations: Vec<String>,
    pub put_declaration_count: usize,
    pub test_declaration_count: usize,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// Structured state of the `.gitnexus` dependency-graph index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DepgraphState {
    Missing,
    Present,
    Stale,
    LoadError,
    SchemaMismatch,
    InvalidDir,
    BranchNotIndexed,
}

/// Result of `topos_depgraph_status` — read-only graph state.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DepgraphStatusResult {
    pub state: DepgraphState,
    pub gitnexus_dir: Option<String>,
    pub gitnexus_mtime: Option<f64>,
    pub git_head_mtime: Option<f64>,
    /// True only when the graph loads and is not stale.
    pub coupling_available: bool,
    pub detail: Option<String>,
    pub recommended_next_action: String,
    pub agent_contract: Option<AgentContract>,
    pub error: Option<String>,
}

/// Result of `topos_generate_depgraph`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GenerateDepgraphResult {
    pub ok: bool,
    pub returncode: i32,
    pub gitnexus_dir: Option<String>,
    pub generated: bool,
    pub state_before: Option<DepgraphState>,
    pub message: String,
    pub agent_contract: Option<AgentContract>,
    pub error: Option<String>,
}

/// Per-file before/after summary inside a changeset assessment.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ChangesetFileEntry {
    pub filepath: String,
    pub status: AssessmentStatus,
    /// True when the file did not exist at baseline_ref.
    pub is_new: bool,
    pub baseline_verdict: Option<LatticeElement>,
    pub current_verdict: Option<LatticeElement>,
    pub score_deltas: HashMap<String, f64>,
    pub metric_deltas: HashMap<String, f64>,
    /// True when max function complexity improved but file cyclomatic
    /// complexity worsened — extraction stayed inside one module.
    pub complexity_relocated_within_file: bool,
    pub warnings: Vec<String>,
    pub blocked_by: Option<String>,
    pub error: Option<String>,
}

/// Result of `topos_assess_changeset` — multi-file rollup.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ChangesetResult {
    pub baseline_ref: String,
    pub files: Vec<ChangesetFileEntry>,
    pub project_before: HashMap<String, LatticeElement>,
    pub project_after: HashMap<String, LatticeElement>,
    pub project_scores_before: HashMap<String, f64>,
    pub project_scores_after: HashMap<String, f64>,
    pub aggregate_before: LatticeElement,
    pub aggregate_after: LatticeElement,
    pub project_regression: bool,
    pub complexity_relocated_files: Vec<String>,
    pub coupling_available: bool,
    pub priority: String,
    pub priority_source: PrioritySource,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
    pub error: Option<String>,
}

/// One ranked refactor hotspot row (cycles / dependencies / process / graphify).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RefactorHotspot {
    /// cycle | dependency_edge | process_transition | graphify_orphan | graphify_fragile_edge
    pub kind: String,
    pub label: String,
    pub filepath: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    /// Betti contribution (cycles) or curvature value (dependencies/process,
    /// descending = worse) or degree (graphify orphans, **ascending** = worse
    /// — inverted from curvature's sign convention; see `docs/decisions/refactor-suite.md`.
    pub score: f64,
    pub suggestion: String,
}

/// Result of `topos_refactor`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RefactorResult {
    pub target: String,
    pub filepath: String,
    pub betti_1: Option<usize>,
    pub gitnexus_available: Option<bool>,
    /// Generic external-tool availability, set for every target going
    /// forward (`dependencies`/`process` mirror `gitnexus_available` here
    /// too, for back-compat; `graphify` sets only this field; `cycles`
    /// leaves both `None`, matching the existing `betti_1`-only-for-cycles
    /// precedent).
    pub tool_available: Option<bool>,
    pub hotspots: Vec<RefactorHotspot>,
    pub error: Option<String>,
}

/// Arguments for `topos_generate_graphify_graph` (side-effecting).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateGraphifyInput {
    /// Directory to analyze (default: MCP file root).
    #[serde(default)]
    pub directory: Option<String>,
    /// Regenerate even when current.
    #[serde(default)]
    pub force: bool,
}

/// Result of `topos_generate_graphify_graph`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GenerateGraphifyResult {
    pub ok: bool,
    pub returncode: i32,
    pub graphify_out_dir: Option<String>,
    pub generated: bool,
    pub message: String,
    pub error: Option<String>,
}
