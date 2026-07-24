//! Characteristic morphism `χ_S : P → Ω`.
//!
//! This module implements the **characteristic morphism** of the
//! program topos. Per the math spec §3, for every program `P ∈ E` and
//! every subprogram `S ↪ P` there exists a unique natural
//! transformation `χ_S : P → Ω` mapping each structural component to an
//! element of `Ω`. This file contains the *map*; the codomain `Ω`
//! itself lives in [`crate::core::omega`].
//!
//! Categorical / Rust correspondence:
//!
//! | Math | Rust |
//! |---|---|
//! | `Ω` | [`crate::core::omega::Omega`] |
//! | elements of `Ω` | [`crate::core::omega::EvaluationValue`] |
//! | `χ_S : P → Ω` | [`CharacteristicMorphism`] |
//! | image of `χ_S(P)` | [`ClassificationResult`] |
//!
//! The characteristic morphism:
//! 1. Builds every available representation (AST + CFG + MDG + CPG) for
//!    the morphism.
//! 2. Groups them by generator (each representation declares its
//!    `dimension()` ∈ `{"simple", "composable", "secure"}`).
//! 3. Runs the matching policy translator `Φᵢ` on the collected metrics
//!    (`simple` → `Φ_SIMPLE`, etc.).
//! 4. Combines the three Boolean truth values via
//!    [`crate::core::omega::verdict_from_generators`] into the final `Ω`
//!    element.
//!
//! `Priority` is recorded on results and steers agent guidance; it does
//! *not* change per-metric pass/fail thresholds inside each `Φᵢ`.
//!
//! # Simplification vs. the Python original
//!
//! Python groups representations by dimension into a generic
//! `dict[str, list[Representation]]` before dispatching. In practice
//! exactly three dimension strings exist across every representation in
//! this crate (`"simple"`, `"composable"`, `"secure"` — including PDG's
//! neutral `"composable"`), so this groups directly into three named
//! buckets instead of a dynamic map keyed by an open-ended string. A
//! representation with a fourth dimension value would have its metrics
//! silently excluded from `raw_metrics` here (Python still records
//! them, just never scores them) — revisit if one is ever added.

use std::collections::HashMap;
use std::fmt;

use crate::core::morphism::ProgramMorphism;
use crate::core::omega::{verdict_from_generators, EvaluationValue};
use crate::evaluation::file_roles::{is_entrypoint_module, is_stable_leaf_module};
use crate::evaluation::policies::base::{Priority, ScoredDecision};
use crate::evaluation::policies::composable::score_coupling;
use crate::evaluation::policies::secure::score_secure;
use crate::evaluation::policies::simple::score_simple;
use crate::graphs::ast::object::AstRepresentation;
use crate::graphs::base::Representation;

/// The image of one program morphism under `χ_S : P → Ω`.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    /// Whether the code parsed successfully.
    pub is_parseable: bool,
    /// Per-generator value in `Ω`: the singleton generator
    /// (SIMPLE/COMPOSABLE/SECURE) when satisfied, SLOP otherwise.
    pub dimensions: HashMap<String, EvaluationValue>,
    /// Per-generator normalized quality score in `[0.0, 1.0]`.
    pub scores: HashMap<String, f64>,
    /// Overall `Ω` element — the join of the satisfied generators.
    pub lattice_element: EvaluationValue,
    /// Generator emphasis label (metadata / guidance).
    pub priority: Priority,
    /// All raw metric floats, namespaced by representation.
    pub raw_metrics: HashMap<String, f64>,
    /// Per-metric interpretation strings.
    pub interpretation: HashMap<String, String>,
    /// Whether the source is an import/export-only entrypoint module
    /// (drives gate exemptions; see [`crate::evaluation::policies::gates`]).
    pub is_entrypoint_module: bool,
    /// Whether the source is a declarations-only "stable leaf" module
    /// (drives the COMPOSABLE Zone-of-Pain exemption; see
    /// [`crate::evaluation::policies::gates`]).
    pub is_stable_leaf_module: bool,
}

impl Default for ClassificationResult {
    fn default() -> Self {
        ClassificationResult {
            is_parseable: false,
            dimensions: HashMap::new(),
            scores: HashMap::new(),
            lattice_element: EvaluationValue::Slop,
            priority: Priority::default(),
            raw_metrics: HashMap::new(),
            interpretation: HashMap::new(),
            is_entrypoint_module: false,
            is_stable_leaf_module: false,
        }
    }
}

