//! Allowlist overlay — the *adjusted* SECURE verdict (anti-gaming design).
//!
//! The core classification pipeline ([`crate::core::characteristic_morphism`])
//! is canonical and untouched: it always produces the raw verdict from the
//! full dangerous-API registry. This module computes an **adjusted** view
//! *on top of* that result by re-counting dangerous calls / taint flows
//! with the allowlisted patterns removed.
//!
//! Both verdicts are always surfaced together, every suppression is
//! disclosed with its mandatory reason, and any active suppression caps
//! the attainable grade below Gold/`IDEAL`. An agent therefore cannot
//! silently hide a finding to inflate the score — only acknowledge it,
//! visibly, and never all the way to the top.
//!
//! # Deviation from the Python original
//!
//! Python's test fixture drives this module through `topos.mcp.evaluation`
//! and `topos.mcp.security_findings` (the MCP server's classification +
//! finding-extraction glue). Those modules live in the Python-only MCP
//! server layer, out of scope for this pure-Rust crate, so the tests here
//! build the [`crate::core::characteristic_morphism::ClassificationResult`]
//! and [`SecurityFinding`]s directly from already-ported primitives
//! ([`crate::core::morphism::ProgramMorphism`],
//! [`crate::core::characteristic_morphism::CharacteristicMorphism`])
//! instead. `apply_allowlist` itself is a straight, behavior-preserving
//! port.

use std::collections::HashSet;
use std::path::Path;

use crate::config::{AllowEntry, ToposConfig};
use crate::core::characteristic_morphism::ClassificationResult;
use crate::core::omega::EvaluationValue;
use crate::evaluation::security_guidance::SecurityFinding;
use crate::functors::probes::cpg::danger::{dangerous_api_reachable, matches_registry};
use crate::functors::probes::cpg::taint::taint_flow_paths;
use crate::graphs::cpg::object::CodePropertyGraph;

/// Raw vs. allowlist-adjusted SECURE verdict for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustedVerdict {
    pub raw_secure_pass: bool,
    pub adjusted_secure_pass: bool,
    pub raw_element: EvaluationValue,
    /// After the grade cap.
    pub adjusted_element: EvaluationValue,
    pub active_findings: Vec<SecurityFinding>,
    pub acknowledged: Vec<(SecurityFinding, AllowEntry)>,
    /// `true` iff `IDEAL` was demoted due to suppression.
    pub grade_capped: bool,
}

impl AdjustedVerdict {
    pub fn suppressions_active(&self) -> bool {
        !self.acknowledged.is_empty()
    }

    pub fn verdict_changed(&self) -> bool {
        self.raw_secure_pass != self.adjusted_secure_pass
    }
}

/// First allow entry whose pattern matches `callee` (suffix-aware).
fn entry_for_callee<'a>(
    callee: Option<&str>,
    entries: &[&'a AllowEntry],
) -> Option<&'a AllowEntry> {
    let callee = callee?;
    entries
        .iter()
        .copied()
        .find(|entry| matches_registry(callee, std::iter::once(entry.pattern.as_str())))
}

/// Overlay `config`'s allowlist onto a canonical classification result.
///
/// `findings` are the raw findings (full registry). `cpg` is used to
/// recompute exact adjusted counts so any display cap on a findings list
/// upstream cannot corrupt the verdict.
pub fn apply_allowlist(
    result: &ClassificationResult,
    findings: &[SecurityFinding],
    config: &ToposConfig,
    file_path: Option<&Path>,
    cpg: Option<&CodePropertyGraph>,
) -> AdjustedVerdict {
    let raw_secure_pass = result.dimensions.get("secure") == Some(&EvaluationValue::Secure);
    let raw_element = result.summary();

    let entries = config.entries_for(file_path);

    // Partition raw findings into acknowledged vs. still-active.
    let mut active: Vec<SecurityFinding> = Vec::new();
    let mut acknowledged: Vec<(SecurityFinding, AllowEntry)> = Vec::new();
    for finding in findings {
        match entry_for_callee(finding.callee.as_deref(), &entries) {
            Some(entry) => acknowledged.push((finding.clone(), entry.clone())),
            None => active.push(finding.clone()),
        }
    }

    // Recompute the adjusted SECURE gate from exact counts (gate == 0).
    let allow_patterns: HashSet<String> = entries.iter().map(|e| e.pattern.clone()).collect();
    let adjusted_secure_pass = match (allow_patterns.is_empty(), cpg) {
        (false, Some(cpg)) => {
            let dangerous = dangerous_api_reachable(cpg, &allow_patterns);
            let taint = taint_flow_paths(cpg, &allow_patterns);
            dangerous == 0 && taint == 0
        }
        _ => raw_secure_pass,
    };

    let mut adjusted_element = recompute_element(result, adjusted_secure_pass);

    // Grade cap: acknowledged risk can never buy the top medal. IDEAL minus
    // the SECURE bit is always SIMPLE_COMPOSABLE (see the Ω encoding in
    // `crate::core::omega`).
    let mut grade_capped = false;
    if !acknowledged.is_empty() && adjusted_element == EvaluationValue::Ideal {
        adjusted_element = EvaluationValue::SimpleComposable;
        grade_capped = true;
    }

    AdjustedVerdict {
        raw_secure_pass,
        adjusted_secure_pass,
        raw_element,
        adjusted_element,
        active_findings: active,
        acknowledged,
        grade_capped,
    }
}

