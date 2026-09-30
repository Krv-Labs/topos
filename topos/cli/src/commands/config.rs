//! `topos config` — scriptable project settings plus a short TTY wizard.

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use console::{Style, Term};
use toml_edit::{value, Array, Decor, DocumentMut, Item, Table, Value};
use topos_engine::config::{
    find_config_file, load_topos_config, PrGateConfig, PrGatePreset, ToposConfig, PRESET_COMMENT,
};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::preferences::{
    default_preferences, Generator, UserPreferences, RANKING_LEN,
};

use super::menu::{self, SelectOption, SelectStep, StepLayout};
use super::render::{guide, guide_line, paint, print_lines, RenderOptions};

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: Option<ConfigCommand>,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print the resolved project settings, including every PR gate setting.
    Show,
    /// Update the evaluation priority and/or the PR gate preset in
    /// `.topos.toml`, in one write.
    Set(ConfigSetArgs),
}

#[derive(Args)]
struct ConfigSetArgs {
    /// Pillar to prioritize (simple, composable, secure, navigable), or a
    /// full comma-separated ranking, most important first.
    #[arg(long, value_name = "PILLAR|SIMPLE,COMPOSABLE,SECURE,NAVIGABLE")]
    priority: Option<String>,
    /// PR gate preset for `topos pr-recap`. `custom` writes every gate
    /// setting under `[pr_recap]` so you can edit them in the file.
    #[arg(long, value_name = "relaxed|recommended|strict|custom")]
    pr_preset: Option<String>,
}

pub(crate) fn run(args: ConfigArgs) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("resolving current directory: {e}"))?;
    match args.command {
        Some(ConfigCommand::Show) => show(&cwd),
        Some(ConfigCommand::Set(set)) => set_config(&cwd, set),
        None if Term::stderr().is_term() => interactive(&cwd),
        None => show(&cwd),
    }
}

fn show(cwd: &Path) -> Result<(), String> {
    let config = load_topos_config(cwd);
    let path = find_config_file(cwd);
    print_lines(show_lines(
        &config,
        path.as_deref(),
        RenderOptions::stdout(),
    ));
    Ok(())
}

fn show_lines(config: &ToposConfig, path: Option<&Path>, options: RenderOptions) -> Vec<String> {
    let mut lines = vec![
        paint("◇  Topos project settings", Style::new().bold(), options),
        guide_line(
            path.map_or_else(
                || "defaults · no .topos.toml".to_string(),
                |p| p.display().to_string(),
            ),
            Style::new().dim(),
            options,
        ),
        guide('│', options),
    ];
    lines.extend(evaluation_rows(config, options));
    lines.push(guide('│', options));
    lines.extend(pr_gate_rows(&config.pr_recap, options));
    lines.push(guide('└', options));
    lines.push(String::new());
    let file = path.map_or_else(|| ".topos.toml".to_string(), |p| p.display().to_string());
    let other = match config.pr_recap.preset {
        PrGatePreset::Strict => PrGatePreset::Recommended,
        _ => PrGatePreset::Strict,
    };
    lines.push(paint(
        format!(
            "Tip: edit [pr_recap] in {file}, or switch presets with topos config set --pr-preset {}.",
            other.as_str()
        ),
        Style::new().dim(),
        options,
    ));
    lines
}

fn evaluation_rows(config: &ToposConfig, options: RenderOptions) -> [String; 2] {
    [
        guide_line(
            format!(
                "priority     {}",
                priority_name(config.effective_priority())
            ),
            Style::new(),
            options,
        ),
        guide_line(
            format!(
                "preferences  {}",
                config
                    .preferences
                    .map_or_else(|| "not set".to_string(), ranking_text)
            ),
            Style::new(),
            options,
        ),
    ]
}

/// The `PR GATE` section of `config show`: every setting with its active
/// value, the ones that differ from the preset in yellow, the waivers,
/// then any keys the parser dropped.
fn pr_gate_rows(gate: &PrGateConfig, options: RenderOptions) -> Vec<String> {
    let rail = guide('│', options);
    let mut lines = vec![format!(
        "{rail}  {}  {}",
        paint("PR GATE", Style::new().cyan().bold(), options),
        paint(gate.label(), Style::new().dim(), options)
    )];
    let settings = gate.settings();
    // One column for every path, sized by the longest setting path.
    let width = settings.iter().map(|s| s.path().len()).max().unwrap_or(0);
    for setting in settings {
        let value = format!("{:<6}", setting.value.to_string());
        let (value, note) = if setting.is_changed() {
            (
                paint(value, Style::new().yellow().bold(), options),
                format!("(preset: {}) {}", setting.preset_value, setting.describe),
            )
        } else {
            (value, setting.describe.to_string())
        };
        lines.push(format!(
            "{rail}  {:<width$} {value} {}",
            setting.path(),
            paint(note, Style::new().dim(), options)
        ));
    }
    for waiver in &gate.waivers {
        let expires = waiver
            .expires
            .as_deref()
            .map_or_else(String::new, |date| format!(" (expires {date})"));
        lines.push(format!(
            "{rail}  {:<width$} {} {}",
            format!("waive.{}", waiver.gate()),
            waiver.path(),
            paint(
                format!("{}{expires}", waiver.reason()),
                Style::new().dim(),
                options
            )
        ));
    }
    if !gate.warnings.is_empty() {
        lines.push(rail.clone());
        for warning in &gate.warnings {
            lines.push(format!(
                "{rail}  {} {}",
                paint('!', Style::new().yellow().bold(), options),
                paint(warning, Style::new().yellow(), options)
            ));
        }
    }
    lines
}

