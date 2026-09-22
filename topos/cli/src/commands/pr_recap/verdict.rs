//! Passes C and D: per-file verdicts, the project and added-file
//! rollups, and the headline for the whole range.

use std::collections::BTreeMap;

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue, Generator, Omega};
use topos_engine::evaluation::policies::gates::{pillar_for_metric, GATE_SPECS};

use super::model::*;
use super::score::Scored;

/// Below this, a score dip is noise (0.1 on the displayed 0–100 scale).
const SCORE_REGRESSION_FLOOR: f64 = 0.001;

pub(super) fn finish_statuses(scored: &mut [Scored], clusters: &[Cluster], lattice: &Omega) {
    // A split parent's raw fan-out rises simply because it now imports the
    // files it was carved into. That is not a coupling regression.
    let routed: Vec<&str> = clusters
        .iter()
        .filter(|cluster| {
            cluster
                .parent_fan_out_after_excluding_children
                .zip(cluster.parent_fan_out_before)
                .is_some_and(|(after, before)| after <= before)
        })
        .map(|cluster| cluster.parent.as_str())
        .collect();
    for file in scored.iter_mut() {
        let drop_composable = routed.contains(&file.recap.path.as_str());
        let deltas = score_deltas(&file.before, &file.after, drop_composable);
        let cosmetic = file
            .distance
            .is_some_and(|distance| distance < STRUCTURAL_CHANGE_THRESHOLD)
            && deltas.iter().any(|d| d.abs() >= MEANINGFUL_SCORE_DELTA);
        file.recap.cosmetic = !file.recap.is_new() && cosmetic;
        if drop_composable {
            file.recap
                .hotspots
                .retain(|spot| spot.metric != "mdg.fan_out");
        }
        if file.recap.is_new() {
            let split_child = file
                .recap
                .cluster
                .as_ref()
                .is_some_and(|member| member.role == ClusterRole::Child);
            file.recap.status = new_file_status(&file.after, split_child);
            continue;
        }
        // The verdict has to drop COMPOSABLE too. A parent that trips the
        // fan-out gate loses a pillar outright, and the score deltas are
        // never consulted once the medal itself moved.
        file.recap.status = file_status(
            &file.before,
            &file.after,
            drop_composable,
            cosmetic,
            lattice,
        );
    }
}

/// An added file has no before-medal to move, so it is judged on arrival:
/// landing failing SECURE, or as SLOP, is a regression the change
/// introduced. A split child is left to its cluster's mark instead: code
/// moved out of the parent brings its findings along, and charging them
/// to the child would fail every split of an imperfect file.
fn new_file_status(after: &ClassificationResult, split_child: bool) -> Headline {
    if !after.is_parseable {
        return Headline::LateralMove;
    }
    let slop = measured_verdict(after) == EvaluationValue::Slop;
    let insecure = pillar_measured(after, Generator::Secure.as_str())
        && !pillar_passed(after, Generator::Secure);
    if split_child {
        return if slop {
            Headline::LateralMove
        } else {
            Headline::Improvement
        };
    }
    if slop || insecure {
        Headline::Regression
    } else {
        Headline::Improvement
    }
}

/// An existing file's verdict. `drop_composable` sets COMPOSABLE aside for
/// a split parent whose fan-out was routed into its own children.
fn file_status(
    before: &ClassificationResult,
    after: &ClassificationResult,
    drop_composable: bool,
    suspicious: bool,
    lattice: &Omega,
) -> Headline {
    if !before.is_parseable || !after.is_parseable {
        return Headline::LateralMove;
    }
    let before_verdict = measured_verdict_excluding(before, drop_composable);
    let after_verdict = measured_verdict_excluding(after, drop_composable);
    if before_verdict == after_verdict {
        let deltas = score_deltas(before, after, drop_composable);
        let improved = deltas.iter().any(|d| *d >= SCORE_REGRESSION_FLOOR);
        let regressed = deltas.iter().any(|d| *d <= -SCORE_REGRESSION_FLOOR);
        return if suspicious && improved {
            Headline::SuspiciousNoStructuralChange
        } else if improved && !regressed {
            Headline::ImprovementScore
        } else if regressed && !improved {
            Headline::RegressionScore
        } else {
            Headline::LateralMove
        };
    }
    // `leq(a, b)` means b is at least as good as a. A strict step up is an
    // improvement; a strict step down is a regression; otherwise the two
    // medals cleared different pillars and neither contains the other.
    if lattice.leq(before_verdict, after_verdict) {
        return if suspicious {
            Headline::SuspiciousNoStructuralChange
        } else {
            Headline::Improvement
        };
    }
    if lattice.leq(after_verdict, before_verdict) {
        Headline::Regression
    } else {
        Headline::LateralMove
    }
}