impl ClassificationResult {
    /// The overall `Ω` element `χ_S(P)`.
    pub fn summary(&self) -> EvaluationValue {
        self.lattice_element
    }
}

impl fmt::Display for ClassificationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_parseable {
            return write!(f, "Classification: ⊥ SLOP (parse failure)");
        }
        writeln!(f, "Classification: {}", self.lattice_element)?;
        let mut dims: Vec<_> = self.dimensions.iter().collect();
        dims.sort_by_key(|(dim, _)| dim.as_str());
        for (dim, val) in dims {
            let score_pct = self.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
            writeln!(f, "  {dim}: {val}  [{score_pct:.0}%]")?;
        }
        let mut metrics: Vec<_> = self.raw_metrics.iter().collect();
        metrics.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in metrics {
            writeln!(f, "    {k}: {v:.3}")?;
        }
        Ok(())
    }
}

/// `χ_S : P → Ω` — the characteristic morphism of the program topos.
///
/// For every program morphism `P` (and the canonical subprogram `S = P`
/// itself, in the absence of a finer subobject) this computes the
/// natural-transformation image `χ_S(P)` as an [`EvaluationValue`] in `Ω`.
///
/// Each generator `gᵢ` is fed by the representation theory says is the
/// correct lens for that quality:
/// - SIMPLE ← CFG cyclomatic complexity
/// - COMPOSABLE ← ModuleDependencyGraph coupling / instability
/// - SECURE ← Code Property Graph taint / danger probes
#[derive(Debug, Default)]
pub struct CharacteristicMorphism;

impl CharacteristicMorphism {
    /// Return `classify_detailed(...).summary()` — the overall `Ω` element.
    pub fn classify(&self, morphism: &ProgramMorphism) -> EvaluationValue {
        self.classify_detailed(morphism, &[], Priority::default())
            .summary()
    }

    /// Compute `χ_S : P → Ω` in full detail.
    ///
    /// An [`AstRepresentation`] is always built from the morphism (it
    /// carries `ast.entropy` into the SIMPLE generator). Any additional
    /// `representations` (CFG, MDG, PDG, CPG) are grouped by their
    /// `dimension()` and scored independently.
    ///
    /// Parse failures collapse to `⊥ = SLOP`.
    pub fn classify_detailed(
        &self,
        morphism: &ProgramMorphism,
        representations: &[&dyn Representation],
        priority: Priority,
    ) -> ClassificationResult {
        let Some(ast) = morphism.ast.as_ref() else {
            return ClassificationResult {
                priority,
                ..Default::default()
            };
        };
        if !morphism.is_valid() {
            return ClassificationResult {
                priority,
                ..Default::default()
            };
        }

        let ast_rep = AstRepresentation::new(ast, &morphism.source, &ast.uast_root);
        let is_entrypoint = is_entrypoint_module(morphism);
        let is_stable_leaf = is_stable_leaf_module(morphism);
        let source_size_bytes = morphism.source.len() as f64;

        let mut simple_raw = ast_rep.metrics();
        let mut composable_raw: HashMap<String, f64> = HashMap::new();
        let mut secure_raw: HashMap<String, f64> = HashMap::new();
        for rep in representations {
            match rep.dimension() {
                "simple" => simple_raw.extend(rep.metrics()),
                "composable" => composable_raw.extend(rep.metrics()),
                "secure" => secure_raw.extend(rep.metrics()),
                _ => {}
            }
        }

        let mut raw_metrics = HashMap::new();
        raw_metrics.extend(simple_raw.clone());
        raw_metrics.extend(composable_raw.clone());
        raw_metrics.extend(secure_raw.clone());

        let mut dimensions = HashMap::new();
        let mut scores = HashMap::new();
        let mut interpretation = HashMap::new();

        if let Some(decision) = score_simple_dim(&simple_raw, is_entrypoint, source_size_bytes) {
            record(
                &mut dimensions,
                &mut scores,
                &mut interpretation,
                "simple",
                EvaluationValue::Simple,
                decision,
            );
        }
        if let Some(decision) = score_composable_dim(&composable_raw, is_entrypoint, is_stable_leaf)
        {
            record(
                &mut dimensions,
                &mut scores,
                &mut interpretation,
                "composable",
                EvaluationValue::Composable,
                decision,
            );
        }
        if let Some(decision) = score_secure_dim(&secure_raw) {
            record(
                &mut dimensions,
                &mut scores,
                &mut interpretation,
                "secure",
                EvaluationValue::Secure,
                decision,
            );
        }

        let lattice_element = verdict_from_generators(
            dimensions.get("simple") == Some(&EvaluationValue::Simple),
            dimensions.get("composable") == Some(&EvaluationValue::Composable),
            dimensions.get("secure") == Some(&EvaluationValue::Secure),
        );

        ClassificationResult {
            is_parseable: true,
            dimensions,
            scores,
            lattice_element,
            priority,
            raw_metrics,
            interpretation,
            is_entrypoint_module: is_entrypoint,
            is_stable_leaf_module: is_stable_leaf,
        }
    }

