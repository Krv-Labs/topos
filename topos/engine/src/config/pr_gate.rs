//! PR gate policy for `topos pr-recap`: the `[pr_recap]` table.
//!
//! Every gate has a severity (`off | info | warn | block`) and every
//! threshold is a setting. The defaults live in [`GATES`],
//! [`PrGatePreset::score_drop`], [`PrGatePreset::import_cycle`] and
//! [`PrGatePreset::fan_in_growth`], and nowhere else: `pr-recap` reads the
//! resolved [`PrGateConfig`] and adds no policy of its own.
//!
//! On disk the table holds a `preset` plus only the keys that differ from
//! it, so a project on `recommended` picks up improved defaults when they
//! change. Parsing is best-effort like the rest of `.topos.toml`: a bad key
//! is dropped and recorded in [`PrGateConfig::warnings`] instead of
//! discarding the whole table.
//!
//! `[[pr_recap.waive]]` entries ([`Waiver`]) are parsed here too, but they
//! are not settings: no preset owns them, they never make the gate
//! `custom`, and `topos config` leaves them where they are.

use std::fmt;

use crate::evaluation::waivers::{parse_waivers, Waiver};
use crate::graphs::ast::languages::language_for_path;

/// How much a gate's finding matters. Ordered, so the worst finding wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Off,
    Info,
    Warn,
    Block,
}

impl Severity {
    pub const ALL: [Severity; 4] = [
        Severity::Off,
        Severity::Info,
        Severity::Warn,
        Severity::Block,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Off => "off",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Block => "block",
        }
    }

    pub fn parse(value: &str) -> Option<Severity> {
        Severity::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// One named rule that can turn a file change into a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GateId {
    PillarLost,
    PillarInherited,
    MovedPillar,
    ScoreDrop,
    NewFileInsecure,
    NewFileSlop,
    NewFilePillar,
    SplitSecureRise,
    SplitMovedGrowth,
    SplitBloat,
    Cosmetic,
    Suspicious,
    Incomplete,
    ImportCycle,
    FanInGrowth,
    BlastRadius,
}

pub const GATE_COUNT: usize = 16;

struct GateSpec {
    id: GateId,
    key: &'static str,
    describe: &'static str,
    /// Default severity per preset: relaxed, recommended, strict.
    defaults: [Severity; 3],
}

use Severity::{Block, Info, Off, Warn};

/// The gate defaults, one row per gate, in [`GateId`] order.
const GATES: [GateSpec; GATE_COUNT] = [
    GateSpec {
        id: GateId::PillarLost,
        key: "pillar_lost",
        describe: "an existing file stops clearing a pillar it cleared before",
        defaults: [Block, Block, Block],
    },
    GateSpec {
        id: GateId::PillarInherited,
        key: "pillar_inherited",
        describe: "a pillar that already failed got worse",
        defaults: [Off, Info, Warn],
    },
    GateSpec {
        id: GateId::MovedPillar,
        key: "moved_pillar",
        describe: "pillar loss or score drop caused by moved code",
        defaults: [Info, Info, Warn],
    },
    GateSpec {
        id: GateId::ScoreDrop,
        key: "score_drop",
        describe: "a pillar score fell by at least [pr_recap.score_drop]",
        defaults: [Warn, Warn, Warn],
    },
    GateSpec {
        id: GateId::NewFileInsecure,
        key: "new_file_insecure",
        describe: "a new file fails SECURE",
        defaults: [Block, Block, Block],
    },
    GateSpec {
        id: GateId::NewFileSlop,
        key: "new_file_slop",
        describe: "a new file is SLOP, the lowest medal",
        defaults: [Block, Block, Block],
    },
    GateSpec {
        id: GateId::NewFilePillar,
        key: "new_file_pillar",
        describe: "a new file fails SIMPLE, COMPOSABLE or NAVIGABLE",
        defaults: [Off, Info, Warn],
    },
    GateSpec {
        id: GateId::SplitSecureRise,
        key: "split_secure_rise",
        describe: "a file split into modules gained SECURE findings",
        defaults: [Block, Block, Block],
    },
    GateSpec {
        id: GateId::SplitMovedGrowth,
        key: "split_moved_growth",
        describe: "a function moved by a split grew more complex",
        defaults: [Info, Warn, Block],
    },
    GateSpec {
        id: GateId::SplitBloat,
        key: "split_bloat",
        describe: "a split grew noticeably or produced a SLOP module",
        defaults: [Info, Info, Info],
    },
    GateSpec {
        id: GateId::Cosmetic,
        key: "cosmetic",
        describe: "a score moved but the code structure did not",
        defaults: [Info, Warn, Block],
    },
    GateSpec {
        id: GateId::Suspicious,
        key: "suspicious",
        describe: "a score rose but the code structure did not change",
        defaults: [Info, Warn, Block],
    },
    GateSpec {
        id: GateId::Incomplete,
        key: "incomplete",
        describe: "some files were skipped or failed to score",
        defaults: [Info, Info, Info],
    },
    GateSpec {
        id: GateId::ImportCycle,
        key: "import_cycle",
        describe: "new import cycle base→head (languages not in [pr_recap.import_cycle])",
        defaults: [Info, Warn, Block],
    },
    GateSpec {
        id: GateId::FanInGrowth,
        key: "fan_in_growth",
        describe: "more files depend on a file that already fails SIMPLE",
        defaults: [Info, Warn, Warn],
    },
    GateSpec {
        id: GateId::BlastRadius,
        key: "blast_radius",
        describe: "direct and transitive dependents of the change",
        defaults: [Off, Info, Info],
    },
];

