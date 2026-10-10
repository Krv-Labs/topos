//! Structural test-coverage policy (outside `Ω`).
//!
//! The UAST profunctor emits a raw
//! [`DeclarationCoverageReport`]; this module threshold-classifies it
//! into a [`CoverageDecision`] (mean recall, F2, uncovered
//! declarations). Independent of the four quality generators in `Ω`.
//! Defaults live in [`crate::evaluation::policies::calibration`].

use crate::evaluation::policies::calibration::COVERAGE;
use crate::functors::profunctors::uast::structural_test_coverage::DeclarationCoverageReport;

/// Thresholded judgment over a raw declaration coverage report.
///
/// Mirrors `ScoredDecision` (score, achieved, interpretation) but
/// carries coverage-specific fields (F2, uncovered declaration list).
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageDecision {
    pub score: f64,
    pub achieved: bool,
    pub verdict: CoverageVerdict,
    /// Why the verdict is [`CoverageVerdict::Inconclusive`], if it is.
    pub inconclusive_reason: Option<String>,
    pub threshold: f64,
    pub coverage_rate: f64,
    pub f2_score: f64,
    pub uncovered_declarations: Vec<(String, f64)>,
    pub interpretation: std::collections::HashMap<String, String>,
}

/// Coverage verdict. `Inconclusive` means the corpus is implausible
/// (too few test declarations to score), not that coverage is low.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageVerdict {
    Pass,
    Fail,
    Inconclusive,
}

impl CoverageVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }
}

/// `Some(reason)` when the test side has fewer than
/// [`COVERAGE`]`.min_test_declaration_ratio` declarations per source
/// declaration — tests probably live outside the scanned paths, so a
/// PASS/FAIL would be confidently wrong (#336).
pub fn implausible_corpus(report: &DeclarationCoverageReport) -> Option<String> {
    let (source, test) = (report.put_declaration_count, report.test_declaration_count);
    let minimum = source as f64 * COVERAGE.min_test_declaration_ratio;
    (source > 0 && (test as f64) < minimum).then(|| {
        format!(
            "found {test} test declarations for {source} source declarations; \
             tests may live outside the scanned paths"
        )
    })
}

/// Threshold-classify a raw coverage report (mean recall vs
/// `threshold`).
pub fn score_declaration_coverage(
    report: &DeclarationCoverageReport,
    threshold: f64,
) -> CoverageDecision {
    let best_recall = &report.best_declaration_recall;
    let (mean_declaration_coverage, coverage_rate) = if !best_recall.is_empty() {
        let mean = best_recall.iter().sum::<f64>() / best_recall.len() as f64;
        let rate = best_recall.iter().filter(|&&s| s >= threshold).count() as f64
            / best_recall.len() as f64;
        (mean, rate)
    } else {
        (1.0, 1.0)
    };

    let mean_test_precision = report.mean_test_precision;
    let numerator = 5.0 * mean_test_precision * mean_declaration_coverage;
    let denominator = 4.0 * mean_test_precision + mean_declaration_coverage;
    let f2_score = if denominator > 0.0 {
        numerator / denominator
    } else {
        0.0
    };

    let uncovered: Vec<(String, f64)> = report
        .declaration_locations
        .iter()
        .zip(best_recall)
        .filter(|(_, &score)| score < threshold)
        .map(|(loc, &score)| (loc.clone(), score))
        .collect();

    let mut interpretation = std::collections::HashMap::new();
    interpretation.insert(
        "declaration_coverage".to_string(),
        coverage_interpretation(mean_declaration_coverage, threshold),
    );
    interpretation.insert(
        "declaration_coverage_rate".to_string(),
        format!(
            "{:.0}% of declarations meet the {threshold:.2} threshold",
            coverage_rate * 100.0
        ),
    );
    interpretation.insert(
        "declaration_f2_score".to_string(),
        format!("F2 score is {f2_score:.3}"),
    );

    let inconclusive_reason = implausible_corpus(report);
    let achieved = inconclusive_reason.is_none() && mean_declaration_coverage >= threshold;
    let verdict = match (&inconclusive_reason, achieved) {
        (Some(_), _) => CoverageVerdict::Inconclusive,
        (None, true) => CoverageVerdict::Pass,
        (None, false) => CoverageVerdict::Fail,
    };

    CoverageDecision {
        score: mean_declaration_coverage,
        achieved,
        verdict,
        inconclusive_reason,
        threshold,
        coverage_rate,
        f2_score,
        uncovered_declarations: uncovered,
        interpretation,
    }
}