    /// Pointwise multi-file meet `⋀_f χ_S(f)`.
    ///
    /// A generator holds for the codebase iff it holds for **every** file
    /// — the lattice meet (`∧`) of the per-file verdicts. Each file's
    /// per-dimension verdict (`dimensions[dim]`) comes from the same hard
    /// gates (`ScoredDecision.achieved`) as its single-file
    /// `lattice_element`, so a one-file codebase agrees exactly with that
    /// file's own classification. Equivalent to `Omega::aggregate` (the
    /// lattice meet) over the per-file `lattice_element`s, decomposed per
    /// generator; the continuous `scores` are advisory and never gate this
    /// rollup.
    ///
    /// An unparseable file satisfies no generator, so it drives every
    /// evaluated dimension to SLOP — a codebase with a file that won't
    /// even compile is not SIMPLE/COMPOSABLE/SECURE. Only dimensions at
    /// least one file actually evaluated are reported.
    pub fn combine_dimensions(
        &self,
        results: &[ClassificationResult],
    ) -> HashMap<String, EvaluationValue> {
        ["simple", "composable", "secure"]
            .into_iter()
            .filter(|dim| results.iter().any(|r| r.dimensions.contains_key(*dim)))
            .map(|dim| {
                let generator = dimension_generator(dim);
                // A file that never evaluated this dimension at all (key
                // absent, e.g. no MDG representation for that file) must not
                // drag the rollup down — only a file that evaluated the
                // dimension and got the SLOP value counts as a failure.
                let satisfied = results.iter().all(|r| {
                    r.is_parseable
                        && match r.dimensions.get(dim) {
                            Some(v) => *v == generator,
                            None => true,
                        }
                });
                let value = if satisfied {
                    generator
                } else {
                    EvaluationValue::Slop
                };
                (dim.to_string(), value)
            })
            .collect()
    }
}

fn record(
    dimensions: &mut HashMap<String, EvaluationValue>,
    scores: &mut HashMap<String, f64>,
    interpretation: &mut HashMap<String, String>,
    dim: &str,
    generator: EvaluationValue,
    decision: ScoredDecision,
) {
    scores.insert(dim.to_string(), decision.score);
    interpretation.extend(decision.interpretation);
    dimensions.insert(
        dim.to_string(),
        if decision.achieved {
            generator
        } else {
            EvaluationValue::Slop
        },
    );
}

fn score_simple_dim(
    raw: &HashMap<String, f64>,
    is_entrypoint_module: bool,
    source_size_bytes: f64,
) -> Option<ScoredDecision> {
    if !raw.contains_key("cfg.cyclomatic")
        && !raw.contains_key("ast.entropy")
        && !raw.contains_key("ast.max_function_complexity")
    {
        return None;
    }
    Some(score_simple(
        raw.get("cfg.cyclomatic").copied(),
        raw.get("ast.entropy").copied(),
        raw.get("ast.max_function_complexity").copied(),
        is_entrypoint_module,
        Some(source_size_bytes),
    ))
}

fn score_composable_dim(
    raw: &HashMap<String, f64>,
    is_entrypoint_module: bool,
    is_stable_leaf_module: bool,
) -> Option<ScoredDecision> {
    if !raw.contains_key("mdg.instability")
        && !raw.contains_key("mdg.fan_in")
        && !raw.contains_key("mdg.fan_out")
    {
        return None;
    }
    Some(score_coupling(
        raw.get("mdg.instability").copied(),
        raw.get("mdg.fan_in").copied(),
        raw.get("mdg.fan_out").copied(),
        raw.get("mdg.abstractness").copied(),
        is_entrypoint_module,
        is_stable_leaf_module,
    ))
}