impl GateId {
    /// Every gate, in [`GATES`] row order.
    pub const ALL: [GateId; GATE_COUNT] = {
        let mut all = [GateId::PillarLost; GATE_COUNT];
        let mut index = 0;
        while index < GATE_COUNT {
            all[index] = GATES[index].id;
            index += 1;
        }
        all
    };

    /// The key under `[pr_recap.gates]`.
    pub const fn key(self) -> &'static str {
        GATES[self as usize].key
    }

    /// One line on what trips the gate, shown by `config show` and the
    /// commented block `topos config` writes for a custom gate.
    pub const fn describe(self) -> &'static str {
        GATES[self as usize].describe
    }

    pub fn parse(key: &str) -> Option<GateId> {
        GateId::ALL.into_iter().find(|g| g.key() == key)
    }
}

/// Severity per gate, indexed by [`GateId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSeverities([Severity; GATE_COUNT]);

impl GateSeverities {
    pub const fn get(&self, gate: GateId) -> Severity {
        self.0[gate as usize]
    }

    pub fn set(&mut self, gate: GateId, severity: Severity) {
        self.0[gate as usize] = severity;
    }
}

/// A score drop only counts once it is this large, in a file with at
/// least this much churn. Integers, because [`crate::config::ToposConfig`]
/// is `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreDropThreshold {
    /// Points on the displayed 0–100 scale.
    pub min_points: u32,
    /// Lines added plus lines removed in the file.
    pub min_changed_lines: u32,
}

/// Languages with their own severity under `[pr_recap.import_cycle]`, in
/// table order. Keys match [`crate::graphs::ast::languages`].
pub const CYCLE_LANGUAGES: [&str; 4] = ["rust", "typescript", "javascript", "python"];

/// Severity of a new import cycle per language, indexed like
/// [`CYCLE_LANGUAGES`]. A cycle in any other language falls back to the
/// `import_cycle` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportCycleSeverities([Severity; 4]);

impl ImportCycleSeverities {
    /// `None` for a language without its own row.
    pub fn get(&self, language: &str) -> Option<Severity> {
        let slot = CYCLE_LANGUAGES.iter().position(|l| *l == language)?;
        Some(self.0[slot])
    }

    fn slot(&mut self, language: &str) -> Option<&mut Severity> {
        let slot = CYCLE_LANGUAGES.iter().position(|l| *l == language)?;
        Some(&mut self.0[slot])
    }
}

/// Fan-in growth on a file that fails SIMPLE only counts once this many
/// new files depend on it and its dependents grew by at least this share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanInGrowthThreshold {
    /// New dependents, and also the smallest absolute growth that counts.
    pub min_new_dependents: u32,
    /// Growth as a percentage of the dependents the file had at base.
    pub min_growth_percent: u32,
}

/// Which findings fail the check (exit 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOn {
    Block,
    Warn,
}