fn score_deltas(
    before: &ClassificationResult,
    after: &ClassificationResult,
    drop_composable: bool,
) -> Vec<f64> {
    Generator::ALL
        .into_iter()
        .filter(|g| !(drop_composable && g.as_str() == "composable"))
        .filter_map(|g| {
            let key = g.as_str();
            Some(after.scores.get(key)? - before.scores.get(key)?)
        })
        .collect()
}

pub(super) fn pillar_deltas(
    before: &ClassificationResult,
    after: &ClassificationResult,
    is_new: bool,
) -> BTreeMap<String, PillarDelta> {
    Generator::ALL
        .into_iter()
        .map(|generator| {
            let key = generator.as_str();
            let measured = pillar_measured(before, key) || pillar_measured(after, key);
            (
                key.to_string(),
                PillarDelta {
                    measured,
                    before_passed: (!is_new && measured).then(|| pillar_passed(before, generator)),
                    after_passed: measured.then(|| pillar_passed(after, generator)),
                    before_score: (!is_new).then(|| rounded_score(before, key)).flatten(),
                    after_score: rounded_score(after, key),
                    lost_gate: (!is_new
                        && pillar_passed(before, generator)
                        && !pillar_passed(after, generator))
                    .then(|| lost_gate(before, after, key))
                    .flatten(),
                },
            )
        })
        .collect()
}

fn pillar_measured(result: &ClassificationResult, pillar: &str) -> bool {
    // `mdg.abstractness` can exist from the file alone. COMPOSABLE's gate
    // needs the dependency graph, which is only attached for a pull request.
    if pillar == "composable" {
        return result.raw_metrics.contains_key("mdg.fan_out");
    }
    result
        .raw_metrics
        .keys()
        .any(|key| pillar_for_metric(key) == pillar)
}

fn pillar_passed(result: &ClassificationResult, generator: Generator) -> bool {
    if !pillar_measured(result, generator.as_str()) {
        return false;
    }
    result
        .dimensions
        .get(generator.as_str())
        .is_some_and(|value| *value == generator.value())
}

/// Medal from the pillars this run actually measured.
///
/// Without a dependency graph, COMPOSABLE is not a pass and not a fail.
/// Counting it as failed would turn every file into a fake regression.
pub(super) fn measured_verdict(result: &ClassificationResult) -> EvaluationValue {
    measured_verdict_excluding(result, false)
}

/// The same verdict with COMPOSABLE optionally set aside, for a split
/// parent whose fan-out rose only because it now imports its own children.
/// Only `file_status` uses this; the reported medal stays factual.
fn measured_verdict_excluding(
    result: &ClassificationResult,
    drop_composable: bool,
) -> EvaluationValue {
    let satisfied: Vec<Generator> = Generator::ALL
        .into_iter()
        .filter(|generator| !(drop_composable && generator.as_str() == "composable"))
        .filter(|generator| pillar_passed(result, *generator))
        .collect();
    verdict_from_generators(&satisfied)
}

fn lost_gate(
    before: &ClassificationResult,
    after: &ClassificationResult,
    pillar: &str,
) -> Option<String> {
    before
        .raw_metrics
        .keys()
        .chain(after.raw_metrics.keys())
        .filter(|key| pillar_for_metric(key) == pillar)
        .find(|key| {
            let was = before.raw_metrics.get(*key).copied().unwrap_or(0.0);
            let now = after.raw_metrics.get(*key).copied().unwrap_or(0.0);
            now > was && now > gate_limit(key).unwrap_or(f64::MAX)
        })
        .cloned()
}