/// Convenience wrapper using [`COVERAGE::declaration_recall`] as the
/// default threshold, matching the Python original's keyword default.
pub fn score_declaration_coverage_default(report: &DeclarationCoverageReport) -> CoverageDecision {
    score_declaration_coverage(report, COVERAGE.declaration_recall)
}

fn coverage_interpretation(score: f64, threshold: f64) -> String {
    if score >= threshold + COVERAGE.strong_offset {
        "coverage is strong".to_string()
    } else if score >= threshold {
        "coverage meets the policy threshold".to_string()
    } else if score >= threshold * COVERAGE.partial_factor {
        "coverage is partial".to_string()
    } else {
        "coverage is weak".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functors::profunctors::uast::structural_test_coverage::declaration_coverage;
    use crate::graphs::ast::dispatch::parse_source;
    use crate::graphs::uast::models::UASTNode;

    fn uast(source: &str) -> UASTNode {
        parse_source(source, "python", None).unwrap().uast_root
    }

    #[test]
    fn decl_coverage_identical_put_and_test_full_scores() {
        let src = "def f(n):\n    for i in range(n):\n        if i % 2:\n            pass\n    return n\n";
        let root = uast(src);
        let rep = declaration_coverage(&[&root], &[&root], 3, false).unwrap();
        let decision = score_declaration_coverage_default(&rep);
        assert!(decision.achieved);
        assert!((decision.coverage_rate - 1.0).abs() < 1e-9);
        assert!((decision.f2_score - 1.0).abs() < 1e-9);
        assert_eq!(decision.uncovered_declarations.len(), 0);
    }

    #[test]
    fn decl_coverage_empty_tests_yield_zero() {
        let put =
            uast("def g():\n    while True:\n        if True:\n            break\n    return 0\n");
        let rep = declaration_coverage(&[&put], &[], 3, true).unwrap();
        let decision = score_declaration_coverage_default(&rep);
        assert!(!decision.achieved);
        assert_eq!(decision.verdict, CoverageVerdict::Inconclusive);
        assert_eq!(decision.coverage_rate, 0.0);
        assert_eq!(decision.f2_score, 0.0);
        assert_eq!(
            decision.uncovered_declarations.len(),
            rep.put_declaration_count
        );
    }

    #[test]
    fn fewer_than_one_test_declaration_per_hundred_is_inconclusive() {
        let report = |source: usize, test: usize| DeclarationCoverageReport {
            mean_declaration_coverage: 0.2,
            best_declaration_recall: vec![0.2; source],
            declaration_locations: vec!["lib.rs:1".to_string(); source],
            stmt_recall: 0.0,
            expr_recall: 0.0,
            mean_test_precision: 0.5,
            declaration_path_recall_kgram: 0.0,
            k: 3,
            put_declaration_count: source,
            test_declaration_count: test,
            include_unknown: false,
        };
        let decision = score_declaration_coverage_default(&report(569, 2));
        assert_eq!(decision.verdict, CoverageVerdict::Inconclusive);
        assert!(!decision.achieved);
        assert_eq!(
            decision.inconclusive_reason.as_deref(),
            Some(
                "found 2 test declarations for 569 source declarations; \
                 tests may live outside the scanned paths"
            )
        );
        let decision = score_declaration_coverage_default(&report(100, 1));
        assert_eq!(decision.verdict, CoverageVerdict::Fail);
        assert_eq!(decision.inconclusive_reason, None);
    }

    #[test]
    fn decl_coverage_vacuous_empty_put_is_achieved() {
        let put = uast("x = 1\ny = x + 2\n");
        let test_src = uast("def test_something(): pass\n");
        let rep = declaration_coverage(&[&put], &[&test_src], 3, true).unwrap();
        let decision = score_declaration_coverage_default(&rep);
        assert!(decision.achieved);
        assert!((decision.coverage_rate - 1.0).abs() < 1e-9);
    }
}
