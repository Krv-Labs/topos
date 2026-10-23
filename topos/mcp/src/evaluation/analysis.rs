//! Process-wide reuse of source-local analysis for repeated assessments.
//!
//! Assessments parse the same baseline over and over while only the proposal
//! changes. Graph evidence, allowlists, preferences, and final conclusions
//! must stay fresh, but the source-local half — AST/CFG/PDG/CPG metrics,
//! entrypoint/stable-leaf flags, node-kind sequence, and the CPG needed for
//! security evidence — is a pure function of the exact source bytes and parser
//! context. This module caches that immutable product only; it never caches
//! a [`ClassificationResult`], rendered output, or a graph-adjusted verdict.
//!
//! The cache is process-local, byte-bounded (not just entry-count bounded),
//! and only locks around lookup/insertion/eviction. Parsing, graph work,
//! analysis, filesystem access, and rendering all happen outside the lock.
//! Duplicate concurrent misses are acceptable; there is no singleflight.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::graphs::base::Representation;
use topos_engine::graphs::cpg::models::{CPGEdge, CPGNode};
use topos_engine::graphs::cpg::object::CodePropertyGraph;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_engine::graphs::uast::models::{AttributeValue, UASTNode};

use super::classify::classify_morphism;

/// Default retained-byte budget for the process-wide analysis cache.
///
/// Measured representative entries (small files with a CPG) land in the tens
/// of KiB; 64 MiB holds thousands of them while staying well below the
/// memory an agent loop should ever need for this optimization.
pub(crate) const DEFAULT_ANALYSIS_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Estimated allocator bookkeeping for one owned String/Vec/map allocation.
const ALLOCATION_OVERHEAD: usize = 24;
/// Estimated per-entry hash/B-tree bucket bookkeeping.
const MAP_ENTRY_OVERHEAD: usize = 64;
/// Estimated cache-key bookkeeping beyond `size_of::<AnalysisKey>()`.
const CACHE_KEY_OVERHEAD: usize = 64;
/// Estimated cache-value bookkeeping beyond `size_of::<CacheEntry>()`.
const CACHE_ENTRY_OVERHEAD: usize = 64;

/// Parser context: preserve the caller's actual parse mode.
///
/// [`ParserContext::Memory`] is exactly [`ProgramMorphism::new`];
/// [`ParserContext::Path`] is exactly [`ProgramMorphism::with_path`]. The
/// assessment tools currently use memory mode even for `.tsx` paths, and
/// this type deliberately does not change that.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ParserContext {
    Memory {
        language: String,
    },
    /// Reserved for callers that parse with a path-aware grammar. The
    /// assessment tools intentionally use [`ParserContext::Memory`] even for
    /// `.tsx` paths; tests cover this variant.
    #[allow(dead_code)]
    Path {
        language: String,
        path: PathBuf,
    },
}