/// Rebuild the `Ω` element from `result`'s dimensions, overriding the
/// SECURE bit with `secure_pass`.
fn recompute_element(result: &ClassificationResult, secure_pass: bool) -> EvaluationValue {
    let simple = result.dimensions.get("simple") == Some(&EvaluationValue::Simple);
    let composable = result.dimensions.get("composable") == Some(&EvaluationValue::Composable);
    crate::core::omega::verdict_from_generators(simple, composable, secure_pass)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::characteristic_morphism::CharacteristicMorphism;
    use crate::core::morphism::ProgramMorphism;
    use crate::evaluation::policies::base::Priority;
    use std::collections::HashMap;
    use std::path::PathBuf;

    const EVAL_SRC: &str = "def f(x):\n    return eval(x)\n";

    fn eval_finding() -> SecurityFinding {
        SecurityFinding {
            kind: "dangerous_call".to_string(),
            line: 2,
            snippet: "return eval(x)".to_string(),
            callee: Some("eval".to_string()),
            source: None,
            sink: None,
        }
    }

    fn classify(source: &str) -> (ClassificationResult, CodePropertyGraph) {
        let mut morphism = ProgramMorphism::new(source, "python");
        let cpg = morphism.build_cpg().unwrap().clone();
        let result =
            CharacteristicMorphism.classify_detailed(&morphism, &[&cpg], Priority::default());
        (result, cpg)
    }

    #[test]
    fn allowlist_flips_secure_pass() {
        let (result, cpg) = classify(EVAL_SRC);
        let config = ToposConfig {
            allow: vec![AllowEntry::new("eval", "trusted REPL")],
            root: None,
        };

        let verdict = apply_allowlist(&result, &[eval_finding()], &config, None, Some(&cpg));

        assert!(!verdict.raw_secure_pass);
        assert!(verdict.adjusted_secure_pass);
        assert!(verdict.active_findings.is_empty());
        assert_eq!(verdict.acknowledged.len(), 1);
        assert_eq!(verdict.acknowledged[0].0.callee.as_deref(), Some("eval"));
        // SECURE is only acknowledged, never a clean pass — no top grade.
        assert_ne!(verdict.adjusted_element, EvaluationValue::Ideal);
    }

    #[test]
    fn grade_is_capped_below_ideal_when_findings_are_acknowledged() {
        // Synthetic result: SIMPLE and COMPOSABLE are already satisfied;
        // SECURE is the only thing standing between this result and IDEAL,
        // and it becomes satisfied only via acknowledgement (the CPG here
        // has no real eval call, so the adjusted count is genuinely zero
        // either way — the point is that the *cap* still applies whenever
        // any finding was acknowledged, regardless of whether the verdict
        // actually flipped).
        let mut morphism = ProgramMorphism::new("def f():\n    return 1\n", "python");
        let cpg = morphism.build_cpg().unwrap().clone();
        let result = ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::from([
                ("simple".to_string(), EvaluationValue::Simple),
                ("composable".to_string(), EvaluationValue::Composable),
                ("secure".to_string(), EvaluationValue::Slop),
            ]),
            lattice_element: EvaluationValue::Slop,
            ..Default::default()
        };

        let config = ToposConfig {
            allow: vec![AllowEntry::new("eval", "trusted")],
            root: None,
        };
        let verdict = apply_allowlist(&result, &[eval_finding()], &config, None, Some(&cpg));

        assert!(verdict.grade_capped);
        assert_eq!(verdict.adjusted_element, EvaluationValue::SimpleComposable);
    }

    #[test]
    fn no_allowlist_leaves_raw_intact() {
        let (result, cpg) = classify(EVAL_SRC);
        let config = ToposConfig::default();

        let verdict = apply_allowlist(&result, &[eval_finding()], &config, None, Some(&cpg));

        assert!(!verdict.raw_secure_pass);
        assert!(!verdict.adjusted_secure_pass);
        assert_eq!(verdict.active_findings.len(), 1);
        assert!(verdict.acknowledged.is_empty());
    }

    #[test]
    fn scope_limits_suppression() {
        let (result, cpg) = classify(EVAL_SRC);
        let root = PathBuf::from("/tmp/topos-suppression-scope-test");
        let config = ToposConfig {
            allow: vec![AllowEntry::new("eval", "ok here").with_scope("experiments/**")],
            root: Some(root.clone()),
        };

        let in_scope = apply_allowlist(
            &result,
            &[eval_finding()],
            &config,
            Some(&root.join("experiments/a.py")),
            Some(&cpg),
        );
        let out_scope = apply_allowlist(
            &result,
            &[eval_finding()],
            &config,
            Some(&root.join("serving/a.py")),
            Some(&cpg),
        );

        assert!(in_scope.adjusted_secure_pass);
        assert!(!out_scope.adjusted_secure_pass);
    }
}
