use std::collections::BTreeMap;

use serde_json::{json, Value};
use topos_engine::core::omega::{EvaluationValue, Generator};

use crate::commands::evaluate::summary::floor_verdict;

const TIER_COLORS: [(&str, &str); 4] = [
    ("PLATINUM", "#b45309"),
    ("GOLD", "#b45309"),
    ("SILVER", "#525252"),
    ("BRONZE", "#92400e"),
];
const NO_MEDAL_COLOR: &str = "#a3a3a3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PillarState {
    Pass,
    Fail,
    NotMeasured,
}

impl PillarState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            PillarState::Pass => "pass",
            PillarState::Fail => "fail",
            PillarState::NotMeasured => "not measured",
        }
    }

    fn key(self) -> &'static str {
        match self {
            PillarState::NotMeasured => "not_measured",
            other => other.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Badge {
    floor: EvaluationValue,
    pillars: [(Generator, PillarState); 4],
}

impl Badge {
    pub(crate) fn from_rollup(overall: &BTreeMap<String, EvaluationValue>) -> Badge {
        Badge {
            floor: floor_verdict(overall),
            pillars: Generator::ALL.map(|pillar| (pillar, pillar_state(overall, pillar))),
        }
    }

    pub(crate) fn pillars(&self) -> &[(Generator, PillarState); 4] {
        &self.pillars
    }

    pub(crate) fn medal(&self) -> &'static str {
        match self.floor {
            EvaluationValue::Slop => "NO MEDAL",
            other => other.medal_tier(),
        }
    }

    pub(crate) fn emoji(&self) -> &'static str {
        self.floor.symbol()
    }

    pub(crate) fn color(&self) -> &'static str {
        let tier = self.floor.medal_tier();
        TIER_COLORS
            .iter()
            .find(|(name, _)| *name == tier)
            .map_or(NO_MEDAL_COLOR, |(_, color)| color)
    }

    pub(crate) fn pillar_summary(&self) -> String {
        self.pillars
            .iter()
            .map(|(pillar, state)| {
                format!("{} {}", pillar.as_str().to_ascii_uppercase(), state.label())
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn endpoint_json(&self) -> Value {
        json!({
            "schemaVersion": 1,
            "label": "topos",
            "message": self.medal(),
            "color": self.color().trim_start_matches('#'),
        })
    }

    pub(crate) fn result_json(&self, files: usize, warnings: &[String]) -> Value {
        let pillars: serde_json::Map<String, Value> = self
            .pillars
            .iter()
            .map(|(pillar, state)| (pillar.as_str().to_string(), json!(state.key())))
            .collect();
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "medal": self.medal(),
            "lattice_element": self.floor.name(),
            "pillars": pillars,
            "files": files,
            "warnings": warnings,
        })
    }
}

fn pillar_state(overall: &BTreeMap<String, EvaluationValue>, pillar: Generator) -> PillarState {
    match overall.get(pillar.as_str()) {
        None => PillarState::NotMeasured,
        Some(EvaluationValue::Slop) => PillarState::Fail,
        Some(_) => PillarState::Pass,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn badge(passing: &[Generator], failing: &[Generator]) -> Badge {
        let mut overall = BTreeMap::new();
        for pillar in passing {
            overall.insert(pillar.as_str().to_string(), pillar.value());
        }
        for pillar in failing {
            overall.insert(pillar.as_str().to_string(), EvaluationValue::Slop);
        }
        Badge::from_rollup(&overall)
    }

    pub(crate) fn badge_passing(count: usize) -> Badge {
        badge(&Generator::ALL[..count], &Generator::ALL[count..])
    }

    #[test]
    fn each_tier_names_its_medal_and_color_in_the_endpoint() {
        for (count, medal, color) in [
            (4, "PLATINUM", "b45309"),
            (3, "GOLD", "b45309"),
            (2, "SILVER", "525252"),
            (1, "BRONZE", "92400e"),
            (0, "NO MEDAL", "a3a3a3"),
        ] {
            assert_eq!(
                badge_passing(count).endpoint_json(),
                json!({"schemaVersion": 1, "label": "topos", "message": medal, "color": color}),
                "{count} passing pillars"
            );
        }
    }

    #[test]
    fn an_unmeasured_pillar_is_neither_a_pass_nor_a_failure() {
        let badge = badge(
            &[Generator::Simple, Generator::Secure, Generator::Navigable],
            &[],
        );
        assert_eq!(badge.medal(), "GOLD");
        assert_eq!(
            badge.pillar_summary(),
            "SIMPLE pass, COMPOSABLE not measured, SECURE pass, NAVIGABLE pass"
        );
        let result = badge.result_json(2, &["no graph".to_string()]);
        assert_eq!(result["pillars"]["composable"], "not_measured");
        assert_eq!(result["pillars"]["simple"], "pass");
        assert_eq!(result["lattice_element"], "SIMPLE_SECURE_NAVIGABLE");
        assert_eq!(result["files"], 2);
        assert_eq!(result["warnings"][0], "no graph");
    }

    #[test]
    fn a_failed_pillar_is_reported_as_failed() {
        let badge = badge(&[Generator::Simple], &[Generator::Composable]);
        assert_eq!(badge.medal(), "BRONZE");
        assert_eq!(badge.result_json(1, &[])["pillars"]["composable"], "fail");
    }
}