fn set_config(cwd: &Path, args: ConfigSetArgs) -> Result<(), String> {
    if args.priority.is_none() && args.pr_preset.is_none() {
        return Err("config set requires --priority or --pr-preset".to_string());
    }
    let current = load_topos_config(cwd);
    let ranking = args
        .priority
        .as_deref()
        .map(|raw| {
            parse_priority_input(raw).map(|input| match input {
                PriorityInput::Ranking(ranking) => ranking,
                PriorityInput::Single(priority) => resolved_ranking(&current, priority),
            })
        })
        .transpose()?;
    let preset = args.pr_preset.as_deref().map(parse_pr_preset).transpose()?;
    let path = config_path(cwd);
    let mut document = load_document(&path)?;
    if let Some(ranking) = ranking {
        apply_priority(&mut document, ranking);
    }
    if let Some(preset) = preset {
        apply_pr_gate(&mut document, &current.pr_recap, preset);
    }
    save(&path, &document)?;
    print_lines(updated_lines(
        &path,
        &load_topos_config(cwd),
        RenderOptions::stdout(),
    ));
    Ok(())
}

/// The two-step wizard: evaluation priority, then PR gate preset. Nothing
/// is written until the last step is confirmed; Esc at either step leaves
/// the file untouched.
fn interactive(cwd: &Path) -> Result<(), String> {
    let term = Term::stderr();
    let options = RenderOptions::stderr();
    let config = load_topos_config(cwd);
    let mut header = vec![
        paint("┌  Topos project settings", Style::new().bold(), options),
        guide('│', options),
    ];
    let Some(choice) = menu::run_select(&header, &priority_step(config.effective_priority()))?
    else {
        return unchanged(&term, options);
    };
    let priority = PRIORITY_CHOICES[choice].0;
    header.push(guide_line(
        format!("priority     {}", priority_name(priority)),
        Style::new(),
        options,
    ));
    header.push(guide('│', options));
    let Some(choice) = menu::run_select(&header, &pr_gate_step(config.pr_recap.preset))? else {
        return unchanged(&term, options);
    };
    let preset = PR_GATE_CHOICES[choice].0;

    let path = config_path(cwd);
    let mut document = load_document(&path)?;
    apply_priority(&mut document, resolved_ranking(&config, priority));
    apply_pr_gate(&mut document, &config.pr_recap, preset);
    save(&path, &document)?;

    let mut lines = updated_lines(&path, &load_topos_config(cwd), options);
    lines.push(String::new());
    let tip = if preset == PrGatePreset::Custom {
        format!(
            "Tip: every PR gate setting is under [pr_recap] in {}, with its default in brackets. Edit the values there.",
            path.display()
        )
    } else {
        "Tip: topos config show lists every PR gate setting; choose Custom to edit them in the file."
            .to_string()
    };
    lines.push(paint(tip, Style::new().dim(), options));
    for line in lines {
        term.write_line(&line).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn unchanged(term: &Term, options: RenderOptions) -> Result<(), String> {
    term.write_line(&paint("◇  Settings unchanged", Style::new().dim(), options))
        .map_err(|e| e.to_string())
}

/// The frame `config set` and the wizard print after a write, read back
/// from the saved file.
fn updated_lines(path: &Path, config: &ToposConfig, options: RenderOptions) -> Vec<String> {
    let mut lines = vec![
        paint("◇  Project settings updated", Style::new().bold(), options),
        guide_line(path.display(), Style::new().dim(), options),
        guide('│', options),
    ];
    lines.extend(evaluation_rows(config, options));
    lines.push(guide_line(
        format!("pr gate      {}", config.pr_recap.label()),
        Style::new(),
        options,
    ));
    lines.push(guide('└', options));
    lines
}

/// The pillars offered by the interactive selector, in display order,
/// each with the label and one-line hint shown beside it.
const PRIORITY_CHOICES: [(Priority, &str, &str); RANKING_LEN] = [
    (
        Priority::Simple,
        "Simple",
        "favor low complexity and readable structure",
    ),
    (
        Priority::Composable,
        "Composable",
        "favor clean module boundaries and coupling",
    ),
    (
        Priority::Secure,
        "Secure",
        "favor safe data flow and dangerous-call review",
    ),
    (
        Priority::Navigable,
        "Navigable",
        "favor shallow nesting an agent can read in one pass",
    ),
];

fn priority_step(current: Priority) -> SelectStep {
    SelectStep {
        title: "Evaluation priority",
        keys: "↑↓ move · enter next · esc cancel",
        options: PRIORITY_CHOICES
            .iter()
            .map(|&(priority, label, hint)| SelectOption {
                label,
                hint: hint.to_string(),
                current: priority == current,
                key: None,
            })
            .collect(),
        initial: PRIORITY_CHOICES
            .iter()
            .position(|(priority, ..)| *priority == current)
            .unwrap_or(0),
        layout: StepLayout::Wizard,
    }
}

/// The PR gate presets offered by the wizard, in display order.
const PR_GATE_CHOICES: [(PrGatePreset, &str); 4] = [
    (PrGatePreset::Recommended, "Recommended"),
    (PrGatePreset::Strict, "Strict"),
    (PrGatePreset::Relaxed, "Relaxed"),
    (PrGatePreset::Custom, "Custom"),
];

fn pr_gate_step(current: PrGatePreset) -> SelectStep {
    SelectStep {
        title: "PR gate · topos pr-recap",
        keys: "↑↓ move · enter save · esc cancel (nothing saved)",
        options: PR_GATE_CHOICES
            .iter()
            .map(|&(preset, label)| SelectOption {
                label,
                hint: preset_hint(preset),
                current: preset == current,
                key: None,
            })
            .collect(),
        initial: PR_GATE_CHOICES
            .iter()
            .position(|(preset, _)| *preset == current)
            .unwrap_or(0),
        layout: StepLayout::Wizard,
    }
}

/// One line on what a preset does, with its numbers read from the preset
/// itself so the wizard never drifts from the defaults.
fn preset_hint(preset: PrGatePreset) -> String {
    let points = PrGateConfig::for_preset(preset).score_drop.min_points;
    match preset {
        PrGatePreset::Recommended => {
            format!("block lost pillars and risky new files · warn on drops of {points}+ points")
        }
        PrGatePreset::Strict => {
            format!("warnings fail the check too · drops of {points}+ points count")
        }
        PrGatePreset::Relaxed => format!(
            "block lost pillars and risky new files · warn only on drops of {points}+ points"
        ),
        PrGatePreset::Custom => "write every setting to .topos.toml and edit it there".to_string(),
    }
}

fn parse_pr_preset(value: &str) -> Result<PrGatePreset, String> {
    PrGatePreset::parse(value).ok_or_else(|| {
        format!(
            "invalid PR gate preset '{value}' (expected relaxed, recommended, strict, or custom)"
        )
    })
}

fn load_document(path: &Path) -> Result<DocumentMut, String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|e| format!("parsing {}: {e}", path.display()))
}