/// The upper bound of a gate that can fail its pillar, straight from the
/// engine's registry so this cannot drift from what `evaluate` enforces.
pub(super) fn gate_limit(metric: &str) -> Option<f64> {
    GATE_SPECS
        .iter()
        .find(|spec| spec.metric == metric && spec.gates_achieved)
        .and_then(|spec| spec.high)
}

fn rounded_score(result: &ClassificationResult, pillar: &str) -> Option<f64> {
    result
        .scores
        .get(pillar)
        .map(|score| (score * 1000.0).round() / 10.0)
}

pub(super) fn complexity_relocated(
    before: &ClassificationResult,
    after: &ClassificationResult,
) -> bool {
    let func = metric_delta(before, after, "ast.max_function_complexity");
    let file = metric_delta(before, after, "cfg.cyclomatic");
    func < 0.0 && file > 0.0
}

pub(super) fn metric_delta(
    before: &ClassificationResult,
    after: &ClassificationResult,
    key: &str,
) -> f64 {
    after.raw_metrics.get(key).copied().unwrap_or(0.0)
        - before.raw_metrics.get(key).copied().unwrap_or(0.0)
}

pub(super) fn raw(result: &ClassificationResult, key: &str) -> Option<usize> {
    result.raw_metrics.get(key).map(|value| *value as usize)
}

pub(super) fn medal(value: EvaluationValue) -> Medal {
    Medal {
        symbol: value.symbol().to_string(),
        tier: value.medal_tier().to_string(),
        verdict: value.name().to_string(),
    }
}

/// A pillar is achieved only if every file that measures it passes it.
/// Vacuous truth is excluded: a pillar nobody measured is not achieved.
///
/// Both sides cover the same existing files. Added files have no before
/// side; counting them only at head would move the mean without any file
/// getting better (see [`added_rollup`]).
pub(super) fn project_rollup(files: &[FileRecap]) -> Option<ProjectRollup> {
    let existing: Vec<&FileRecap> = files.iter().filter(|file| !file.is_new()).collect();
    if existing.is_empty() {
        return None;
    }
    let mut pillars = BTreeMap::new();
    let mut before_achieved = Vec::new();
    let mut after_achieved = Vec::new();
    for generator in Generator::ALL {
        let key = generator.as_str();
        // `measured` sets both `before_passed` and `after_passed` on an
        // existing file, so one filter gives one population.
        let measured: Vec<&PillarDelta> = existing
            .iter()
            .filter_map(|file| file.pillars.get(key))
            .filter(|delta| delta.measured)
            .collect();
        if measured.is_empty() {
            continue;
        }
        let before_passed = measured.iter().all(|d| d.before_passed == Some(true));
        let after_passed = measured.iter().all(|d| d.after_passed == Some(true));
        if before_passed {
            before_achieved.push(generator);
        }
        if after_passed {
            after_achieved.push(generator);
        }
        // No score on a side (nothing parsed there) is left out, not 0%.
        let (Some(before_score), Some(after_score)) = (
            mean(measured.iter().filter_map(|d| d.before_score)),
            mean(measured.iter().filter_map(|d| d.after_score)),
        ) else {
            continue;
        };
        pillars.insert(
            key.to_string(),
            PillarRollup {
                before_passed,
                after_passed,
                before_score,
                after_score,
                files_before: measured.len(),
                files_after: measured.len(),
                failing_before: measured
                    .iter()
                    .filter(|d| d.before_passed == Some(false))
                    .count(),
                failing_after: measured
                    .iter()
                    .filter(|d| d.after_passed == Some(false))
                    .count(),
            },
        );
    }
    let regression = Generator::ALL.into_iter().any(|generator| {
        before_achieved.contains(&generator) && !after_achieved.contains(&generator)
    });
    Some(ProjectRollup {
        medal_before: medal(verdict_from_generators(&before_achieved)),
        medal_after: medal(verdict_from_generators(&after_achieved)),
        pillars,
        regression,
        files_before: existing.len(),
        files_after: existing.len(),
    })
}

