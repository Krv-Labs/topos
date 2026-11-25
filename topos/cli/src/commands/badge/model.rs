use std::collections::BTreeMap;

use serde_json::{json, Value};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::{EvaluationValue, Generator};

use crate::commands::evaluate::summary::floor_verdict;

const RATE_COLORS: [(u8, &str); 6] = [
    (100, "#44cc11"),
    (95, "#97ca00"),
    (85, "#a4a61d"),
    (70, "#dfb317"),
    (50, "#fe7d37"),
    (0, "#e05d44"),
];
pub(crate) const NOT_MEASURED_COLOR: &str = "#9f9f9f";
pub(crate) const STRICT_RULE: &str =
    "a pillar holds only if every scored file passes it, as in `topos evaluate`";

pub(crate) fn rate_color(rate: Option<u8>) -> &'static str {
    let Some(rate) = rate else {
        return NOT_MEASURED_COLOR;
    };
    RATE_COLORS
        .iter()
        .find(|(floor, _)| rate >= *floor)
        .map_or(NOT_MEASURED_COLOR, |(_, color)| color)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coverage {
    Measured,
    Partial,
    NotMeasured,
}

impl Coverage {
    fn key(self) -> &'static str {
        match self {
            Coverage::Measured => "measured",
            Coverage::Partial => "partial",
            Coverage::NotMeasured => "not_measured",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PillarRate {
    pub(crate) pillar: Generator,
    pub(crate) passed: usize,
    pub(crate) measured: usize,
    scored: usize,
}

impl PillarRate {
    fn tally(results: &[ClassificationResult], pillar: Generator) -> PillarRate {
        let scored: Vec<_> = results.iter().filter(|r| r.is_parseable).collect();
        let values: Vec<_> = scored
            .iter()
            .filter_map(|r| r.dimensions.get(pillar.as_str()))
            .collect();
        PillarRate {
            pillar,
            passed: values.iter().filter(|v| ***v == pillar.value()).count(),
            measured: values.len(),
            scored: scored.len(),
        }
    }

    pub(crate) fn rate(&self) -> Option<u8> {
        (self.measured > 0).then(|| (self.passed * 100 / self.measured) as u8)
    }

    pub(crate) fn coverage(&self) -> Coverage {
        match self.measured {
            0 => Coverage::NotMeasured,
            n if n < self.scored => Coverage::Partial,
            _ => Coverage::Measured,
        }
    }

    pub(crate) fn name(&self) -> String {
        self.pillar.as_str().to_ascii_uppercase()
    }

    pub(crate) fn shown(&self) -> String {
        self.rate()
            .map_or_else(|| "n/a".to_string(), |rate| format!("{rate}%"))
    }

    fn json(&self) -> Value {
        json!({
            "passed": self.passed,
            "measured": self.measured,
            "unmeasured": self.scored - self.measured,
            "rate": self.rate(),
            "state": self.coverage().key(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Badge {
    rates: [PillarRate; 4],
    unparsed: usize,
    floor: EvaluationValue,
}

impl Badge {
    pub(crate) fn new(
        results: &[ClassificationResult],
        strict: &BTreeMap<String, EvaluationValue>,
    ) -> Badge {
        Badge {
            rates: Generator::ALL.map(|pillar| PillarRate::tally(results, pillar)),
            unparsed: results.iter().filter(|r| !r.is_parseable).count(),
            floor: floor_verdict(strict),
        }
    }

    pub(crate) fn rates(&self) -> &[PillarRate; 4] {
        &self.rates
    }

    pub(crate) fn unparsed(&self) -> usize {
        self.unparsed
    }

    fn strict_medal(&self) -> &'static str {
        match self.floor {
            EvaluationValue::Slop => "NO MEDAL",
            other => other.medal_tier(),
        }
    }

    pub(crate) fn summary(&self) -> String {
        self.rates
            .iter()
            .map(|rate| format!("{} {}", rate.name(), rate.shown()))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub(crate) fn lowest_color(&self) -> &'static str {
        rate_color(self.rates.iter().filter_map(PillarRate::rate).min())
    }

    pub(crate) fn endpoint_json(&self) -> Value {
        json!({
            "schemaVersion": 1,
            "label": "topos",
            "message": self.summary(),
            "color": self.lowest_color().trim_start_matches('#'),
        })
    }

    pub(crate) fn result_json(&self, files: usize, warnings: &[String]) -> Value {
        let pillars: serde_json::Map<String, Value> = self
            .rates
            .iter()
            .map(|rate| (rate.pillar.as_str().to_string(), rate.json()))
            .collect();
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "pillars": pillars,
            "files": files,
            "unparsed": self.unparsed,
            "strict": {
                "rule": STRICT_RULE,
                "medal": self.strict_medal(),
                "lattice_element": self.floor.name(),
            },
            "warnings": warnings,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::commands::badge::coverage::measured_rollup;

    pub(crate) fn file(pillars: &[(Generator, bool)]) -> ClassificationResult {
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

    pub(crate) fn badge_of(results: &[ClassificationResult]) -> Badge {
        Badge::new(results, &measured_rollup(results).overall)
    }

    pub(crate) fn badge_with(passed: [usize; 4], measured: [usize; 4]) -> Badge {
        let files = measured.iter().copied().max().unwrap_or(0);
        let results: Vec<_> = (0..files)
            .map(|index| {
                let pillars: Vec<_> = Generator::ALL
                    .iter()
                    .enumerate()
                    .filter(|(slot, _)| index < measured[*slot])
                    .map(|(slot, pillar)| (*pillar, index < passed[slot]))
                    .collect();
                file(&pillars)
            })
            .collect();
        badge_of(&results)
    }

    fn rate_of(badge: &Badge, pillar: Generator) -> PillarRate {
        badge.rates()[Generator::ALL.iter().position(|p| *p == pillar).unwrap()]
    }

    #[test]
    fn a_rate_rounds_down_so_almost_all_never_reads_as_all() {
        let badge = badge_with([249, 3, 2, 0], [250, 4, 3, 1]);
        assert_eq!(
            badge.rates().map(|rate| rate.rate()),
            [Some(99), Some(75), Some(66), Some(0)]
        );
        assert_eq!(
            badge.summary(),
            "SIMPLE 99% · COMPOSABLE 75% · SECURE 66% · NAVIGABLE 0%"
        );
    }

    #[test]
    fn a_pillar_with_no_measured_files_is_not_measured() {
        let badge = badge_of(&[file(&[(Generator::Simple, true)])]);
        let composable = rate_of(&badge, Generator::Composable);
        assert_eq!(composable.rate(), None);
        assert_eq!(composable.coverage(), Coverage::NotMeasured);
        assert_eq!(composable.shown(), "n/a");
        let result = badge.result_json(1, &[]);
        assert_eq!(
            result["pillars"]["composable"],
            json!({"passed": 0, "measured": 0, "unmeasured": 1, "rate": null, "state": "not_measured"})
        );
    }

    #[test]
    fn unparsed_files_are_left_out_of_every_rate_and_counted_apart() {
        let badge = badge_of(&[
            file(&[(Generator::Simple, true), (Generator::Secure, false)]),
            ClassificationResult::default(),
        ]);
        assert_eq!(badge.unparsed(), 1);
        assert_eq!(rate_of(&badge, Generator::Simple).rate(), Some(100));
        assert_eq!(rate_of(&badge, Generator::Secure).rate(), Some(0));
        let result = badge.result_json(2, &[]);
        assert_eq!(result["unparsed"], 1);
        assert_eq!(result["files"], 2);
        assert_eq!(result["pillars"]["simple"]["measured"], 1);
        assert_eq!(result["strict"]["medal"], "NO MEDAL");
    }

    #[test]
    fn partial_coverage_rates_only_the_covered_files() {
        let badge = badge_of(&[
            file(&[(Generator::Simple, true), (Generator::Composable, true)]),
            file(&[(Generator::Simple, true), (Generator::Composable, false)]),
            file(&[(Generator::Simple, false)]),
        ]);
        let composable = rate_of(&badge, Generator::Composable);
        assert_eq!((composable.passed, composable.measured), (1, 2));
        assert_eq!(composable.rate(), Some(50));
        assert_eq!(composable.coverage(), Coverage::Partial);
        assert_eq!(
            badge.result_json(3, &[])["pillars"]["composable"],
            json!({"passed": 1, "measured": 2, "unmeasured": 1, "rate": 50, "state": "partial"})
        );
        assert_eq!(rate_of(&badge, Generator::Simple).rate(), Some(66));
        assert_eq!(
            rate_of(&badge, Generator::Simple).coverage(),
            Coverage::Measured
        );
    }

    #[test]
    fn the_color_scale_changes_hue_exactly_at_each_boundary() {
        for (rate, color) in [
            (Some(100), "#44cc11"),
            (Some(99), "#97ca00"),
            (Some(95), "#97ca00"),
            (Some(94), "#a4a61d"),
            (Some(85), "#a4a61d"),
            (Some(84), "#dfb317"),
            (Some(70), "#dfb317"),
            (Some(69), "#fe7d37"),
            (Some(50), "#fe7d37"),
            (Some(49), "#e05d44"),
            (Some(0), "#e05d44"),
            (None, NOT_MEASURED_COLOR),
        ] {
            assert_eq!(rate_color(rate), color, "{rate:?}");
        }
    }

    #[test]
    fn the_endpoint_names_every_pillar_and_takes_the_lowest_measured_color() {
        let badge = badge_with([4, 0, 4, 3], [4, 0, 4, 4]);
        assert_eq!(
            badge.endpoint_json(),
            json!({
                "schemaVersion": 1,
                "label": "topos",
                "message": "SIMPLE 100% · COMPOSABLE n/a · SECURE 100% · NAVIGABLE 75%",
                "color": "dfb317",
            })
        );
    }

    #[test]
    fn an_endpoint_with_nothing_measured_is_gray() {
        assert_eq!(badge_of(&[]).endpoint_json()["color"], "9f9f9f");
    }

    #[test]
    fn the_strict_medal_is_kept_for_reference_and_labeled() {
        let badge = badge_with([2, 0, 2, 2], [2, 0, 2, 2]);
        let strict = &badge.result_json(2, &[])["strict"];
        assert_eq!(strict["medal"], "GOLD");
        assert_eq!(strict["lattice_element"], "SIMPLE_SECURE_NAVIGABLE");
        assert_eq!(strict["rule"], STRICT_RULE);
        let failing = badge_with([1, 0, 2, 2], [2, 0, 2, 2]);
        assert_eq!(failing.result_json(2, &[])["strict"]["medal"], "SILVER");
    }
}