fn save(path: &Path, document: &DocumentMut) -> Result<(), String> {
    fs::write(path, document.to_string()).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Set evaluation settings using the canonical single-key schema:
/// `priority` is the full ranking array. Legacy `preferences` is removed so
/// the file has one source of truth that `load_topos_config` round-trips.
fn apply_priority(document: &mut DocumentMut, ranking: [Generator; RANKING_LEN]) {
    if !document.as_table().contains_key("evaluation") {
        let mut table = Table::new();
        table.set_position(next_position(document.as_table()));
        document["evaluation"] = Item::Table(table);
    }
    let mut values = Array::new();
    for generator in ranking {
        values.push(generator.as_str());
    }
    document["evaluation"]["priority"] = value(values);
    if let Some(evaluation) = document["evaluation"].as_table_mut() {
        evaluation.remove("preferences");
    }
}

/// Point `[pr_recap]` at `preset`. A named preset drops the keys it
/// controls, so the project picks up improved defaults; any other key or
/// table under `[pr_recap]` stays. `custom` writes every setting, seeded
/// from `active`, with its default and meaning beside it; keys the file
/// already has keep their values.
fn apply_pr_gate(document: &mut DocumentMut, active: &PrGateConfig, preset: PrGatePreset) {
    let mut next = next_position(document.as_table());
    let table = pr_recap_table(document, &mut next);
    if preset == PrGatePreset::Custom {
        let seeded = PrGateConfig {
            preset,
            ..active.clone()
        };
        let block = seeded
            .to_commented_toml()
            .parse::<DocumentMut>()
            .expect("the generated [pr_recap] block is valid TOML");
        if let Some(source) = block["pr_recap"].as_table() {
            insert_missing(table, source, &mut next);
        }
    } else {
        let owned = PrGateConfig::preset_keys();
        table.retain(|key, _| !owned.contains(&key));
    }
    set_preset(table, preset);
}

/// `[pr_recap]` as a standard table, created at the end of the document
/// when missing.
fn pr_recap_table<'a>(document: &'a mut DocumentMut, next: &mut usize) -> &'a mut Table {
    let item = document
        .as_table_mut()
        .entry("pr_recap")
        .or_insert(Item::None);
    if !item.is_table() {
        // An inline table keeps its keys; anything else starts over.
        let mut table = std::mem::take(item).into_table().unwrap_or_default();
        table.set_position(*next);
        *next += 1;
        *item = Item::Table(table);
    }
    let table = item
        .as_table_mut()
        .expect("[pr_recap] was just made a table");
    table.set_implicit(false);
    table.set_dotted(false);
    table
}