impl FailOn {
    pub const fn as_str(self) -> &'static str {
        match self {
            FailOn::Block => "block",
            FailOn::Warn => "warn",
        }
    }

    pub fn parse(value: &str) -> Option<FailOn> {
        [FailOn::Block, FailOn::Warn]
            .into_iter()
            .find(|f| f.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrGatePreset {
    Relaxed,
    #[default]
    Recommended,
    Strict,
    /// Recommended plus whatever keys the file sets explicitly.
    Custom,
}

impl PrGatePreset {
    pub const ALL: [PrGatePreset; 4] = [
        PrGatePreset::Relaxed,
        PrGatePreset::Recommended,
        PrGatePreset::Strict,
        PrGatePreset::Custom,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            PrGatePreset::Relaxed => "relaxed",
            PrGatePreset::Recommended => "recommended",
            PrGatePreset::Strict => "strict",
            PrGatePreset::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<PrGatePreset> {
        PrGatePreset::ALL
            .into_iter()
            .find(|p| p.as_str() == value.trim().to_ascii_lowercase())
    }

    /// Column in [`GateSpec::defaults`]. Custom starts from Recommended.
    const fn column(self) -> usize {
        match self {
            PrGatePreset::Relaxed => 0,
            PrGatePreset::Recommended | PrGatePreset::Custom => 1,
            PrGatePreset::Strict => 2,
        }
    }

    const fn score_drop(self) -> ScoreDropThreshold {
        match self {
            PrGatePreset::Relaxed => ScoreDropThreshold {
                min_points: 20,
                min_changed_lines: 50,
            },
            PrGatePreset::Recommended | PrGatePreset::Custom => ScoreDropThreshold {
                min_points: 10,
                min_changed_lines: 20,
            },
            PrGatePreset::Strict => ScoreDropThreshold {
                min_points: 5,
                min_changed_lines: 0,
            },
        }
    }

    /// Rust and TypeScript/JavaScript tolerate module cycles; Python's can
    /// fail at import time, so its row is stricter.
    const fn import_cycle(self) -> ImportCycleSeverities {
        match self {
            PrGatePreset::Relaxed => ImportCycleSeverities([Off, Off, Off, Warn]),
            PrGatePreset::Recommended | PrGatePreset::Custom => {
                ImportCycleSeverities([Info, Info, Info, Warn])
            }
            PrGatePreset::Strict => ImportCycleSeverities([Warn, Warn, Warn, Block]),
        }
    }

    const fn fan_in_growth(self) -> FanInGrowthThreshold {
        FanInGrowthThreshold {
            min_new_dependents: 2,
            min_growth_percent: 25,
        }
    }

    const fn fail_on(self) -> FailOn {
        match self {
            PrGatePreset::Strict => FailOn::Warn,
            _ => FailOn::Block,
        }
    }
}

const DEFAULT_MAX_HOTSPOTS: u32 = 3;

/// The comment written beside `preset` in the commented block. `topos
/// config` compares against it to tell its own comment from a user's.
pub const PRESET_COMMENT: &str =
    "relaxed | recommended | strict | custom (custom = recommended + the keys below)";

/// The resolved `[pr_recap]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrGateConfig {
    pub preset: PrGatePreset,
    pub gates: GateSeverities,
    pub score_drop: ScoreDropThreshold,
    pub import_cycle: ImportCycleSeverities,
    pub fan_in_growth: FanInGrowthThreshold,
    pub fail_on: FailOn,
    /// Exactly how many "where to look" items the report lists.
    pub max_hotspots: u32,
    /// `[[pr_recap.waive]]`, in file order. Not a setting: see the module
    /// docs.
    pub waivers: Vec<Waiver>,
    /// Keys dropped while parsing, so a typo is never silent.
    pub warnings: Vec<String>,
}

impl Default for PrGateConfig {
    fn default() -> Self {
        PrGateConfig::for_preset(PrGatePreset::Recommended)
    }
}

/// A setting value as `config show` prints it and TOML stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingValue {
    Text(&'static str),
    Number(u32),
}

impl SettingValue {
    fn toml_literal(self) -> String {
        match self {
            SettingValue::Text(text) => format!("\"{text}\""),
            SettingValue::Number(n) => n.to_string(),
        }
    }
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::Text(text) => f.write_str(text),
            SettingValue::Number(n) => write!(f, "{n}"),
        }
    }
}

/// One editable setting, for listing and for the commented TOML block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    /// Table under `[pr_recap]` (`""`, `"gates"`, `"score_drop"`,
    /// `"import_cycle"` or `"fan_in_growth"`).
    pub table: &'static str,
    pub key: &'static str,
    pub value: SettingValue,
    /// Value under the active preset, before explicit keys.
    pub preset_value: SettingValue,
    /// Value under Recommended, the shipped default.
    pub default_value: SettingValue,
    pub describe: &'static str,
}

impl Setting {
    /// Path relative to `[pr_recap]`, e.g. `gates.pillar_lost`.
    pub fn path(&self) -> String {
        if self.table.is_empty() {
            self.key.to_string()
        } else {
            format!("{}.{}", self.table, self.key)
        }
    }

    pub fn is_changed(&self) -> bool {
        self.value != self.preset_value
    }
}