/// The added files on their own, at head: same achievement rule as
/// [`project_rollup`], no before side.
pub(super) fn added_rollup(files: &[FileRecap]) -> Option<AddedRollup> {
    let added: Vec<&FileRecap> = files.iter().filter(|file| file.is_new()).collect();
    if added.is_empty() {
        return None;
    }
    let mut pillars = BTreeMap::new();
    let mut achieved = Vec::new();
    for generator in Generator::ALL {
        let key = generator.as_str();
        let measured: Vec<&PillarDelta> = added
            .iter()
            .filter_map(|file| file.pillars.get(key))
            .filter(|delta| delta.after_passed.is_some())
            .collect();
        if measured.is_empty() {
            continue;
        }
        let passed = measured.iter().all(|d| d.after_passed == Some(true));
        if passed {
            achieved.push(generator);
        }
        let Some(score) = mean(measured.iter().filter_map(|d| d.after_score)) else {
            continue;
        };
        pillars.insert(
            key.to_string(),
            AddedPillar {
                passed,
                score,
                files: measured.len(),
                failing: measured
                    .iter()
                    .filter(|d| d.after_passed == Some(false))
                    .count(),
            },
        );
    }
    Some(AddedRollup {
        files: added.len(),
        medal: medal(verdict_from_generators(&achieved)),
        pillars,
    })
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<f64> = values.collect();
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

pub(super) fn headline_for(
    files: &[FileRecap],
    clusters: &[Cluster],
    base: &str,
    head: &str,
) -> (Headline, String) {
    if files.is_empty() {
        let reason = if base == head {
            // `base` is the merge-base, so this also covers a head that is
            // already contained in the base branch.
            "The head adds no commits on top of the base. Uncommitted edits need --head :worktree."
                .to_string()
        } else if head == "worktree" {
            "The working tree has no supported source changes.".to_string()
        } else {
            "No supported source files changed.".to_string()
        };
        return (Headline::LateralMove, reason);
    }
    // A passing new file has no before-medal, so it cannot make an existing
    // file's lateral move into an improvement, and it cannot hide one
    // either. A new file that arrives failing is judged like changed code.
    let existing: Vec<&FileRecap> = files.iter().filter(|file| !file.is_new()).collect();
    let judged: Vec<&FileRecap> = if existing.is_empty() {
        files.iter().collect()
    } else {
        files
            .iter()
            .filter(|file| !file.is_new() || file.status.fails_check())
            .collect()
    };
    // Worst measured file wins. A mixed change is not an improvement.
    let worst = judged
        .iter()
        .min_by_key(|file| file.status.rank())
        .expect("judged is non-empty");
    let best = judged
        .iter()
        .max_by_key(|file| file.status.rank())
        .expect("judged is non-empty");
    let unanimous = best.status.rank() == worst.status.rank();
    let headline = match worst.status {
        Headline::SuspiciousNoStructuralChange => Headline::SuspiciousNoStructuralChange,
        Headline::Regression => Headline::Regression,
        Headline::RegressionScore => Headline::RegressionScore,
        Headline::Improvement if unanimous => Headline::Improvement,
        Headline::ImprovementScore if unanimous => Headline::ImprovementScore,
        _ => Headline::LateralMove,
    };
    // A failed split counts as a lost pillar would: it outranks every
    // file status short of a regression, which keeps its own reason.
    if headline.rank() > Headline::Regression.rank() {
        if let Some(cluster) = clusters
            .iter()
            .find(|cluster| cluster.mark == ClusterMark::Fail)
        {
            let why = cluster
                .reasons
                .first()
                .map_or(String::new(), |reason| format!(": {reason}"));
            return (
                Headline::Regression,
                format!("The split of {} failed{why}.", cluster.parent),
            );
        }
    }
    let reason = match headline {
        Headline::SuspiciousNoStructuralChange => format!(
            "{} moved its score while the syntax tree barely changed.",
            worst.path
        ),
        Headline::Regression if worst.is_new() => new_file_reason(worst),
        Headline::Regression => format!("{} lost a structural pillar.", worst.path),
        Headline::RegressionScore => {
            format!(
                "{} kept its medal, but a pillar score went down.",
                worst.path
            )
        }
        Headline::Improvement => format!(
            "{} cleared a structural pillar it missed before.",
            worst.path
        ),
        Headline::ImprovementScore => {
            format!("{} kept its medal and improved a pillar score.", worst.path)
        }
        Headline::LateralMove => lateral_reason(&existing),
    };
    (headline, reason)
}

fn new_file_reason(file: &FileRecap) -> String {
    let insecure = file
        .pillars
        .get(Generator::Secure.as_str())
        .is_some_and(|delta| delta.after_passed == Some(false));
    if insecure {
        format!("{} is new and fails SECURE.", file.path)
    } else {
        format!("{} is new and passes no pillar (SLOP).", file.path)
    }
}

/// A lateral move is a mixed or flat change. Say what actually happened to
/// the existing files instead of claiming they all held, which is false as
/// soon as one of them went up.
fn lateral_reason(existing: &[&FileRecap]) -> String {
    if existing.is_empty() {
        return "New files arrived; no existing file was compared.".to_string();
    }
    let up = existing
        .iter()
        .filter(|file| {
            matches!(
                file.status,
                Headline::Improvement | Headline::ImprovementScore
            )
        })
        .count();
    let held = existing.len() - up;
    if up == 0 {
        return "Existing files kept their medals.".to_string();
    }
    format!(
        "{up} existing file{} improved, {held} held; no pillar was lost.",
        if up == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A split parent's fan-out rises only because it imports its own
    /// children: COMPOSABLE must be set aside for the verdict too, not
    /// only for the score deltas.
    #[test]
    fn a_routed_fan_out_does_not_read_as_a_regression() {
        fn side(composable: bool) -> ClassificationResult {
            let mut result = ClassificationResult {
                is_parseable: true,
                ..Default::default()
            };
            for generator in Generator::ALL {
                let key = generator.as_str().to_string();
                let passing = generator.as_str() != "composable" || composable;
                result.dimensions.insert(
                    key.clone(),
                    if passing {
                        generator.value()
                    } else {
                        EvaluationValue::Slop
                    },
                );
                result.scores.insert(key, if passing { 1.0 } else { 0.0 });
            }
            result.raw_metrics.insert("mdg.fan_out".to_string(), 1.0);
            result.raw_metrics.insert("cfg.cyclomatic".to_string(), 1.0);
            result
                .raw_metrics
                .insert("cpg.dangerous_calls".to_string(), 0.0);
            result
                .raw_metrics
                .insert("nav.max_function_divergence".to_string(), 0.0);
            result
        }
        let (before, after) = (side(true), side(false));
        let lattice = Omega::default();
        let status =
            |drop_composable: bool| file_status(&before, &after, drop_composable, false, &lattice);
        assert_eq!(status(false), Headline::Regression);
        assert_ne!(status(true), Headline::Regression);
    }

    #[test]
    fn gate_limits_come_from_the_engine_registry() {
        use topos_engine::evaluation::policies::calibration::{COMPOSABLE, SECURE};
        assert_eq!(
            gate_limit("cpg.dangerous_calls"),
            Some(SECURE.max_dangerous_calls)
        );
        assert_eq!(gate_limit("cpg.taint_flows"), Some(SECURE.max_taint_flows));
        assert_eq!(gate_limit("mdg.fan_out"), Some(COMPOSABLE.max_fan_out));
        // Advisory gates cannot fail a pillar, so they cannot be the lost one.
        assert_eq!(gate_limit("mdg.instability"), None);
        assert_eq!(gate_limit("cfg.cyclomatic"), None);
    }
}