/// Replace the `preset` value, keeping any comment written beside it.
/// A named preset drops the comment `custom` generated, which describes
/// keys that are now gone; a comment the user wrote always stays.
fn set_preset(table: &mut Table, preset: PrGatePreset) {
    if let Some(Item::Value(existing)) = table.get_mut("preset") {
        let mut decor = existing.decor().clone();
        if preset != PrGatePreset::Custom && has_generated_comment(&decor) {
            decor.set_suffix("");
        }
        *existing = Value::from(preset.as_str());
        *existing.decor_mut() = decor;
    } else {
        table.insert("preset", value(preset.as_str()));
    }
}

/// Whether the comment after a value is [`PRESET_COMMENT`], ignoring the
/// padding and `#` around it.
fn has_generated_comment(decor: &Decor) -> bool {
    decor
        .suffix()
        .and_then(|suffix| suffix.as_str())
        .and_then(|suffix| suffix.trim().strip_prefix('#'))
        .is_some_and(|comment| comment.trim() == PRESET_COMMENT)
}

/// Copy into `target` every key of `source` it lacks, comments included,
/// recursing into tables both have. Existing values always win.
fn insert_missing(target: &mut Table, source: &Table, next: &mut usize) {
    for (key, item) in source.iter() {
        match (target.get_mut(key), item) {
            (Some(Item::Table(existing)), Item::Table(nested)) => {
                insert_missing(existing, nested, next);
            }
            (Some(_), _) => {}
            (None, _) => {
                let (key, _) = source
                    .get_key_value(key)
                    .expect("key comes from iterating source");
                let mut item = item.clone();
                if let Item::Table(table) = &mut item {
                    place_at_end(table, next);
                }
                target.insert_formatted(key, item);
            }
        }
    }
}

/// Tables print in `position` order, and a table copied from another
/// document keeps the position it had there. Renumber `table` and its
/// subtables so they print after everything already in the document.
fn place_at_end(table: &mut Table, next: &mut usize) {
    table.set_position(*next);
    *next += 1;
    for (_, item) in table.iter_mut() {
        if let Item::Table(nested) = item {
            place_at_end(nested, next);
        }
    }
}

/// One past the highest table position in `table` and below it.
fn next_position(table: &Table) -> usize {
    let mut next = table.position().map_or(0, |p| p + 1);
    for (_, item) in table.iter() {
        match item {
            Item::Table(nested) => next = next.max(next_position(nested)),
            Item::ArrayOfTables(array) => {
                for nested in array.iter() {
                    next = next.max(next_position(nested));
                }
            }
            _ => {}
        }
    }
    next
}

fn config_path(cwd: &Path) -> PathBuf {
    find_config_file(cwd).unwrap_or_else(|| cwd.join(".topos.toml"))
}

pub(crate) fn parse_priority(value: &str) -> Result<Priority, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "simple" => Ok(Priority::Simple),
        "composable" => Ok(Priority::Composable),
        "secure" => Ok(Priority::Secure),
        "navigable" => Ok(Priority::Navigable),
        _ => Err(format!(
            "invalid priority '{value}' (expected simple, composable, secure, or navigable)"
        )),
    }
}

pub(crate) fn parse_ranking(value: &str) -> Result<[Generator; RANKING_LEN], String> {
    let parsed: Vec<Generator> = value
        .split(',')
        .map(|part| {
            let name = part.trim().to_ascii_lowercase();
            Generator::ALL
                .into_iter()
                .find(|g| g.as_str() == name)
                .ok_or_else(|| format!("invalid preference '{name}'"))
        })
        .collect::<Result<_, _>>()?;
    let ranking: [Generator; RANKING_LEN] = parsed.try_into().map_err(|values: Vec<_>| {
        format!(
            "preferences require all {RANKING_LEN} pillars exactly once (got {})",
            values.len()
        )
    })?;
    UserPreferences::new(ranking)
        .map(|prefs| prefs.ranking())
        .map_err(|e| e.to_string())
}

/// One `--priority` value: either a single pillar, or a full ranking
/// (comma-separated, most important first).
pub(crate) enum PriorityInput {
    Single(Priority),
    Ranking([Generator; RANKING_LEN]),
}