impl ParserContext {
    pub(crate) fn memory(language: impl Into<String>) -> Self {
        ParserContext::Memory {
            language: language.into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn path(language: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        ParserContext::Path {
            language: language.into(),
            path: path.into(),
        }
    }

    fn build_morphism(&self, source: &str) -> ProgramMorphism {
        match self {
            ParserContext::Memory { language } => ProgramMorphism::new(source, language),
            ParserContext::Path { language, path } => {
                ProgramMorphism::with_path(source, language, path)
            }
        }
    }
}

/// SECURE evidence captured from the source-local CPG.
///
/// Only the CPG is cached. Sighthound scans the file named by
/// `security_path` rather than the passed source, so findings are not a
/// pure function of the source and must be rebuilt against the current
/// path on every overlay.
#[derive(Debug, Clone)]
pub(crate) struct SecurityEvidence {
    pub(crate) cpg: CodePropertyGraph,
}

/// Owned immutable product of source-local analysis.
///
/// Contains no [`ModuleDependencyGraph`] readings, no graph-adjusted
/// verdict, no [`ClassificationResult`], and no [`ProgramMorphism`] (which
/// carries `Cell`s and trait objects and is not shareable analysis state).
#[derive(Debug, Clone)]
pub(crate) struct SourceAnalysis {
    /// Exact source bytes this product describes.
    pub(crate) source: String,
    /// Effective AST language (`uast_root.lang`), empty on parse failure.
    pub(crate) language: String,
    /// Whether the source parsed into a valid AST.
    pub(crate) is_parseable: bool,
    /// Length of `source` in bytes.
    pub(crate) source_size_bytes: f64,
    /// Source-local entrypoint flag (filename hint + source shape).
    pub(crate) is_entrypoint_module: bool,
    /// Source-local stable-leaf flag.
    pub(crate) is_stable_leaf_module: bool,
    /// Source-local raw metrics: AST + CFG + PDG + CPG + NAV + abstractness.
    /// Never contains a `ModuleDependencyGraph` reading.
    pub(crate) raw_metrics: BTreeMap<String, f64>,
    /// Depth-first tree-sitter node-kind sequence, for AST edit distance.
    pub(crate) ast_kinds: Vec<String>,
    /// CPG evidence, present only when the SECURE gate failed and an overlay
    /// could therefore need it. Findings are rebuilt against the current
    /// security path on every overlay.
    pub(crate) security: Option<SecurityEvidence>,
}

impl SourceAnalysis {
    /// Estimated retained bytes (source, metrics, kind sequence, security
    /// evidence), used for the cache's byte budget.
    pub(crate) fn estimated_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>();
        bytes = bytes.saturating_add(estimate_string(&self.source));
        bytes = bytes.saturating_add(estimate_string(&self.language));
        bytes = bytes.saturating_add(
            self.raw_metrics
                .keys()
                .map(|key| {
                    std::mem::size_of::<String>()
                        + key.capacity()
                        + std::mem::size_of::<f64>()
                        + MAP_ENTRY_OVERHEAD
                        + ALLOCATION_OVERHEAD
                })
                .sum::<usize>(),
        );
        bytes = bytes.saturating_add(
            self.ast_kinds.capacity() * std::mem::size_of::<String>()
                + ALLOCATION_OVERHEAD
                + self.ast_kinds.iter().map(estimate_string).sum::<usize>(),
        );
        if let Some(security) = &self.security {
            bytes = bytes.saturating_add(estimate_cpg_bytes(&security.cpg));
        }
        bytes
    }
}

fn estimate_string(value: &String) -> usize {
    value.capacity().saturating_add(ALLOCATION_OVERHEAD)
}

fn estimate_attributes(attributes: &HashMap<String, AttributeValue>) -> usize {
    let mut bytes = attributes.capacity()
        * (std::mem::size_of::<String>() + std::mem::size_of::<AttributeValue>())
        + attributes.len() * MAP_ENTRY_OVERHEAD;
    if attributes.capacity() > 0 {
        bytes = bytes.saturating_add(ALLOCATION_OVERHEAD);
    }
    for (key, value) in attributes {
        bytes = bytes.saturating_add(estimate_string(key));
        if let AttributeValue::Str(value) = value {
            bytes = bytes.saturating_add(estimate_string(value));
        }
    }
    bytes
}

/// Estimate the heap owned by a UAST node's strings, attributes, and child
/// wrappers. The node's own struct is accounted for by its container.
fn estimate_uast_owned_bytes(node: &UASTNode) -> usize {
    let mut bytes = estimate_string(&node.kind)
        .saturating_add(estimate_string(&node.lang))
        .saturating_add(estimate_string(&node.id));
    if let Some(file) = &node.span.file {
        bytes = bytes.saturating_add(estimate_string(file));
    }
    bytes = bytes
        .saturating_add(estimate_string(&node.native.parser))
        .saturating_add(estimate_string(&node.native.parser_version))
        .saturating_add(estimate_string(&node.native.node_kind))
        .saturating_add(estimate_attributes(&node.attributes));
    bytes = bytes.saturating_add(node.children.capacity() * std::mem::size_of::<UASTNode>());
    if node.children.capacity() > 0 {
        bytes = bytes.saturating_add(ALLOCATION_OVERHEAD);
    }
    for child in &node.children {
        bytes = bytes.saturating_add(estimate_uast_owned_bytes(child));
    }
    bytes
}

fn estimate_cpg_bytes(cpg: &CodePropertyGraph) -> usize {
    let mut bytes = estimate_string(&cpg.source)
        .saturating_add(estimate_string(&cpg.language))
        .saturating_add(
            cpg.nodes.capacity() * (std::mem::size_of::<String>() + std::mem::size_of::<CPGNode>()),
        )
        .saturating_add(cpg.nodes.len() * MAP_ENTRY_OVERHEAD)
        .saturating_add(cpg.edges.capacity() * std::mem::size_of::<CPGEdge>());
    if cpg.nodes.capacity() > 0 || cpg.edges.capacity() > 0 {
        bytes = bytes.saturating_add(ALLOCATION_OVERHEAD);
    }
    for (id, node) in &cpg.nodes {
        bytes = bytes
            .saturating_add(estimate_string(id))
            .saturating_add(estimate_uast_owned_bytes(&node.uast));
    }
    for edge in &cpg.edges {
        bytes = bytes
            .saturating_add(estimate_string(&edge.source))
            .saturating_add(estimate_string(&edge.target))
            .saturating_add(estimate_string(&edge.label));
    }
    bytes
}

/// Whether a classification needs SECURE evidence at all.
///
/// Mirrors the diagnostics overlay guard: parseable source whose CPG gate
/// failed. An allowlist cannot conjure an overlay where the raw gate passed.
pub(crate) fn security_evidence_needed(result: &ClassificationResult) -> bool {
    result.is_parseable
        && (result
            .raw_metrics
            .get("cpg.dangerous_calls")
            .copied()
            .unwrap_or(0.0)
            > 0.0
            || result
                .raw_metrics
                .get("cpg.taint_flows")
                .copied()
                .unwrap_or(0.0)
                > 0.0)
}

/// Classify an analysis against the *current* graph and priority.
///
/// Cached local metrics are cloned, then the graph's readings are added only
/// when the target file has a `File` node — the exact same filter
/// [`classify_morphism`](super::classify_morphism) applies. Nothing
/// graph-derived is ever stored in the cache.
pub(crate) fn classify_from_analysis(
    analysis: &SourceAnalysis,
    dep_graph: Option<&ModuleDependencyGraph>,
    priority: Priority,
) -> ClassificationResult {
    if !analysis.is_parseable {
        return ClassificationResult {
            priority,
            ..Default::default()
        };
    }
    let mut raw_metrics = analysis.raw_metrics.clone();
    if let Some(dep_graph) = dep_graph.filter(|graph| graph.file_node_id().is_some()) {
        raw_metrics.extend(dep_graph.metrics());
    }
    CharacteristicMorphism::classify_raw_metrics(
        raw_metrics,
        &analysis.language,
        analysis.is_entrypoint_module,
        analysis.is_stable_leaf_module,
        analysis.source_size_bytes,
        priority,
    )
}

// ---------------------------------------------------------------------------
// Process-wide cache
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct CacheCounters {
    hits: u64,
    misses: u64,
    evictions: u64,
    oversized: u64,
}

/// Cache counters, entries, retained bytes, and budget. Internal only:
/// never attached to a tool response.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnalysisCacheStats {
    pub(crate) hits: u64,
    pub(crate) misses: u64,
    pub(crate) evictions: u64,
    pub(crate) oversized: u64,
    pub(crate) entries: usize,
    pub(crate) retained_bytes: usize,
    pub(crate) budget_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AnalysisKey {
    source_hash: [u8; 32],
    source_len: usize,
    parser: ParserContext,
    /// The cached CPG itself is path-independent, but path-scoped SECURE
    /// evidence must never share entries across different security paths.
    /// Findings are rebuilt on every overlay; these fields keep any future
    /// path-dependent evidence separated.
    security_path: Option<PathBuf>,
    sighthound_enabled: bool,
}

impl AnalysisKey {
    fn new(source: &str, parser: &ParserContext, security_path: Option<&Path>) -> Self {
        let source_hash: [u8; 32] = Sha256::digest(source.as_bytes()).into();
        AnalysisKey {
            source_hash,
            source_len: source.len(),
            parser: parser.clone(),
            security_path: security_path.map(Path::to_path_buf),
            sighthound_enabled: sighthound_enabled(),
        }
    }

