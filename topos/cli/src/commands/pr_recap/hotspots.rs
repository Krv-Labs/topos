//! Where to look: the functions and calls that pushed a metric over its
//! gate, in one canonical order.

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::evaluation::security_guidance::remediation_for;
use topos_engine::functors::probes::ast::complexity::FunctionComplexityEntry;
use topos_engine::functors::probes::ast::divergence::calculate_function_divergence_entries;
use topos_mcp::schemas::SecurityFinding;

use super::gates::advice_for;
use super::model::{FileRecap, Hotspot};
use super::score::Side;
use super::verdict::{gate_limit, metric_delta};

/// The range's first `cap` hotspots in their one canonical order, which
/// renderers rely on, and how many there were in all, so a capped list
/// can say `N more`. The order is by metric (dangerous call, function
/// complexity, nesting divergence, fan-out), then by file. Each file's
/// own list is already in metric order, so a stable sort keeps the files
/// in place.
pub(super) fn top_hotspots(files: &[FileRecap], cap: usize) -> (Vec<Hotspot>, usize) {
    let mut ranked: Vec<&Hotspot> = files.iter().flat_map(|f| f.hotspots.iter()).collect();
    ranked.sort_by_key(|spot| hotspot_rank(&spot.metric));
    let total = ranked.len();
    (ranked.into_iter().take(cap).cloned().collect(), total)
}

fn hotspot_rank(metric: &str) -> u8 {
    match metric {
        DANGEROUS_CALLS => 0,
        FUNCTION_COMPLEXITY => 1,
        FUNCTION_DIVERGENCE => 2,
        FAN_OUT => 3,
        _ => 4,
    }
}

const DANGEROUS_CALLS: &str = "cpg.dangerous_calls";

pub(super) const FUNCTION_COMPLEXITY: &str = "ast.max_function_complexity";

const FUNCTION_DIVERGENCE: &str = "nav.max_function_divergence";

const FAN_OUT: &str = "mdg.fan_out";

/// A file's hotspots, in [`hotspot_rank`] order: each metric that rose
/// over the change and now sits over its gate, pointed at the function or
/// call that put it there. An added file is compared against nothing: its
/// before side is empty and scores no metric, so anything over a gate at
/// head is a hotspot.
pub(super) fn file_hotspots(
    path: &str,
    worst: Option<&FunctionComplexityEntry>,
    before: &mut Side,
    after: &mut Side,
) -> Vec<Hotspot> {
    let hotspot = |line: usize, function: Option<&str>, metric: &str, detail: String| Hotspot {
        path: path.to_string(),
        line,
        function: function.map(str::to_string),
        metric: metric.to_string(),
        detail,
        advice: advice_for(metric).unwrap_or_default().to_string(),
    };
    let mut hotspots = Vec::new();
    // A line split can make one call look like two snippets, so only a
    // rise in the scored count has a new finding to point at.
    if over_gate(&before.result, &after.result, DANGEROUS_CALLS).is_some() {
        if let Some(finding) = new_security_finding(before, after) {
            let (advice, _) = remediation_for(&finding.to_core());
            hotspots.push(Hotspot {
                advice,
                ..hotspot(
                    finding.line as usize,
                    None,
                    DANGEROUS_CALLS,
                    format!(
                        "dangerous call {}",
                        finding.callee.as_deref().unwrap_or("unknown")
                    ),
                )
            });
        }
    }
    if let (Some(gate), Some(worst)) = (
        over_gate(&before.result, &after.result, FUNCTION_COMPLEXITY),
        worst,
    ) {
        hotspots.push(hotspot(
            worst.start_line,
            Some(&worst.name),
            FUNCTION_COMPLEXITY,
            format!(
                "{} complexity is {}, gate is {}",
                worst.name, worst.complexity, gate as i64
            ),
        ));
    }
    if let (Some(gate), Some(ast)) = (
        over_gate(&before.result, &after.result, FUNCTION_DIVERGENCE),
        after.ast(),
    ) {
        if let Some(worst) = calculate_function_divergence_entries(&ast.uast_root, after.source())
            .into_iter()
            .max_by(|a, b| a.divergence.total_cmp(&b.divergence))
        {
            hotspots.push(hotspot(
                worst.start_line,
                Some(&worst.name),
                FUNCTION_DIVERGENCE,
                format!(
                    "{} nesting divergence is {:.1}, gate is {}",
                    worst.name, worst.divergence, gate as i64
                ),
            ));
        }
    }
    if let Some(gate) = over_gate(&before.result, &after.result, FAN_OUT) {
        hotspots.push(hotspot(
            1,
            None,
            FAN_OUT,
            format!(
                "fan-out is {:.0}, gate is {}",
                after.result.raw_metrics[FAN_OUT], gate as i64
            ),
        ));
    }
    hotspots
}

/// The gate `metric` now fails, when it rose over the change to get there.
/// The limit comes from the engine's registry, like [`gate_limit`].
fn over_gate(
    before: &ClassificationResult,
    after: &ClassificationResult,
    metric: &str,
) -> Option<f64> {
    let gate = gate_limit(metric)?;
    let now = *after.raw_metrics.get(metric)?;
    (metric_delta(before, after, metric) > 0.0 && now > gate).then_some(gate)
}

/// The first dangerous call at head that the base did not already make,
/// matched by callee and counted, so a call that only moved lines is not
/// new. The caller has checked that the scored count rose.
fn new_security_finding(before: &mut Side, after: &mut Side) -> Option<SecurityFinding> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for finding in before.dangerous_calls() {
        *seen
            .entry(finding.callee.unwrap_or(finding.snippet))
            .or_insert(0) += 1;
    }
    after.dangerous_calls().into_iter().find(|finding| {
        match seen.get_mut(finding.callee.as_ref().unwrap_or(&finding.snippet)) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{branchy, commit_all, recap, write_files, write_repo};

    #[test]
    fn an_over_gate_function_in_a_new_file_is_a_hotspot() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(&repo, &[("src/big.py", &branchy("pick"))]);
        commit_all(&repo, "add");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        let file = &recap.files[0];
        assert!(
            file.hotspots
                .iter()
                .any(|h| h.metric == "ast.max_function_complexity" && h.path == "src/big.py"),
            "{:?}",
            file.hotspots
        );
    }
}