pub(crate) fn parse_priority_input(value: &str) -> Result<PriorityInput, String> {
    if value.contains(',') {
        parse_ranking(value).map(PriorityInput::Ranking)
    } else {
        parse_priority(value).map(PriorityInput::Single)
    }
}

pub(crate) fn priority_for_generator(generator: Generator) -> Priority {
    match generator {
        Generator::Simple => Priority::Simple,
        Generator::Composable => Priority::Composable,
        Generator::Secure => Priority::Secure,
        Generator::Navigable => Priority::Navigable,
    }
}

pub(crate) fn priority_name(priority: Priority) -> &'static str {
    priority.top_generator().as_str()
}

fn generator_for_priority(priority: Priority) -> Generator {
    priority.top_generator()
}

fn default_ranking() -> [Generator; RANKING_LEN] {
    default_preferences().ranking()
}

/// Ranking to persist for `priority`, preserving the relative order of the
/// other pillars from `config` rather than resetting to the default.
fn resolved_ranking(config: &ToposConfig, priority: Priority) -> [Generator; RANKING_LEN] {
    let base = config.preferences.unwrap_or_else(default_ranking);
    move_first(base, generator_for_priority(priority))
}

fn move_first(ranking: [Generator; RANKING_LEN], first: Generator) -> [Generator; RANKING_LEN] {
    let mut out = vec![first];
    out.extend(ranking.into_iter().filter(|g| *g != first));
    out.try_into()
        .expect("moving one member of a permutation to the front keeps its length")
}