    fn estimated_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>().saturating_add(CACHE_KEY_OVERHEAD);
        match &self.parser {
            ParserContext::Memory { language } => {
                bytes = bytes.saturating_add(estimate_string(language));
            }
            ParserContext::Path { language, path } => {
                bytes = bytes
                    .saturating_add(estimate_string(language))
                    .saturating_add(path.capacity())
                    .saturating_add(ALLOCATION_OVERHEAD);
            }
        }
        if let Some(path) = &self.security_path {
            bytes = bytes
                .saturating_add(path.capacity())
                .saturating_add(ALLOCATION_OVERHEAD);
        }
        bytes
    }
}

fn estimated_entry_bytes(key: &AnalysisKey, analysis: &SourceAnalysis) -> usize {
    key.estimated_bytes()
        .saturating_add(analysis.estimated_bytes())
        .saturating_add(std::mem::size_of::<CacheEntry>())
        .saturating_add(CACHE_ENTRY_OVERHEAD)
}

struct CacheEntry {
    analysis: Arc<SourceAnalysis>,
    last_used: u64,
    bytes: usize,
}

struct AnalysisCacheState {
    entries: HashMap<AnalysisKey, CacheEntry>,
    tick: u64,
    retained_bytes: usize,
    budget_bytes: usize,
    counters: CacheCounters,
}