fn score_secure_dim(raw: &HashMap<String, f64>) -> Option<ScoredDecision> {
    if !raw.contains_key("cpg.dangerous_calls") && !raw.contains_key("cpg.taint_flows") {
        return None;
    }
    Some(score_secure(
        raw.get("cpg.dangerous_calls").copied().unwrap_or(0.0),
        raw.get("cpg.taint_flows").copied().unwrap_or(0.0),
    ))
}

/// Map each *dimension* name to the singleton generator value it
/// produces when satisfied. These three generators are pairwise
/// incomparable in `H`.
fn dimension_generator(dim: &str) -> EvaluationValue {
    match dim {
        "simple" => EvaluationValue::Simple,
        "composable" => EvaluationValue::Composable,
        "secure" => EvaluationValue::Secure,
        _ => EvaluationValue::Slop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_generator_satisfied_for_clean_code() {
        let classifier = CharacteristicMorphism;
        let source = "def process_data(data):\n    result = []\n    for item in data:\n        if item is not None:\n            result.append(item * 2)\n    return result\n";
        let mut morphism = ProgramMorphism::new(source, "python");
        let cfg = morphism.build_cfg().unwrap().clone();

        let result = classifier.classify_detailed(&morphism, &[&cfg], Priority::default());
        assert!(result.is_parseable);
        assert!(result.dimensions.contains_key("simple"));
        let score = result.scores.get("simple").copied().unwrap_or(0.0);
        assert!(score > 0.5, "expected SIMPLE score > 0.5, got {score}");
        assert!((0.0..=1.0).contains(&result.scores["simple"]));
    }

    #[test]
    fn invalid_syntax_collapses_to_slop() {
        let classifier = CharacteristicMorphism;
        let morphism = ProgramMorphism::new("def broken(:", "python");
        assert_eq!(classifier.classify(&morphism), EvaluationValue::Slop);
    }

    #[test]
    fn combine_dimensions_meets_per_file_verdicts() {
        let classifier = CharacteristicMorphism;
        let r1 = ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::from([("simple".to_string(), EvaluationValue::Simple)]),
            scores: HashMap::from([("simple".to_string(), 0.8)]),
            lattice_element: EvaluationValue::Simple,
            ..Default::default()
        };
        let r2 = ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::from([("simple".to_string(), EvaluationValue::Slop)]),
            scores: HashMap::from([("simple".to_string(), 0.3)]),
            lattice_element: EvaluationValue::Slop,
            ..Default::default()
        };
        let combined = classifier.combine_dimensions(&[r1, r2]);
        // r2's SIMPLE verdict is SLOP, so the meet across files is SLOP.
        assert_eq!(combined["simple"], EvaluationValue::Slop);
    }

    #[test]
    fn combine_dimensions_counts_parse_failures_as_simple_slop() {
        let classifier = CharacteristicMorphism;
        let good = ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::from([("simple".to_string(), EvaluationValue::Simple)]),
            scores: HashMap::from([("simple".to_string(), 0.9)]),
            lattice_element: EvaluationValue::Simple,
            ..Default::default()
        };
        let parse_failure = ClassificationResult {
            is_parseable: false,
            ..Default::default()
        };
        let combined = classifier.combine_dimensions(&[good, parse_failure]);
        assert_eq!(combined["simple"], EvaluationValue::Slop);
    }

    #[test]
    fn combine_dimensions_ignores_files_missing_the_representation() {
        let classifier = CharacteristicMorphism;
        let has_repr = ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::from([("composable".to_string(), EvaluationValue::Composable)]),
            scores: HashMap::from([("composable".to_string(), 0.9)]),
            lattice_element: EvaluationValue::Composable,
            ..Default::default()
        };
        // No MDG representation at all for this file: "composable" key is
        // absent, not present-and-failing.
        let missing_repr = ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::new(),
            scores: HashMap::new(),
            lattice_element: EvaluationValue::Slop,
            ..Default::default()
        };
        let combined = classifier.combine_dimensions(&[has_repr, missing_repr]);
        assert_eq!(combined["composable"], EvaluationValue::Composable);
    }

    #[test]
    fn display_mentions_at_least_one_generator_dimension() {
        let classifier = CharacteristicMorphism;
        let morphism = ProgramMorphism::new("x = 1", "python");
        let result = classifier.classify_detailed(&morphism, &[], Priority::default());
        let text = result.to_string();
        assert!(["simple", "composable", "secure"]
            .iter()
            .any(|g| text.contains(g)));
    }
}
