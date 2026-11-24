use std::collections::BTreeMap;

use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::omega::{EvaluationValue, Generator};

pub(crate) struct MeasuredRollup {
    pub(crate) overall: BTreeMap<String, EvaluationValue>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn measured_rollup(results: &[ClassificationResult]) -> MeasuredRollup {
    let mut overall = CharacteristicMorphism.combine_dimensions(results);
    let scored = results.iter().filter(|result| result.is_parseable).count();
    let mut warnings = Vec::new();
    for pillar in Generator::ALL {
        let unmeasured = unmeasured_files(results, pillar);
        if unmeasured == 0 || !overall.contains_key(pillar.as_str()) {
            continue;
        }
        if overall.get(pillar.as_str()) != Some(&EvaluationValue::Slop) {
            overall.remove(pillar.as_str());
        }
        warnings.push(gap_warning(pillar, unmeasured, scored));
    }
    MeasuredRollup { overall, warnings }
}

fn unmeasured_files(results: &[ClassificationResult], pillar: Generator) -> usize {
    results
        .iter()
        .filter(|result| result.is_parseable && !result.dimensions.contains_key(pillar.as_str()))
        .count()
}

fn gap_warning(pillar: Generator, unmeasured: usize, scored: usize) -> String {
    let name = pillar.as_str().to_ascii_uppercase();
    match pillar {
        Generator::Composable => format!(
            "{name} only partly measured — {unmeasured} of {scored} files had no \
             dependency-graph entry; run `topos depgraph generate` to refresh the graph."
        ),
        _ => format!(
            "{name} only partly measured — {unmeasured} of {scored} files were not scored for it."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(pillars: &[(Generator, bool)]) -> ClassificationResult {
        ClassificationResult {
            is_parseable: true,
            dimensions: pillars
                .iter()
                .map(|(pillar, passed)| {
                    let value = if *passed {
                        pillar.value()
                    } else {
                        EvaluationValue::Slop
                    };
                    (pillar.as_str().to_string(), value)
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_pillar_some_files_were_not_scored_for_is_dropped_with_a_warning() {
        let rollup = measured_rollup(&[
            file(&[(Generator::Simple, true), (Generator::Composable, true)]),
            file(&[(Generator::Simple, true)]),
        ]);
        assert_eq!(
            rollup.overall.keys().collect::<Vec<_>>(),
            ["simple"],
            "an unindexed file must not count as passing COMPOSABLE"
        );
        assert_eq!(
            rollup.warnings,
            [
                "COMPOSABLE only partly measured — 1 of 2 files had no dependency-graph entry; \
              run `topos depgraph generate` to refresh the graph."
            ]
        );
    }

    #[test]
    fn a_measured_failure_stays_a_failure_when_coverage_is_partial() {
        let rollup = measured_rollup(&[
            file(&[(Generator::Navigable, false)]),
            file(&[(Generator::Simple, true)]),
        ]);
        assert_eq!(rollup.overall["navigable"], EvaluationValue::Slop);
        assert!(!rollup.overall.contains_key("simple"));
        assert_eq!(rollup.warnings.len(), 2);
        assert!(rollup.warnings[1].starts_with("NAVIGABLE only partly measured — 1 of 2 files"));
    }

    #[test]
    fn full_coverage_and_unparseable_files_add_no_warning() {
        let unparseable = ClassificationResult::default();
        let rollup = measured_rollup(&[file(&[(Generator::Simple, true)]), unparseable]);
        assert_eq!(rollup.overall["simple"], EvaluationValue::Slop);
        assert!(rollup.warnings.is_empty());
    }
}