fn ranking_text(ranking: [Generator; RANKING_LEN]) -> String {
    ranking.map(Generator::as_str).join(" > ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_priority(dir: &Path, ranking: [Generator; RANKING_LEN]) {
        let path = config_path(dir);
        let mut document = load_document(&path).unwrap();
        apply_priority(&mut document, ranking);
        save(&path, &document).unwrap();
    }

    /// A temp project whose `.topos.toml` holds `source`.
    fn project(name: &str, source: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos-cli-config-{name}-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".topos.toml"), source).unwrap();
        dir
    }

    /// Apply `preset` the way `config set --pr-preset` does and return the file.
    fn write_pr_gate(dir: &Path, preset: PrGatePreset) -> String {
        let path = config_path(dir);
        let mut document = load_document(&path).unwrap();
        apply_pr_gate(&mut document, &load_topos_config(dir).pr_recap, preset);
        save(&path, &document).unwrap();
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn ranking_requires_a_permutation() {
        assert!(parse_ranking("secure,simple,composable,navigable").is_ok());
        assert!(parse_ranking("secure,secure,simple,navigable").is_err());
        assert!(parse_ranking("secure,simple").is_err());
    }

    #[test]
    fn moving_priority_preserves_the_other_order() {
        assert_eq!(
            move_first(default_ranking(), Generator::Secure),
            [
                Generator::Secure,
                Generator::Simple,
                Generator::Navigable,
                Generator::Composable
            ]
        );
    }

    #[test]
    fn writing_settings_preserves_unrelated_content_and_comments() {
        let dir = std::env::temp_dir().join(format!(
            "topos-cli-config-write-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".topos.toml");
        fs::write(
            &path,
            "# keep this\n[[secure.allow]]\npattern = \"eval\"\nreason = \"trusted\"\n",
        )
        .unwrap();

        write_priority(
            &dir,
            [
                Generator::Secure,
                Generator::Simple,
                Generator::Composable,
                Generator::Navigable,
            ],
        );

        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# keep this"));
        assert!(updated.contains("[[secure.allow]]"));
        assert!(
            updated.contains("priority = [\"secure\", \"simple\", \"composable\", \"navigable\"]")
        );
        assert!(!updated.contains("preferences"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn writing_settings_round_trips_through_load_topos_config() {
        let dir = std::env::temp_dir().join(format!(
            "topos-cli-config-roundtrip-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let ranking = [
            Generator::Composable,
            Generator::Secure,
            Generator::Simple,
            Generator::Navigable,
        ];

        write_priority(&dir, ranking);

        let loaded = load_topos_config(&dir);
        assert_eq!(loaded.priority, None);
        assert_eq!(loaded.preferences, Some(ranking));
        assert_eq!(loaded.effective_priority(), Priority::Composable);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn writing_settings_migrates_legacy_preferences_key_away() {
        let dir = std::env::temp_dir().join(format!(
            "topos-cli-config-migrate-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".topos.toml");
        fs::write(
            &path,
            "[evaluation]\npriority = \"simple\"\npreferences = [\"simple\", \"composable\", \"secure\"]\n",
        )
        .unwrap();

        write_priority(
            &dir,
            [
                Generator::Secure,
                Generator::Composable,
                Generator::Simple,
                Generator::Navigable,
            ],
        );

        let updated = fs::read_to_string(path).unwrap();
        assert!(
            updated.contains("priority = [\"secure\", \"composable\", \"simple\", \"navigable\"]")
        );
        assert!(!updated.contains("preferences"));
        let loaded = load_topos_config(&dir);
        assert_eq!(
            loaded.preferences,
            Some([
                Generator::Secure,
                Generator::Composable,
                Generator::Simple,
                Generator::Navigable
            ])
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resolved_ranking_preserves_existing_order_around_the_new_priority() {
        let config = ToposConfig {
            preferences: Some([
                Generator::Composable,
                Generator::Secure,
                Generator::Simple,
                Generator::Navigable,
            ]),
            ..Default::default()
        };
        assert_eq!(
            resolved_ranking(&config, Priority::Secure),
            [
                Generator::Secure,
                Generator::Composable,
                Generator::Simple,
                Generator::Navigable
            ]
        );
    }

    #[test]
    fn resolved_ranking_falls_back_to_default_order_without_existing_preferences() {
        let config = ToposConfig::default();
        assert_eq!(
            resolved_ranking(&config, Priority::Secure),
            [
                Generator::Secure,
                Generator::Simple,
                Generator::Navigable,
                Generator::Composable
            ]
        );
    }

    #[test]
    fn a_single_pillar_parses_as_priority_and_a_list_parses_as_ranking() {
        assert!(matches!(
            parse_priority_input("secure").unwrap(),
            PriorityInput::Single(Priority::Secure)
        ));
        assert!(matches!(
            parse_priority_input("secure,simple,composable,navigable").unwrap(),
            PriorityInput::Ranking([
                Generator::Secure,
                Generator::Simple,
                Generator::Composable,
                Generator::Navigable
            ])
        ));
        // A ranking missing a pillar is not a permutation of G_qual.
        assert!(parse_priority_input("secure,simple,composable").is_err());
    }

    const CUSTOM_WITH_EXTRAS: &str = "\
# keep this
[[secure.allow]]
pattern = \"eval\"
reason = \"trusted\"

[pr_recap]
preset = \"custom\"  # pinned by the team
fail_on = \"warn\"
colour = 1

[pr_recap.gates]
cosmetic = \"off\"

[pr_recap.score_drop]
min_points = 3
";

    #[test]
    fn a_named_preset_strips_every_override_and_round_trips() {
        let dir = project("preset-write", CUSTOM_WITH_EXTRAS);

        let updated = write_pr_gate(&dir, PrGatePreset::Strict);

        assert!(updated.contains("# keep this"), "{updated}");
        assert!(updated.contains("[[secure.allow]]"), "{updated}");
        assert!(
            updated.contains("preset = \"strict\"  # pinned by the team"),
            "{updated}"
        );
        for gone in ["fail_on", "[pr_recap.gates]", "cosmetic", "score_drop"] {
            assert!(!updated.contains(gone), "{gone} survived:\n{updated}");
        }
        // A key the preset does not control is not the preset's to remove.
        assert!(updated.contains("colour = 1"), "{updated}");
        let loaded = load_topos_config(&dir).pr_recap;
        assert_eq!(
            loaded.warnings,
            ["pr_recap.colour: unknown setting, ignored"]
        );
        assert_eq!(
            PrGateConfig {
                warnings: Vec::new(),
                ..loaded.clone()
            },
            PrGateConfig::for_preset(PrGatePreset::Strict)
        );
        assert_eq!(loaded.label(), "strict");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn custom_on_an_empty_file_lists_every_setting_and_round_trips() {
        let dir = project("custom-empty", "");

        let updated = write_pr_gate(&dir, PrGatePreset::Custom);

        let seeded = PrGateConfig {
            preset: PrGatePreset::Custom,
            ..PrGateConfig::default()
        };
        for setting in seeded.settings() {
            assert!(
                updated.contains(&format!("\n{} = ", setting.key)),
                "{} missing:\n{updated}",
                setting.path()
            );
        }
        assert!(updated.starts_with("[pr_recap]\n"), "{updated}");
        assert!(
            updated.contains("# [block] an existing file stops clearing"),
            "{updated}"
        );
        let loaded = load_topos_config(&dir).pr_recap;
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded, seeded);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn custom_keeps_values_the_file_already_sets() {
        let dir = project(
            "custom-merge",
            "[pr_recap.gates]\ncosmetic = \"off\"  # too noisy for us\n",
        );

        let updated = write_pr_gate(&dir, PrGatePreset::Custom);

        assert!(
            updated.contains("cosmetic = \"off\"  # too noisy for us"),
            "{updated}"
        );
        assert_eq!(updated.matches("cosmetic =").count(), 1, "{updated}");
        assert!(updated.contains("suspicious = \"warn\""), "{updated}");
        let loaded = load_topos_config(&dir).pr_recap;
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.preset, PrGatePreset::Custom);
        assert_eq!(
            loaded.severity(topos_engine::config::GateId::Cosmetic),
            topos_engine::config::Severity::Off
        );
        assert_eq!(loaded.label(), "custom · 1 change");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn custom_block_follows_existing_tables_in_order() {
        let dir = project(
            "custom-order",
            "# keep this\n[evaluation]\npriority = \"secure\"\n\n[[secure.allow]]\npattern = \"eval\"\nreason = \"trusted\"\n",
        );

        let updated = write_pr_gate(&dir, PrGatePreset::Custom);

        let at = |needle: &str| {
            updated
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} missing:\n{updated}"))
        };
        assert!(at("# keep this") < at("[evaluation]"), "{updated}");
        assert!(at("[evaluation]") < at("[[secure.allow]]"), "{updated}");
        assert!(at("[[secure.allow]]") < at("[pr_recap]"), "{updated}");
        assert!(at("[pr_recap]") < at("[pr_recap.gates]"), "{updated}");
        assert!(
            at("[pr_recap.gates]") < at("[pr_recap.score_drop]"),
            "{updated}"
        );
        assert!(
            updated.contains("reason = \"trusted\"\n\n[pr_recap]\n"),
            "{updated}"
        );
        assert!(updated.contains("\n\n[pr_recap.gates]"), "{updated}");
        assert!(!updated.contains("\n\n\n"), "{updated}");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn switching_from_custom_to_a_preset_removes_the_block() {
        let dir = project(
            "custom-to-preset",
            "[[secure.allow]]\npattern = \"eval\"\nreason = \"trusted\"\n",
        );
        write_pr_gate(&dir, PrGatePreset::Custom);

        let updated = write_pr_gate(&dir, PrGatePreset::Recommended);

        assert!(updated.contains("preset = \"recommended\""), "{updated}");
        assert!(updated.contains("[[secure.allow]]"), "{updated}");
        assert!(!updated.contains("[pr_recap."), "{updated}");
        assert!(!updated.contains("fail_on"), "{updated}");
        assert_eq!(load_topos_config(&dir).pr_recap, PrGateConfig::default());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_named_preset_drops_the_generated_preset_comment_only() {
        let dir = project("preset-comment", "");
        write_pr_gate(&dir, PrGatePreset::Custom);

        let updated = write_pr_gate(&dir, PrGatePreset::Strict);

        assert!(updated.contains("preset = \"strict\"\n"), "{updated}");
        assert!(!updated.contains(PRESET_COMMENT), "{updated}");

        fs::write(
            dir.join(".topos.toml"),
            "[pr_recap]\npreset = \"custom\"  # team policy\n",
        )
        .unwrap();
        let updated = write_pr_gate(&dir, PrGatePreset::Strict);

        assert!(
            updated.contains("preset = \"strict\"  # team policy"),
            "{updated}"
        );
        fs::remove_dir_all(dir).ok();
    }

    const PRESET_WITH_FUTURE_TABLES: &str = "\
[pr_recap]
preset = \"custom\"
fail_on = \"warn\"

[pr_recap.gates]
cosmetic = \"off\"

[[pr_recap.waive]]
gate = \"pillar_lost\"
path = \"src/legacy/**\"
reason = \"x\"

[[pr_recap.waive]]
gate = \"cosmetic\"
path = \"src/gen/**\"
reason = \"y\"

[pr_recap.import_cycle]
rust = \"info\"

[pr_recap.team]
owner = \"x\"
";

    #[test]
    fn a_named_preset_keeps_tables_it_does_not_own() {
        let dir = project("preset-keeps-tables", PRESET_WITH_FUTURE_TABLES);

        let updated = write_pr_gate(&dir, PrGatePreset::Recommended);

        assert!(updated.contains("preset = \"recommended\""), "{updated}");
        for gone in [
            "fail_on",
            "[pr_recap.gates]",
            "cosmetic = ",
            "[pr_recap.import_cycle]",
        ] {
            assert!(!updated.contains(gone), "{gone} survived:\n{updated}");
        }
        for kept in [
            "[[pr_recap.waive]]\ngate = \"pillar_lost\"\npath = \"src/legacy/**\"\nreason = \"x\"\n",
            "[[pr_recap.waive]]\ngate = \"cosmetic\"\npath = \"src/gen/**\"\nreason = \"y\"\n",
            "[pr_recap.team]\nowner = \"x\"\n",
        ] {
            assert!(updated.contains(kept), "lost {kept:?}:\n{updated}");
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn switching_presets_keeps_the_waivers() {
        // Custom keeps the file's two settings; the waivers are not a third.
        for (preset, label) in [
            (PrGatePreset::Custom, "custom · 2 changes"),
            (PrGatePreset::Strict, "strict"),
        ] {
            let dir = project(
                &format!("preset-keeps-waivers-{}", preset.as_str()),
                PRESET_WITH_FUTURE_TABLES,
            );

            write_pr_gate(&dir, preset);

            let gate = load_topos_config(&dir).pr_recap;
            let waived: Vec<(&str, &str, &str)> = gate
                .waivers
                .iter()
                .map(|waiver| (waiver.gate(), waiver.path(), waiver.reason()))
                .collect();
            assert_eq!(
                waived,
                [
                    ("pillar_lost", "src/legacy/**", "x"),
                    ("cosmetic", "src/gen/**", "y")
                ],
                "{preset:?}"
            );
            assert!(
                !gate.warnings.iter().any(|w| w.contains("waive")),
                "{preset:?}: {:?}",
                gate.warnings
            );
            assert_eq!(gate.label(), label, "{preset:?}");
            fs::remove_dir_all(dir).ok();
        }
    }

    #[test]
    fn a_custom_import_cycle_language_round_trips_and_a_preset_resets_it() {
        use topos_engine::config::Severity;
        let dir = project(
            "import-cycle-language",
            "[pr_recap]\npreset = \"custom\"\n\n[pr_recap.import_cycle]\npython = \"block\"\n",
        );

        let custom = write_pr_gate(&dir, PrGatePreset::Custom);
        assert!(custom.contains("[pr_recap.import_cycle]"), "{custom}");
        assert!(custom.contains("[pr_recap.fan_in_growth]"), "{custom}");
        let gate = load_topos_config(&dir).pr_recap;
        assert!(gate.warnings.is_empty(), "{:?}", gate.warnings);
        assert_eq!(gate.import_cycle.get("python"), Some(Severity::Block));
        assert_eq!(gate.import_cycle.get("rust"), Some(Severity::Info));
        assert_eq!(gate.label(), "custom · 1 change");

        let relaxed = write_pr_gate(&dir, PrGatePreset::Relaxed);
        assert!(!relaxed.contains("import_cycle"), "{relaxed}");
        let gate = load_topos_config(&dir).pr_recap;
        assert_eq!(gate.import_cycle.get("python"), Some(Severity::Warn));
        assert_eq!(gate.label(), "relaxed");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn show_lists_the_waivers() {
        let config = ToposConfig {
            pr_recap: PrGateConfig::from_table(
                &"[[waive]]\ngate = \"pillar_lost\"\npath = \"src/legacy/**\"\nreason = \"vendored\"\nexpires = \"2026-12-31\"\n"
                    .parse()
                    .unwrap(),
            ),
            ..Default::default()
        };
        let options = RenderOptions {
            styled: false,
            width: 100,
        };

        let lines = show_lines(&config, None, options);

        assert!(
            lines.contains(&"│  PR GATE  recommended".to_string()),
            "{lines:#?}"
        );
        assert!(
            lines.contains(
                &"│  waive.pillar_lost                src/legacy/** vendored (expires 2026-12-31)"
                    .to_string()
            ),
            "{lines:#?}"
        );
    }

    #[test]
    fn show_lists_every_gate_setting_and_marks_changes() {
        let config = ToposConfig {
            pr_recap: PrGateConfig::from_table(
                &"preset = \"strict\"\ncolour = 1\n[gates]\ncosmetic = \"info\"\n"
                    .parse()
                    .unwrap(),
            ),
            ..Default::default()
        };
        let options = RenderOptions {
            styled: false,
            width: 80,
        };

        let lines = show_lines(&config, Some(Path::new("/repo/.topos.toml")), options);

        assert_eq!(lines[0], "◇  Topos project settings");
        assert_eq!(lines[1], "│  /repo/.topos.toml");
        assert!(
            lines.contains(&"│  PR GATE  strict · 1 change".to_string()),
            "{lines:#?}"
        );
        let cosmetic = lines.iter().find(|l| l.contains("gates.cosmetic")).unwrap();
        assert_eq!(
            cosmetic,
            "│  gates.cosmetic                   info   (preset: block) a score moved but the code structure did not"
        );
        let unchanged = lines
            .iter()
            .find(|l| l.contains("gates.pillar_lost"))
            .unwrap();
        assert!(!unchanged.contains("preset:"), "{unchanged}");
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("│  gates.")).count(),
            topos_engine::config::GATE_COUNT
        );
        assert!(
            lines.contains(&"│  ! pr_recap.colour: unknown setting, ignored".to_string()),
            "{lines:#?}"
        );
        let end = lines.iter().position(|l| l == "└").unwrap();
        assert_eq!(lines[end + 1], "");
        assert_eq!(
            lines[end + 2],
            "Tip: edit [pr_recap] in /repo/.topos.toml, or switch presets with topos config set --pr-preset recommended."
        );
        assert_eq!(lines.len(), end + 3);
    }
}