impl AnalysisCacheState {
    fn new(budget_bytes: usize) -> Self {
        AnalysisCacheState {
            entries: HashMap::new(),
            tick: 0,
            retained_bytes: 0,
            budget_bytes,
            counters: CacheCounters {
                hits: 0,
                misses: 0,
                evictions: 0,
                oversized: 0,
            },
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn lookup(&mut self, key: &AnalysisKey) -> Option<Arc<SourceAnalysis>> {
        let tick = self.next_tick();
        let entry = self.entries.get_mut(key)?;
        entry.last_used = tick;
        self.counters.hits += 1;
        Some(Arc::clone(&entry.analysis))
    }

    fn record_miss(&mut self) {
        self.counters.misses += 1;
    }

    fn insert(&mut self, key: AnalysisKey, analysis: Arc<SourceAnalysis>, bytes: usize) {
        if bytes > self.budget_bytes {
            self.counters.oversized += 1;
            return;
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.retained_bytes = self.retained_bytes.saturating_sub(previous.bytes);
        }
        let tick = self.next_tick();
        self.entries.insert(
            key.clone(),
            CacheEntry {
                analysis,
                last_used: tick,
                bytes,
            },
        );
        self.retained_bytes = self.retained_bytes.saturating_add(bytes);
        self.evict_until_within_budget(&key);
    }

    fn evict_until_within_budget(&mut self, keep: &AnalysisKey) {
        while self.retained_bytes > self.budget_bytes {
            let victim = self
                .entries
                .iter()
                .filter(|(key, _)| *key != keep)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            let Some(victim) = victim else {
                break;
            };
            if let Some(removed) = self.entries.remove(&victim) {
                self.retained_bytes = self.retained_bytes.saturating_sub(removed.bytes);
                self.counters.evictions += 1;
            }
        }
    }

    #[cfg(test)]
    fn stats(&self) -> AnalysisCacheStats {
        AnalysisCacheStats {
            hits: self.counters.hits,
            misses: self.counters.misses,
            evictions: self.counters.evictions,
            oversized: self.counters.oversized,
            entries: self.entries.len(),
            retained_bytes: self.retained_bytes,
            budget_bytes: self.budget_bytes,
        }
    }
}

static ANALYSIS_CACHE: OnceLock<Mutex<AnalysisCacheState>> = OnceLock::new();

fn analysis_cache() -> &'static Mutex<AnalysisCacheState> {
    ANALYSIS_CACHE
        .get_or_init(|| Mutex::new(AnalysisCacheState::new(DEFAULT_ANALYSIS_CACHE_BUDGET_BYTES)))
}

#[cfg(test)]
static CACHE_BYPASS: AtomicBool = AtomicBool::new(false);

/// Serializes tests that clear the process-wide cache or assert its
/// counters, so they cannot observe each other's inserts/evictions.
#[cfg(test)]
pub(crate) static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Test/benchmark-only cache bypass. Production callers use
/// [`analyze_source`].
#[cfg(test)]
pub(crate) fn set_analysis_cache_bypass(enabled: bool) {
    CACHE_BYPASS.store(enabled, Ordering::Relaxed);
}

#[inline]
fn cache_bypassed() -> bool {
    #[cfg(test)]
    {
        CACHE_BYPASS.load(Ordering::Relaxed)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn sighthound_enabled() -> bool {
    !std::env::var("TOPOS_DISABLE_SIGHTHOUND").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Analyze `source` through the process-wide cache.
///
/// The lock is held only for lookup, counter updates, insertion, and
/// eviction. The analysis itself runs after the lookup lock is released.
pub(crate) fn analyze_source(
    source: &str,
    parser: ParserContext,
    security_path: Option<&Path>,
) -> Arc<SourceAnalysis> {
    if cache_bypassed() {
        return Arc::new(analyze_source_uncached(source, &parser, security_path));
    }

    let key = AnalysisKey::new(source, &parser, security_path);
    if let Ok(mut cache) = analysis_cache().lock() {
        if let Some(hit) = cache.lookup(&key) {
            return hit;
        }
        cache.record_miss();
    }

    let analysis = Arc::new(analyze_source_uncached(source, &parser, security_path));
    let bytes = estimated_entry_bytes(&key, &analysis);
    if let Ok(mut cache) = analysis_cache().lock() {
        cache.insert(key, Arc::clone(&analysis), bytes);
    }
    analysis
}

/// Analyze `source` without consulting or populating the cache.
pub(crate) fn analyze_source_uncached(
    source: &str,
    parser: &ParserContext,
    _security_path: Option<&Path>,
) -> SourceAnalysis {
    let mut morphism = parser.build_morphism(source);
    let local = classify_morphism(&mut morphism, Priority::default(), None);
    let ast_kinds = morphism
        .ast
        .as_ref()
        .map(|ast| {
            ast.traverse()
                .iter()
                .map(|node| node.kind().to_string())
                .collect()
        })
        .unwrap_or_default();
    let security = if security_evidence_needed(&local) {
        morphism.take_cpg().map(|cpg| SecurityEvidence { cpg })
    } else {
        None
    };

    SourceAnalysis {
        source: source.to_string(),
        language: local.language.clone(),
        is_parseable: local.is_parseable,
        source_size_bytes: source.len() as f64,
        is_entrypoint_module: local.is_entrypoint_module,
        is_stable_leaf_module: local.is_stable_leaf_module,
        raw_metrics: local.raw_metrics,
        ast_kinds,
        security,
    }
}

/// Clear all cached analyses and reset counters. The configured budget is
/// preserved.
#[cfg(test)]
pub(crate) fn clear_analysis_cache() {
    if let Ok(mut cache) = analysis_cache().lock() {
        cache.entries.clear();
        cache.retained_bytes = 0;
        cache.tick = 0;
        cache.counters = CacheCounters::default();
    }
}

/// Process-wide cache statistics (internal; not exposed to MCP/CLI).
#[cfg(test)]
pub(crate) fn analysis_cache_stats() -> AnalysisCacheStats {
    analysis_cache()
        .lock()
        .map(|cache| cache.stats())
        .unwrap_or(AnalysisCacheStats {
            hits: 0,
            misses: 0,
            evictions: 0,
            oversized: 0,
            entries: 0,
            retained_bytes: 0,
            budget_bytes: DEFAULT_ANALYSIS_CACHE_BUDGET_BYTES,
        })
}

/// Test-only budget override, so retention/eviction can be exercised
/// without allocating 64 MiB.
#[cfg(test)]
pub(crate) fn set_analysis_cache_budget_for_tests(bytes: usize) {
    if let Ok(mut cache) = analysis_cache().lock() {
        cache.budget_bytes = bytes;
        cache.evict_until_within_budget(&AnalysisKey {
            source_hash: [0; 32],
            source_len: 0,
            parser: ParserContext::memory(""),
            security_path: None,
            sighthound_enabled: false,
        });
    }
}

#[cfg(test)]
pub(crate) fn analysis_cache_contains(
    source: &str,
    parser: &ParserContext,
    security_path: Option<&Path>,
) -> bool {
    analysis_cache()
        .lock()
        .map(|cache| {
            cache
                .entries
                .contains_key(&AnalysisKey::new(source, parser, security_path))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use topos_engine::graphs::mdg::models::{GraphNode, GraphRelationship};
    use topos_engine::graphs::mdg::object::ModuleDependencyGraph;

    fn parser() -> ParserContext {
        ParserContext::memory("python")
    }

    fn analyze(source: &str) -> Arc<SourceAnalysis> {
        analyze_source(source, parser(), None)
    }

    fn same_classification(left: &ClassificationResult, right: &ClassificationResult) {
        assert_eq!(left.is_parseable, right.is_parseable);
        assert_eq!(left.dimensions, right.dimensions);
        assert_eq!(left.scores, right.scores);
        assert_eq!(left.gate_scores, right.gate_scores);
        assert_eq!(left.lattice_element, right.lattice_element);
        assert_eq!(left.priority, right.priority);
        assert_eq!(left.raw_metrics, right.raw_metrics);
        assert_eq!(left.interpretation, right.interpretation);
        assert_eq!(left.advisories, right.advisories);
        assert_eq!(left.language, right.language);
        assert_eq!(left.is_entrypoint_module, right.is_entrypoint_module);
        assert_eq!(left.is_stable_leaf_module, right.is_stable_leaf_module);
    }

    fn same_analysis(left: &SourceAnalysis, right: &SourceAnalysis) {
        assert_eq!(left.source, right.source);
        assert_eq!(left.language, right.language);
        assert_eq!(left.is_parseable, right.is_parseable);
        assert_eq!(left.source_size_bytes, right.source_size_bytes);
        assert_eq!(left.is_entrypoint_module, right.is_entrypoint_module);
        assert_eq!(left.is_stable_leaf_module, right.is_stable_leaf_module);
        assert_eq!(left.raw_metrics, right.raw_metrics);
        assert_eq!(left.ast_kinds, right.ast_kinds);
        match (&left.security, &right.security) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                assert_eq!(a.cpg.source, b.cpg.source);
                assert_eq!(a.cpg.language, b.cpg.language);
                assert_eq!(a.cpg.nodes.len(), b.cpg.nodes.len());
                assert_eq!(a.cpg.edges.len(), b.cpg.edges.len());
            }
            _ => panic!("only one side has security evidence"),
        }
    }

    fn node(id: &str, label: &str, file_path: Option<&str>) -> GraphNode {
        let mut properties = HashMap::new();
        if let Some(path) = file_path {
            properties.insert(
                "filePath".to_string(),
                serde_json::Value::String(path.to_string()),
            );
        }
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            properties,
        }
    }

    fn relationship(id: &str, source: &str, target: &str, rel_type: &str) -> GraphRelationship {
        GraphRelationship {
            id: id.to_string(),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: rel_type.to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties: HashMap::new(),
        }
    }

    /// A graph whose target file owns one symbol calling `fan_out` external
    /// callees.
    fn graph_with_fan_out(fan_out: usize) -> ModuleDependencyGraph {
        let mut graph = ModuleDependencyGraph::new("m.py");
        graph.add_node(node("File:m.py", "File", Some("m.py")));
        graph.add_node(node("Function:m.py:f", "Function", None));
        graph.add_relationship(relationship(
            "defines",
            "File:m.py",
            "Function:m.py:f",
            "DEFINES",
        ));
        for index in 0..fan_out {
            graph.add_relationship(relationship(
                &format!("call{index}"),
                "Function:m.py:f",
                &format!("Function:other{index}:g"),
                "CALLS",
            ));
        }
        graph
    }

    #[test]
    fn cache_hit_reuses_and_content_change_misses() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let source = "def cached_value(x):\n    return x + 1\n";
        let first = analyze(source);
        let second = analyze(source);
        assert!(
            Arc::ptr_eq(&first, &second),
            "identical source and parser context must reuse the analysis"
        );

        let changed = analyze("def cached_value(x):\n    return x + 2\n");
        assert!(
            !Arc::ptr_eq(&first, &changed),
            "different source bytes must miss"
        );
        let stats = analysis_cache_stats();
        assert!(stats.hits >= 1);
        assert!(stats.entries >= 2);
        assert!(stats.retained_bytes <= stats.budget_bytes);
    }

    #[test]
    fn mtime_change_with_same_content_still_hits() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module.py");
        let source = "def stable(x):\n    return x\n";
        std::fs::write(&path, source).unwrap();
        let parser = ParserContext::path("python", &path);
        let first = analyze_source(source, parser.clone(), None);

        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        let second = analyze_source(source, parser, None);
        assert!(
            Arc::ptr_eq(&first, &second),
            "mtime is not part of the cache identity"
        );
    }

    #[test]
    fn parser_context_changes_do_not_collide() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let source = "def f(x):\n    return x\n";
        let memory = analyze_source(source, ParserContext::memory("python"), None);
        let path_a = analyze_source(source, ParserContext::path("python", "a.py"), None);
        let path_b = analyze_source(source, ParserContext::path("python", "b.py"), None);
        assert!(!Arc::ptr_eq(&memory, &path_a));
        assert!(!Arc::ptr_eq(&path_a, &path_b));
    }