impl PrGateConfig {
    pub fn for_preset(preset: PrGatePreset) -> Self {
        let column = preset.column();
        PrGateConfig {
            preset,
            gates: GateSeverities(GATES.map(|spec| spec.defaults[column])),
            score_drop: preset.score_drop(),
            import_cycle: preset.import_cycle(),
            fan_in_growth: preset.fan_in_growth(),
            fail_on: preset.fail_on(),
            max_hotspots: DEFAULT_MAX_HOTSPOTS,
            waivers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn severity(&self, gate: GateId) -> Severity {
        self.gates.get(gate)
    }

    /// Severity of a new import cycle through `members`: each file takes its
    /// language's `[pr_recap.import_cycle]` row, or the `import_cycle` gate
    /// when its language has none, and the strictest file wins.
    pub fn cycle_severity<'a>(&self, members: impl IntoIterator<Item = &'a str>) -> Severity {
        let fallback = self.severity(GateId::ImportCycle);
        members
            .into_iter()
            .map(|path| {
                language_for_path(path)
                    .and_then(|language| self.import_cycle.get(language))
                    .unwrap_or(fallback)
            })
            .max()
            .unwrap_or(fallback)
    }

    /// Every editable setting with its active, preset and default value.
    pub fn settings(&self) -> Vec<Setting> {
        let preset = PrGateConfig::for_preset(self.preset);
        let default = PrGateConfig::default();
        let row = |config: &PrGateConfig, table: &str, key: &str| config.value_of(table, key);
        let mut rows = Vec::new();
        let mut push = |table: &'static str, key: &'static str, describe: &'static str| {
            rows.push(Setting {
                table,
                key,
                value: row(self, table, key),
                preset_value: row(&preset, table, key),
                default_value: row(&default, table, key),
                describe,
            });
        };
        push(
            "",
            "fail_on",
            "block | warn; warn also fails the check on NEEDS ATTENTION",
        );
        push(
            "",
            "max_hotspots",
            "how many places to look the report lists",
        );
        for gate in GateId::ALL {
            push("gates", gate.key(), gate.describe());
        }
        push(
            "score_drop",
            "min_points",
            "smallest drop that counts, on the 0–100 scale",
        );
        push(
            "score_drop",
            "min_changed_lines",
            "lines added + removed in the file; smaller edits don't count",
        );
        for (language, describe) in CYCLE_LANGUAGES.into_iter().zip([
            "a new cycle among Rust modules",
            "a new cycle among TypeScript modules",
            "a new cycle among JavaScript modules",
            "a new cycle among Python modules; these can fail at import time",
        ]) {
            push("import_cycle", language, describe);
        }
        push(
            "fan_in_growth",
            "min_new_dependents",
            "fewest new dependents that count, and the smallest growth",
        );
        push(
            "fan_in_growth",
            "min_growth_percent",
            "growth as a share of the dependents at base",
        );
        rows
    }