    #[test]
    fn tsx_path_mode_preserves_the_parser_difference() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let tsx = "const el = <Foo />;\nexport function f(a: boolean) { return a ? 1 : 0 }\n";
        let memory = analyze_source(tsx, ParserContext::memory("typescript"), None);
        assert!(
            !memory.is_parseable,
            "assessment's memory mode keeps parsing this as plain TypeScript"
        );

        let path = analyze_source(tsx, ParserContext::path("typescript", "Widget.tsx"), None);
        assert!(path.is_parseable, "path mode selects the TSX grammar");

        let ts = analyze_source(tsx, ParserContext::path("typescript", "Widget.ts"), None);
        assert!(!ts.is_parseable);
    }

    #[test]
    fn cached_and_bypass_analysis_are_equivalent() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let sources = [
            "def f(x):\n    return x\n",
            "def broken(:\n",
            "import os\n\ndef run(cmd):\n    return os.system(cmd)\n",
        ];
        for source in sources {
            let cached = analyze(source);
            let bypass = analyze_source_uncached(source, &parser(), None);
            same_analysis(&cached, &bypass);

            let cached_class = classify_from_analysis(&cached, None, Priority::Simple);
            let bypass_class = classify_from_analysis(&bypass, None, Priority::Simple);
            same_classification(&cached_class, &bypass_class);
        }
    }

    #[test]
    fn classification_from_analysis_matches_classify_morphism() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let sources = [
            (
                "python",
                "def f(x):\n    if x:\n        return x\n    return 0\n",
            ),
            (
                "python",
                "import os\n\ndef run(cmd):\n    return os.system(cmd)\n",
            ),
            ("rust", "pub fn f(x: i32) -> i32 {\n    x + 1\n}\n"),
            (
                "typescript",
                "export function f(a: boolean) { return a ? 1 : 0 }\n",
            ),
        ];
        let graph = graph_with_fan_out(12);
        for (language, source) in sources {
            let parser = ParserContext::memory(language);
            let analysis = analyze_source(source, parser.clone(), None);
            for dep_graph in [None, Some(&graph)] {
                let expected = classify_morphism(
                    &mut parser.build_morphism(source),
                    Priority::Navigable,
                    dep_graph,
                );
                let actual = classify_from_analysis(&analysis, dep_graph, Priority::Navigable);
                same_classification(&expected, &actual);
            }
        }
    }

    #[test]
    fn graph_changes_conclusions_while_local_analysis_hits() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let source = "def f(x):\n    return x\n";
        let analysis = analyze(source);

        let absent = classify_from_analysis(&analysis, None, Priority::Simple);
        assert!(
            !absent.dimensions.contains_key("composable"),
            "no graph means COMPOSABLE is not fabricated"
        );

        let heavy = graph_with_fan_out(12);
        let with_graph = classify_from_analysis(&analysis, Some(&heavy), Priority::Simple);
        assert_eq!(with_graph.raw_metrics["mdg.fan_out"], 12.0);
        assert_eq!(
            with_graph.dimensions.get("composable"),
            Some(&topos_engine::core::omega::EvaluationValue::Slop),
            "fan-out 12 exceeds the COMPOSABLE gate"
        );
        assert_ne!(
            with_graph.dimensions, absent.dimensions,
            "the graph adds a COMPOSABLE verdict the no-graph path cannot fabricate"
        );

        let rebuilt = graph_with_fan_out(0);
        let after_rebuild = classify_from_analysis(&analysis, Some(&rebuilt), Priority::Simple);
        assert_eq!(after_rebuild.raw_metrics["mdg.fan_out"], 0.0);
        assert_eq!(
            after_rebuild.dimensions.get("composable"),
            Some(&topos_engine::core::omega::EvaluationValue::Composable),
            "the rebuilt graph's fresh metrics change the conclusion"
        );

        let hit_again = analyze(source);
        assert!(
            Arc::ptr_eq(&analysis, &hit_again),
            "graph changes never touch the source-local cache"
        );
    }

    #[test]
    fn allowlist_and_priority_apply_fresh_on_hits() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let source = "def f(expr):\n    return eval(expr)\n";
        let analysis = analyze(source);
        let hit = analyze(source);
        assert!(Arc::ptr_eq(&analysis, &hit));

        let simple = classify_from_analysis(&analysis, None, Priority::Simple);
        let secure = classify_from_analysis(&analysis, None, Priority::Secure);
        assert_eq!(simple.raw_metrics, secure.raw_metrics);
        assert_eq!(simple.priority, Priority::Simple);
        assert_eq!(secure.priority, Priority::Secure);

        let no_allow = crate::diagnostics::overlay_from_analysis(&analysis, &secure, None, &[])
            .expect("a SECURE-failing source has an overlay");
        assert!(
            no_allow.acknowledged_risks.is_empty(),
            "nothing is acknowledged without an allowlist"
        );

        let allow = vec!["eval".to_string()];
        let allowed = crate::diagnostics::overlay_from_analysis(&analysis, &secure, None, &allow)
            .expect("the same cached evidence still applies");
        assert!(
            allowed
                .acknowledged_risks
                .iter()
                .any(|risk| risk.callee.as_deref() == Some("eval")),
            "the hit must apply the current allowlist"
        );
        assert!(
            !allowed.verdict.acknowledged.is_empty(),
            "the EVAL finding is acknowledged on this hit"
        );
    }

    #[test]
    fn path_scanned_findings_are_rebuilt_on_each_overlay() {
        if std::env::var("TOPOS_DISABLE_SIGHTHOUND")
            .is_ok_and(|value| !value.is_empty() && value != "0")
        {
            return;
        }
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module.py");
        let source = "def f(expr):\n    return eval(expr)\n";
        std::fs::write(
            &path,
            "import os\n\ndef f(cmd):\n    return os.system(cmd)\n",
        )
        .unwrap();

        let analysis = analyze_source(source, parser(), Some(&path));
        let result = classify_from_analysis(&analysis, None, Priority::Secure);
        let first = crate::diagnostics::overlay_from_analysis(&analysis, &result, Some(&path), &[])
            .expect("the eval source still needs an overlay");
        assert!(
            first
                .active_findings
                .iter()
                .any(|finding| finding.callee.as_deref() == Some("os.system")),
            "the first overlay must reflect the on-disk file: {:?}",
            first.active_findings
        );

        std::fs::write(&path, source).unwrap();
        let second =
            crate::diagnostics::overlay_from_analysis(&analysis, &result, Some(&path), &[])
                .expect("the cached CPG remains usable");
        assert!(
            second
                .active_findings
                .iter()
                .any(|finding| finding.callee.as_deref() == Some("eval")),
            "the hit must rescan the changed on-disk file: {:?}",
            second.active_findings
        );
    }

    #[test]
    fn parse_failure_reuse_is_deterministic() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let first = analyze("def broken(:\n");
        let second = analyze("def broken(:\n");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!first.is_parseable);
        assert!(first.raw_metrics.is_empty());
        assert!(first.security.is_none());

        let class = classify_from_analysis(&first, None, Priority::Composable);
        assert!(!class.is_parseable);
        assert_eq!(class.priority, Priority::Composable);
        assert!(class.dimensions.is_empty());
    }

    fn stub_analysis(source: &str) -> Arc<SourceAnalysis> {
        Arc::new(SourceAnalysis {
            source: source.to_string(),
            language: "python".to_string(),
            is_parseable: true,
            source_size_bytes: source.len() as f64,
            is_entrypoint_module: false,
            is_stable_leaf_module: false,
            raw_metrics: BTreeMap::new(),
            ast_kinds: Vec::new(),
            security: None,
        })
    }

    fn stub_key(source: &str) -> AnalysisKey {
        AnalysisKey::new(source, &parser(), None)
    }

    struct BudgetGuard(usize);

    impl Drop for BudgetGuard {
        fn drop(&mut self) {
            set_analysis_cache_budget_for_tests(self.0);
        }
    }

    fn cpg_owned_string_floor(cpg: &CodePropertyGraph) -> usize {
        cpg.source.capacity()
            + cpg.language.capacity()
            + cpg
                .nodes
                .iter()
                .map(|(id, node)| id.capacity() + uast_owned_string_floor(&node.uast))
                .sum::<usize>()
            + cpg
                .edges
                .iter()
                .map(|edge| edge.source.capacity() + edge.target.capacity() + edge.label.capacity())
                .sum::<usize>()
    }

    fn uast_owned_string_floor(node: &UASTNode) -> usize {
        node.kind.capacity()
            + node.lang.capacity()
            + node.id.capacity()
            + node.span.file.as_ref().map_or(0, String::capacity)
            + node.native.parser.capacity()
            + node.native.parser_version.capacity()
            + node.native.node_kind.capacity()
            + node
                .attributes
                .iter()
                .map(|(key, value)| {
                    key.capacity()
                        + match value {
                            AttributeValue::Bool(_) => 0,
                            AttributeValue::Str(value) => value.capacity(),
                        }
                })
                .sum::<usize>()
            + node
                .children
                .iter()
                .map(uast_owned_string_floor)
                .sum::<usize>()
    }

    #[test]
    fn lru_eviction_keeps_retained_bytes_within_budget() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let one = stub_analysis("aaaa");
        let key_one = stub_key("aaaa");
        let entry_bytes = estimated_entry_bytes(&key_one, &one);
        let mut state = AnalysisCacheState::new(entry_bytes * 2 + 8);

        state.insert(key_one.clone(), Arc::clone(&one), entry_bytes);
        let key_two = stub_key("bbbb");
        let two = stub_analysis("bbbb");
        let two_bytes = estimated_entry_bytes(&key_two, &two);
        state.insert(key_two.clone(), two, two_bytes);
        assert_eq!(state.entries.len(), 2);

        // Touch key one so key two becomes the least recently used.
        assert!(state.lookup(&key_one).is_some());
        let key_three = stub_key("cccc");
        let three = stub_analysis("cccc");
        let three_bytes = estimated_entry_bytes(&key_three, &three);
        state.insert(key_three.clone(), three, three_bytes);

        assert!(state.entries.contains_key(&key_one));
        assert!(!state.entries.contains_key(&key_two));
        assert!(state.entries.contains_key(&key_three));
        assert_eq!(state.counters.evictions, 1);
        assert!(state.retained_bytes <= state.budget_bytes);

        let stats = state.stats();
        assert_eq!(stats.entries, 2);
        assert!(stats.retained_bytes <= stats.budget_bytes);
    }

    #[test]
    fn oversized_key_is_budgeted_before_insertion() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let original = analysis_cache_stats().budget_bytes;
        let _budget = BudgetGuard(original);
        set_analysis_cache_budget_for_tests(1024);
        clear_analysis_cache();

        let source = "x = 1\n";
        let language = "x".repeat(8 * 1024);
        let _analysis = analyze_source(source, ParserContext::memory(language), None);

        let stats = analysis_cache_stats();
        assert_eq!(stats.entries, 0, "an oversized key must not be retained");
        assert_eq!(stats.retained_bytes, 0);
        assert_eq!(stats.oversized, 1);
        assert!(stats.retained_bytes <= stats.budget_bytes);
    }

    #[test]
    fn security_bearing_analysis_accounts_for_cpg_payload() {
        let parser = ParserContext::memory("python");
        let mut source = String::from("import os\n\n");
        for index in 0..30 {
            source.push_str(&format!(
                "def f_{index}(cmd):\n    if cmd:\n        return os.system(cmd)\n    return {index}\n\n"
            ));
        }
        let analysis = analyze_source_uncached(&source, &parser, None);
        let cpg = &analysis
            .security
            .as_ref()
            .expect("the os.system sample has CPG security evidence")
            .cpg;
        assert!(cpg.nodes.len() > 100);
        let floor = cpg_owned_string_floor(cpg);
        let key = AnalysisKey::new(&source, &parser, None);
        let weight = estimated_entry_bytes(&key, &analysis);
        assert!(
            weight >= floor,
            "estimated weight {weight} is below CPG-owned strings {floor}"
        );

        let mut state = AnalysisCacheState::new(weight.saturating_sub(1));
        state.insert(key, Arc::new(analysis), weight);
        assert!(state.entries.is_empty());
        assert_eq!(state.counters.oversized, 1);
        assert_eq!(state.retained_bytes, 0);
    }

    #[test]
    fn budget_setter_round_trips_without_changing_the_default() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let original = analysis_cache_stats().budget_bytes;
        set_analysis_cache_budget_for_tests(original);
        assert_eq!(analysis_cache_stats().budget_bytes, original);
        set_analysis_cache_budget_for_tests(DEFAULT_ANALYSIS_CACHE_BUDGET_BYTES);
        assert_eq!(
            analysis_cache_stats().budget_bytes,
            DEFAULT_ANALYSIS_CACHE_BUDGET_BYTES
        );
    }

    #[test]
    fn oversized_entries_bypass_the_cache() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let sample = stub_analysis("oversized");
        let key = stub_key("oversized");
        let entry_bytes = estimated_entry_bytes(&key, &sample);
        let mut state = AnalysisCacheState::new(entry_bytes.saturating_sub(1));
        state.insert(key, sample, entry_bytes);

        assert!(state.entries.is_empty());
        assert_eq!(state.counters.oversized, 1);
        assert_eq!(state.retained_bytes, 0);
        assert!(state.retained_bytes <= state.budget_bytes);
    }

    #[test]
    fn concurrent_analysis_calls_are_safe() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let source = "def concurrent(x):\n    return x * 2\n";
        let results = std::thread::scope(|scope| {
            (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        (0..5)
                            .map(|_| analyze(source))
                            .collect::<Vec<Arc<SourceAnalysis>>>()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .flat_map(|handle| handle.join().expect("analysis thread must not panic"))
                .collect::<Vec<_>>()
        });

        assert_eq!(results.len(), 40);
        let expected = &results[0];
        for result in &results {
            assert_eq!(result.raw_metrics, expected.raw_metrics);
            assert_eq!(result.ast_kinds, expected.ast_kinds);
        }
        let stats = analysis_cache_stats();
        assert!(stats.entries >= 1);
        assert!(stats.retained_bytes <= stats.budget_bytes);
    }

    /// Compare the pre-cache parse/classify/distance path with the new cold
    /// path.
    ///
    /// AST-kind collection replaces the distance traversal the original path
    /// performs later, so both sides include the distance. The cold-cache
    /// side uses a unique source per iteration, exercising real misses
    /// (hash, lock, and insert) without a test-only clear in the hot loop.
    #[test]
    #[ignore]
    fn bench_cold_analysis_overhead() {
        use std::hint::black_box;
        use std::time::Instant;
        use topos_engine::functors::profunctors::ast::compare::{
            calculate_ast_distance, calculate_ast_distance_from_kinds,
        };

        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        clear_analysis_cache();
        let body = (0..30)
            .map(|index| {
                format!(
                    "def f_{index}(x):\n    if x:\n        return x + {index}\n    return {index}\n\n"
                )
            })
            .collect::<String>();
        const ITERATIONS: usize = 100;
        let baselines: Vec<String> = (0..ITERATIONS)
            .map(|index| format!("{body}# cold baseline {index}\n"))
            .collect();
        let proposals: Vec<String> = (0..ITERATIONS)
            .map(|index| format!("{body}# cold proposal {index}\n    return 1\n"))
            .collect();
        let parser = parser();

        let start = Instant::now();
        for (baseline, proposed) in baselines.iter().zip(&proposals) {
            let mut base_morphism = ProgramMorphism::new(baseline, "python");
            let _ = black_box(classify_morphism(
                &mut base_morphism,
                Priority::Simple,
                None,
            ));
            let mut proposed_morphism = ProgramMorphism::new(proposed, "python");
            let _ = black_box(classify_morphism(
                &mut proposed_morphism,
                Priority::Simple,
                None,
            ));
            black_box(calculate_ast_distance(
                base_morphism.ast.as_ref().expect("baseline AST"),
                proposed_morphism.ast.as_ref().expect("proposal AST"),
            ));
        }
        let original = start.elapsed();

        let start = Instant::now();
        for (baseline, proposed) in baselines.iter().zip(&proposals) {
            let base_analysis = analyze_source_uncached(baseline, &parser, None);
            black_box(classify_from_analysis(
                &base_analysis,
                None,
                Priority::Simple,
            ));
            let proposed_analysis = analyze_source_uncached(proposed, &parser, None);
            black_box(classify_from_analysis(
                &proposed_analysis,
                None,
                Priority::Simple,
            ));
            black_box(calculate_ast_distance_from_kinds(
                &base_analysis.ast_kinds,
                &proposed_analysis.ast_kinds,
            ));
        }
        let cold = start.elapsed();

        let start = Instant::now();
        for (baseline, proposed) in baselines.iter().zip(&proposals) {
            let base_analysis = analyze_source(baseline, parser.clone(), None);
            black_box(classify_from_analysis(
                &base_analysis,
                None,
                Priority::Simple,
            ));
            let proposed_analysis = analyze_source(proposed, parser.clone(), None);
            black_box(classify_from_analysis(
                &proposed_analysis,
                None,
                Priority::Simple,
            ));
            black_box(calculate_ast_distance_from_kinds(
                &base_analysis.ast_kinds,
                &proposed_analysis.ast_kinds,
            ));
        }
        let cold_cached = start.elapsed();
        clear_analysis_cache();

        println!(
            "cold analysis: iterations={ITERATIONS}, original_ms={:.3}, \
             new_uncached_ms={:.3}, new_cold_cache_ms={:.3}, \
             uncached_overhead_pct={:.1}, cache_store_overhead_pct={:.1}",
            original.as_secs_f64() * 1000.0,
            cold.as_secs_f64() * 1000.0,
            cold_cached.as_secs_f64() * 1000.0,
            (cold.as_secs_f64() / original.as_secs_f64() - 1.0) * 100.0,
            (cold_cached.as_secs_f64() / cold.as_secs_f64() - 1.0) * 100.0,
        );
    }
}