    /// The `[pr_recap]` keys a preset controls: each top-level setting and
    /// each settings table, in [`PrGateConfig::settings`] order. A named
    /// preset owns these keys and nothing else under `[pr_recap]`.
    pub fn preset_keys() -> Vec<&'static str> {
        let mut keys = Vec::new();
        for setting in PrGateConfig::default().settings() {
            let key = if setting.table.is_empty() {
                setting.key
            } else {
                setting.table
            };
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    /// Settings whose value differs from the preset's.
    pub fn overrides(&self) -> Vec<Setting> {
        self.settings()
            .into_iter()
            .filter(Setting::is_changed)
            .collect()
    }

    /// One-line summary for reports: `recommended`, `custom · 2 changes`.
    pub fn label(&self) -> String {
        match self.overrides().len() {
            0 => self.preset.as_str().to_string(),
            1 => format!("{} · 1 change", self.preset.as_str()),
            n => format!("{} · {n} changes", self.preset.as_str()),
        }
    }

    fn value_of(&self, table: &str, key: &str) -> SettingValue {
        match (table, key) {
            ("", "fail_on") => SettingValue::Text(self.fail_on.as_str()),
            ("", "max_hotspots") => SettingValue::Number(self.max_hotspots),
            ("score_drop", "min_points") => SettingValue::Number(self.score_drop.min_points),
            ("score_drop", "min_changed_lines") => {
                SettingValue::Number(self.score_drop.min_changed_lines)
            }
            ("fan_in_growth", "min_new_dependents") => {
                SettingValue::Number(self.fan_in_growth.min_new_dependents)
            }
            ("fan_in_growth", "min_growth_percent") => {
                SettingValue::Number(self.fan_in_growth.min_growth_percent)
            }
            ("import_cycle", language) => SettingValue::Text(
                self.import_cycle
                    .get(language)
                    .unwrap_or(Severity::Off)
                    .as_str(),
            ),
            ("gates", gate) => SettingValue::Text(
                GateId::parse(gate)
                    .map_or(Severity::Off, |g| self.severity(g))
                    .as_str(),
            ),
            _ => unreachable!("settings() only asks for known keys"),
        }
    }

    /// The full `[pr_recap]` block with every key, its default in brackets
    /// and what it means, so nobody needs the docs to edit it.
    pub fn to_commented_toml(&self) -> String {
        let settings = self.settings();
        let mut lines: Vec<(String, String)> = vec![
            ("[pr_recap]".to_string(), String::new()),
            (
                format!("preset = \"{}\"", self.preset.as_str()),
                PRESET_COMMENT.to_string(),
            ),
        ];
        let mut table = "";
        for setting in &settings {
            if setting.table != table {
                table = setting.table;
                let header_note = match table {
                    "gates" => "off | info | warn | block   [default in brackets]",
                    "import_cycle" => "per language; other languages use gates.import_cycle",
                    "fan_in_growth" => "fan-in growth has to clear both",
                    _ => "a score drop has to clear both",
                };
                lines.push((String::new(), String::new()));
                lines.push((format!("[pr_recap.{table}]"), header_note.to_string()));
            }
            lines.push((
                format!("{} = {}", setting.key, setting.value.toml_literal()),
                format!("[{}] {}", setting.default_value, setting.describe),
            ));
        }
        let width = lines.iter().map(|(code, _)| code.len()).max().unwrap_or(0) + 2;
        let mut out = String::new();
        for (code, note) in lines {
            if note.is_empty() {
                out.push_str(&code);
            } else {
                out.push_str(&format!("{code:<width$}# {note}"));
            }
            out.push('\n');
        }
        out
    }

    /// Parse the `[pr_recap]` table: start from its preset, then apply every
    /// valid explicit key. Invalid keys are skipped and recorded.
    pub fn from_table(table: &toml::Table) -> Self {
        let mut warnings = Vec::new();
        let preset = match table.get("preset") {
            None => PrGatePreset::default(),
            Some(value) => value
                .as_str()
                .and_then(PrGatePreset::parse)
                .unwrap_or_else(|| {
                    warnings.push(invalid(
                        "preset",
                        value,
                        "relaxed, recommended, strict or custom",
                    ));
                    PrGatePreset::default()
                }),
        };
        let mut config = PrGateConfig::for_preset(preset);
        for (key, value) in table {
            match key.as_str() {
                "preset" => {}
                "fail_on" => match value.as_str().and_then(FailOn::parse) {
                    Some(fail_on) => config.fail_on = fail_on,
                    None => warnings.push(invalid("fail_on", value, "block or warn")),
                },
                "max_hotspots" => match count(value) {
                    Some(n) => config.max_hotspots = n,
                    None => warnings.push(invalid("max_hotspots", value, "a whole number")),
                },
                "gates" => config.apply_gates(value, &mut warnings),
                "score_drop" => config.apply_score_drop(value, &mut warnings),
                "import_cycle" => config.apply_import_cycle(value, &mut warnings),
                "fan_in_growth" => config.apply_fan_in_growth(value, &mut warnings),
                "waive" => config.waivers = parse_waivers(value, &mut warnings),
                other => warnings.push(format!("pr_recap.{other}: unknown setting, ignored")),
            }
        }
        config.warnings = warnings;
        config
    }

    fn apply_gates(&mut self, value: &toml::Value, warnings: &mut Vec<String>) {
        let Some(gates) = value.as_table() else {
            warnings.push("pr_recap.gates: expected a table, ignored".to_string());
            return;
        };
        for (key, value) in gates {
            let path = format!("gates.{key}");
            let Some(gate) = GateId::parse(key) else {
                warnings.push(format!("pr_recap.{path}: unknown gate, ignored"));
                continue;
            };
            match value.as_str().and_then(Severity::parse) {
                Some(severity) => self.gates.set(gate, severity),
                None => warnings.push(invalid(&path, value, "off, info, warn or block")),
            }
        }
    }

    fn apply_score_drop(&mut self, value: &toml::Value, warnings: &mut Vec<String>) {
        let Some(table) = value.as_table() else {
            warnings.push("pr_recap.score_drop: expected a table, ignored".to_string());
            return;
        };
        for (key, value) in table {
            let path = format!("score_drop.{key}");
            let slot = match key.as_str() {
                "min_points" => &mut self.score_drop.min_points,
                "min_changed_lines" => &mut self.score_drop.min_changed_lines,
                _ => {
                    warnings.push(format!("pr_recap.{path}: unknown setting, ignored"));
                    continue;
                }
            };
            match count(value) {
                Some(n) => *slot = n,
                None => warnings.push(invalid(&path, value, "a whole number")),
            }
        }
    }

    fn apply_import_cycle(&mut self, value: &toml::Value, warnings: &mut Vec<String>) {
        let Some(table) = value.as_table() else {
            warnings.push("pr_recap.import_cycle: expected a table, ignored".to_string());
            return;
        };
        for (key, value) in table {
            let path = format!("import_cycle.{key}");
            let Some(slot) = self.import_cycle.slot(key) else {
                warnings.push(format!("pr_recap.{path}: unknown language, ignored"));
                continue;
            };
            match value.as_str().and_then(Severity::parse) {
                Some(severity) => *slot = severity,
                None => warnings.push(invalid(&path, value, "off, info, warn or block")),
            }
        }
    }

    fn apply_fan_in_growth(&mut self, value: &toml::Value, warnings: &mut Vec<String>) {
        let Some(table) = value.as_table() else {
            warnings.push("pr_recap.fan_in_growth: expected a table, ignored".to_string());
            return;
        };
        for (key, value) in table {
            let path = format!("fan_in_growth.{key}");
            let slot = match key.as_str() {
                "min_new_dependents" => &mut self.fan_in_growth.min_new_dependents,
                "min_growth_percent" => &mut self.fan_in_growth.min_growth_percent,
                _ => {
                    warnings.push(format!("pr_recap.{path}: unknown setting, ignored"));
                    continue;
                }
            };
            match count(value) {
                Some(n) => *slot = n,
                None => warnings.push(invalid(&path, value, "a whole number")),
            }
        }
    }
}

fn count(value: &toml::Value) -> Option<u32> {
    value.as_integer().and_then(|n| u32::try_from(n).ok())
}

fn invalid(path: &str, value: &toml::Value, expected: &str) -> String {
    format!("pr_recap.{path}: {value} is not valid (expected {expected}), ignored")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> PrGateConfig {
        let table: toml::Table = text.parse().unwrap();
        let pr_recap = table
            .get("pr_recap")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        PrGateConfig::from_table(&pr_recap)
    }

    #[test]
    fn gate_table_rows_follow_gate_id_order() {
        for (index, spec) in GATES.iter().enumerate() {
            assert_eq!(spec.id as usize, index, "{}", spec.key);
            assert_eq!(GateId::ALL[index], spec.id);
        }
    }

    #[test]
    fn empty_table_is_recommended() {
        let config = parse("");
        assert_eq!(config, PrGateConfig::default());
        assert_eq!(config.preset, PrGatePreset::Recommended);
        assert_eq!(config.severity(GateId::PillarLost), Severity::Block);
        assert_eq!(config.severity(GateId::Cosmetic), Severity::Warn);
        assert_eq!(config.score_drop.min_points, 10);
        assert_eq!(config.score_drop.min_changed_lines, 20);
        assert_eq!(config.fail_on, FailOn::Block);
        assert_eq!(config.max_hotspots, 3);
        assert_eq!(config.label(), "recommended");
    }

    #[test]
    fn presets_differ_where_the_spec_says() {
        let relaxed = PrGateConfig::for_preset(PrGatePreset::Relaxed);
        let strict = PrGateConfig::for_preset(PrGatePreset::Strict);
        assert_eq!(relaxed.severity(GateId::Cosmetic), Severity::Info);
        assert_eq!(relaxed.severity(GateId::PillarInherited), Severity::Off);
        assert_eq!(relaxed.score_drop.min_points, 20);
        assert_eq!(strict.fail_on, FailOn::Warn);
        assert_eq!(strict.severity(GateId::Suspicious), Severity::Block);
        assert_eq!(strict.score_drop.min_changed_lines, 0);
        for preset in PrGatePreset::ALL {
            let config = PrGateConfig::for_preset(preset);
            assert_eq!(config.severity(GateId::PillarLost), Severity::Block);
            assert_eq!(config.severity(GateId::NewFileInsecure), Severity::Block);
        }
    }

    #[test]
    fn moved_code_only_warns_under_strict() {
        let row = GateId::parse("moved_pillar").expect("moved_pillar is a gate");
        assert_eq!(row, GateId::MovedPillar);
        assert!(row.describe().contains("moved code"), "{}", row.describe());
        let severity = |preset| PrGateConfig::for_preset(preset).severity(GateId::MovedPillar);
        assert_eq!(severity(PrGatePreset::Relaxed), Severity::Info);
        assert_eq!(severity(PrGatePreset::Recommended), Severity::Info);
        assert_eq!(severity(PrGatePreset::Custom), Severity::Info);
        assert_eq!(severity(PrGatePreset::Strict), Severity::Warn);
        assert_eq!(GateId::ALL.len(), 16);
    }

    const WAIVERS: &str = "[pr_recap]\npreset = \"strict\"\n\n[[pr_recap.waive]]\ngate = \"pillar_lost\"\npath = \"src/legacy/**\"\nreason = \"being rewritten in #412\"\nexpires = \"2026-12-31\"\n\n[[pr_recap.waive]]\ngate = \"cosmetic\"\npath = \"src/gen/**\"\n\n[[pr_recap.waive]]\ngate = \"pilar_lost\"\npath = \"a/**\"\nreason = \"x\"\n\n[[pr_recap.waive]]\ngate = \"score_drop\"\npath = \"a/**\"\nreason = \"x\"\nexpires = \"next week\"\n";

    #[test]
    fn waivers_parse_and_bad_ones_are_reported() {
        let config = parse(WAIVERS);
        assert_eq!(config.waivers.len(), 1, "{:?}", config.waivers);
        let waiver = &config.waivers[0];
        assert_eq!(waiver.gate(), "pillar_lost");
        assert_eq!(waiver.path(), "src/legacy/**");
        assert_eq!(waiver.reason(), "being rewritten in #412");
        assert_eq!(waiver.expires.as_deref(), Some("2026-12-31"));
        let joined = config.warnings.join("\n");
        for needle in [
            "pr_recap.waive #2: missing reason",
            "pr_recap.waive #3: unknown gate",
            "pr_recap.waive #4: expires \"next week\" is not a YYYY-MM-DD date",
        ] {
            assert!(joined.contains(needle), "missing {needle}: {joined}");
        }
        assert_eq!(config.warnings.len(), 3, "{joined}");
    }

    #[test]
    fn waivers_are_not_settings() {
        let config = parse(WAIVERS);
        assert_eq!(config.preset, PrGatePreset::Strict);
        assert!(config.overrides().is_empty());
        assert_eq!(config.label(), "strict");
        assert!(!config.to_commented_toml().contains("waive"));
        assert!(!PrGateConfig::preset_keys().contains(&"waive"));
    }

    #[test]
    fn explicit_keys_apply_on_top_of_the_preset() {
        let config = parse(
            "[pr_recap]\npreset = \"strict\"\n[pr_recap.gates]\ncosmetic = \"info\"\n[pr_recap.score_drop]\nmin_points = 8\n",
        );
        assert_eq!(config.preset, PrGatePreset::Strict);
        assert_eq!(config.severity(GateId::Cosmetic), Severity::Info);
        assert_eq!(config.severity(GateId::Suspicious), Severity::Block);
        assert_eq!(config.score_drop.min_points, 8);
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        let changed: Vec<String> = config.overrides().iter().map(Setting::path).collect();
        assert_eq!(changed, ["gates.cosmetic", "score_drop.min_points"]);
        assert_eq!(config.label(), "strict · 2 changes");
    }

    #[test]
    fn bad_keys_are_dropped_and_reported_without_losing_the_rest() {
        let config = parse(
            "[pr_recap]\npreset = \"lenient\"\nfail_on = \"never\"\ncolour = 1\n[pr_recap.gates]\npilar_lost = \"off\"\ncosmetic = \"loud\"\nsuspicious = \"info\"\n[pr_recap.score_drop]\nmin_points = -3\n",
        );
        assert_eq!(config.preset, PrGatePreset::Recommended);
        assert_eq!(config.fail_on, FailOn::Block);
        assert_eq!(config.severity(GateId::Cosmetic), Severity::Warn);
        assert_eq!(config.severity(GateId::Suspicious), Severity::Info);
        assert_eq!(config.score_drop.min_points, 10);
        let joined = config.warnings.join("\n");
        for needle in [
            "pr_recap.preset",
            "pr_recap.fail_on",
            "pr_recap.colour",
            "pr_recap.gates.pilar_lost: unknown gate",
            "pr_recap.gates.cosmetic",
            "pr_recap.score_drop.min_points",
        ] {
            assert!(joined.contains(needle), "missing {needle}: {joined}");
        }
    }

    #[test]
    fn preset_keys_are_the_top_level_settings_and_tables() {
        assert_eq!(
            PrGateConfig::preset_keys(),
            [
                "fail_on",
                "max_hotspots",
                "gates",
                "score_drop",
                "import_cycle",
                "fan_in_growth"
            ]
        );
    }

    #[test]
    fn commented_block_round_trips_and_lists_every_setting() {
        let mut seeded = PrGateConfig::for_preset(PrGatePreset::Relaxed);
        seeded.preset = PrGatePreset::Custom;
        let text = seeded.to_commented_toml();
        for setting in seeded.settings() {
            assert!(
                text.contains(&format!("{} = ", setting.key)),
                "{} missing:\n{text}",
                setting.key
            );
        }
        assert!(
            text.contains("# [block] an existing file stops clearing"),
            "{text}"
        );
        assert!(text.contains("min_points = 20"), "{text}");
        assert!(text.contains("# [10] smallest drop"), "{text}");

        let reparsed = parse(&text);
        assert_eq!(reparsed.preset, PrGatePreset::Custom);
        assert!(reparsed.warnings.is_empty(), "{:?}", reparsed.warnings);
        assert_eq!(reparsed.gates, seeded.gates);
        assert_eq!(reparsed.score_drop, seeded.score_drop);
        assert_eq!(reparsed.import_cycle, seeded.import_cycle);
        assert_eq!(reparsed.fan_in_growth, seeded.fan_in_growth);
        assert_eq!(reparsed.fail_on, seeded.fail_on);
    }

    #[test]
    fn coupling_gates_default_per_preset() {
        let severity = |preset, gate| PrGateConfig::for_preset(preset).severity(gate);
        use PrGatePreset::{Recommended, Relaxed, Strict};
        let rows = [
            (GateId::ImportCycle, [Info, Warn, Block]),
            (GateId::FanInGrowth, [Info, Warn, Warn]),
            (GateId::BlastRadius, [Off, Info, Info]),
        ];
        for (gate, [relaxed, recommended, strict]) in rows {
            assert_eq!(severity(Relaxed, gate), relaxed, "{}", gate.key());
            assert_eq!(severity(Recommended, gate), recommended, "{}", gate.key());
            assert_eq!(severity(Strict, gate), strict, "{}", gate.key());
        }
        let language = |preset, language| {
            PrGateConfig::for_preset(preset)
                .import_cycle
                .get(language)
                .unwrap()
        };
        assert_eq!(language(Relaxed, "rust"), Off);
        assert_eq!(language(Relaxed, "python"), Warn);
        assert_eq!(language(Recommended, "typescript"), Info);
        assert_eq!(language(Recommended, "python"), Warn);
        assert_eq!(language(Strict, "javascript"), Warn);
        assert_eq!(language(Strict, "python"), Block);
        for preset in PrGatePreset::ALL {
            let config = PrGateConfig::for_preset(preset);
            assert_eq!(config.fan_in_growth.min_new_dependents, 2);
            assert_eq!(config.fan_in_growth.min_growth_percent, 25);
        }
    }

    #[test]
    fn a_cycle_takes_its_strictest_language_and_falls_back_to_the_gate() {
        let config = PrGateConfig::default();
        assert_eq!(config.cycle_severity(["a.rs", "b.rs"]), Info);
        assert_eq!(config.cycle_severity(["a.rs", "b.py"]), Warn);
        // Go has no row, and a Markdown file has no language: the gate.
        assert_eq!(config.cycle_severity(["a.go", "b.rs"]), Warn);
        assert_eq!(config.cycle_severity(["README.md", "a.ts"]), Warn);
        assert_eq!(config.cycle_severity([]), Warn);
    }

    #[test]
    fn coupling_tables_parse_and_bad_keys_are_reported() {
        let config = parse(
            "[pr_recap]\n[pr_recap.import_cycle]\nrust = \"block\"\ncobol = \"warn\"\npython = \"loud\"\n[pr_recap.fan_in_growth]\nmin_new_dependents = 4\nmin_growth_percent = -1\nmax = 3\n",
        );
        assert_eq!(config.import_cycle.get("rust"), Some(Block));
        assert_eq!(config.import_cycle.get("python"), Some(Warn));
        assert_eq!(config.fan_in_growth.min_new_dependents, 4);
        assert_eq!(config.fan_in_growth.min_growth_percent, 25);
        let joined = config.warnings.join("\n");
        for needle in [
            "pr_recap.import_cycle.cobol: unknown language, ignored",
            "pr_recap.import_cycle.python",
            "pr_recap.fan_in_growth.min_growth_percent",
            "pr_recap.fan_in_growth.max: unknown setting",
        ] {
            assert!(joined.contains(needle), "missing {needle}: {joined}");
        }
        assert_eq!(config.warnings.len(), 4, "{joined}");
        let changed: Vec<String> = config.overrides().iter().map(Setting::path).collect();
        assert_eq!(
            changed,
            ["import_cycle.rust", "fan_in_growth.min_new_dependents"]
        );
    }
}
