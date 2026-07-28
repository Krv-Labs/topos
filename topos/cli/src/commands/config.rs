//! `topos config` — scriptable project settings plus a small TTY selector.

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use console::{Key, Term};
use toml_edit::{value, Array, DocumentMut, Item, Table};
use topos_engine::config::{find_config_file, load_topos_config, ToposConfig};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::preferences::{Generator, UserPreferences};

use super::render::{guide, guide_line, paint, RenderOptions};

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: Option<ConfigCommand>,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print the resolved project settings.
    Show,
    /// Update evaluation settings in `.topos.toml`.
    Set(ConfigSetArgs),
}

#[derive(Args)]
struct ConfigSetArgs {
    /// Pillar to prioritize (simple, composable, secure), or a full
    /// comma-separated ranking, most important first.
    #[arg(long, value_name = "PILLAR|SIMPLE,COMPOSABLE,SECURE")]
    priority: Option<String>,
}

pub(crate) fn run(args: ConfigArgs) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("resolving current directory: {e}"))?;
    match args.command {
        Some(ConfigCommand::Show) => show(&cwd),
        Some(ConfigCommand::Set(set)) => set_config(&cwd, set.priority),
        None if Term::stderr().is_term() => interactive(&cwd),
        None => show(&cwd),
    }
}

fn show(cwd: &Path) -> Result<(), String> {
    let config = load_topos_config(cwd);
    let path = find_config_file(cwd);
    let options = RenderOptions::stdout();
    println!(
        "{}",
        paint(
            "◇  Topos project settings",
            console::Style::new().bold(),
            options
        )
    );
    println!(
        "{}",
        guide_line(
            path.map_or_else(
                || "defaults · no .topos.toml".to_string(),
                |p| p.display().to_string()
            ),
            console::Style::new().dim(),
            options,
        )
    );
    println!("{}", guide('│', options));
    println!(
        "{}",
        guide_line(
            format!(
                "priority     {}",
                priority_name(config.effective_priority())
            ),
            console::Style::new(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!(
                "preferences  {}",
                config
                    .preferences
                    .map_or_else(|| "not set".to_string(), ranking_text)
            ),
            console::Style::new(),
            options,
        )
    );
    println!("{}", guide('└', options));
    Ok(())
}

fn set_config(cwd: &Path, raw_priority: Option<String>) -> Result<(), String> {
    let Some(raw_priority) = raw_priority else {
        return Err("config set requires --priority".to_string());
    };
    let current = load_topos_config(cwd);
    let (priority, ranking) = match parse_priority_input(&raw_priority)? {
        PriorityInput::Ranking(ranking) => (priority_for_generator(ranking[0]), ranking),
        PriorityInput::Single(priority) => (priority, resolved_ranking(&current, priority)),
    };
    write_settings(cwd, ranking)?;
    let options = RenderOptions::stdout();
    println!(
        "{}",
        paint(
            "◇  Project settings updated",
            console::Style::new().bold(),
            options
        )
    );
    println!(
        "{}",
        guide_line(
            config_path(cwd).display(),
            console::Style::new().dim(),
            options
        )
    );
    println!("{}", guide('│', options));
    println!(
        "{}",
        guide_line(
            format!("priority     {}", priority_name(priority)),
            console::Style::new(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!("preferences  {}", ranking_text(ranking)),
            console::Style::new(),
            options,
        )
    );
    println!("{}", guide('└', options));
    Ok(())
}

fn interactive(cwd: &Path) -> Result<(), String> {
    let term = Term::stderr();
    let config = load_topos_config(cwd);
    let current = config.effective_priority();
    let choices = [Priority::Simple, Priority::Composable, Priority::Secure];
    let mut selected = choices.iter().position(|p| *p == current).unwrap_or(2);
    let mut rendered = 0;
    term.hide_cursor().map_err(|e| e.to_string())?;
    let result = (|| -> Result<Option<Priority>, String> {
        loop {
            if rendered > 0 {
                term.clear_last_lines(rendered).map_err(|e| e.to_string())?;
            }
            let lines = selector_lines(selected, RenderOptions::stderr());
            rendered = lines.len();
            for line in lines {
                term.write_line(&line).map_err(|e| e.to_string())?;
            }
            match term.read_key().map_err(|e| e.to_string())? {
                Key::ArrowUp | Key::Char('k') => selected = selected.saturating_sub(1),
                Key::ArrowDown | Key::Char('j') => selected = (selected + 1).min(2),
                Key::Enter => return Ok(Some(choices[selected])),
                Key::Escape | Key::CtrlC | Key::Char('q') => return Ok(None),
                _ => {}
            }
        }
    })();
    term.show_cursor().ok();
    let Some(priority) = result? else {
        return Ok(());
    };
    write_settings(cwd, resolved_ranking(&config, priority))?;
    term.write_line(&format!(
        "◇  Saved {} priority to {}",
        priority_name(priority),
        config_path(cwd).display()
    ))
    .map_err(|e| e.to_string())
}

fn selector_lines(selected: usize, options: RenderOptions) -> Vec<String> {
    let choices = [
        ("Simple", "favor low complexity and readable structure"),
        ("Composable", "favor clean module boundaries and coupling"),
        ("Secure", "favor safe data flow and dangerous-call review"),
    ];
    let mut lines = vec![
        paint(
            "┌  Topos project settings",
            console::Style::new().bold(),
            options,
        ),
        "│".to_string(),
        format!(
            "│  {}",
            paint(
                "Evaluation priority",
                console::Style::new().cyan().bold(),
                options,
            )
        ),
        format!(
            "│  {}",
            paint(
                "↑↓ move · enter save · esc cancel",
                console::Style::new().dim(),
                options,
            )
        ),
        "│".to_string(),
    ];
    for (idx, (label, hint)) in choices.iter().enumerate() {
        let cursor = if idx == selected { "›" } else { " " };
        let marker = if idx == selected { "●" } else { "○" };
        let row = format!("{cursor} {marker} {label:<12} {hint}");
        let style = if idx == selected {
            console::Style::new().bold()
        } else {
            console::Style::new().dim()
        };
        lines.push(format!("│ {}", paint(row, style, options)));
    }
    lines.push("└".to_string());
    lines
}

/// Persist evaluation settings using the canonical single-key schema:
/// `priority` is the full ranking array. Legacy `preferences` is removed so
/// the file has one source of truth that `load_topos_config` round-trips.
fn write_settings(cwd: &Path, ranking: [Generator; 3]) -> Result<(), String> {
    let path = config_path(cwd);
    let source = fs::read_to_string(&path).unwrap_or_default();
    let mut document = source
        .parse::<DocumentMut>()
        .map_err(|e| format!("parsing {}: {e}", path.display()))?;
    if !document.as_table().contains_key("evaluation") {
        document["evaluation"] = Item::Table(Table::new());
    }
    let mut values = Array::new();
    for generator in ranking {
        values.push(generator.as_str());
    }
    document["evaluation"]["priority"] = value(values);
    if let Some(evaluation) = document["evaluation"].as_table_mut() {
        evaluation.remove("preferences");
    }
    fs::write(&path, document.to_string()).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn config_path(cwd: &Path) -> PathBuf {
    find_config_file(cwd).unwrap_or_else(|| cwd.join(".topos.toml"))
}

pub(crate) fn parse_priority(value: &str) -> Result<Priority, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "simple" => Ok(Priority::Simple),
        "composable" => Ok(Priority::Composable),
        "secure" => Ok(Priority::Secure),
        _ => Err(format!(
            "invalid priority '{value}' (expected simple, composable, or secure)"
        )),
    }
}

pub(crate) fn parse_ranking(value: &str) -> Result<[Generator; 3], String> {
    let parsed: Vec<Generator> = value
        .split(',')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "simple" => Ok(Generator::Simple),
            "composable" => Ok(Generator::Composable),
            "secure" => Ok(Generator::Secure),
            other => Err(format!("invalid preference '{other}'")),
        })
        .collect::<Result<_, _>>()?;
    let ranking: [Generator; 3] = parsed.try_into().map_err(|values: Vec<_>| {
        format!(
            "preferences require all three pillars exactly once (got {})",
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
    Ranking([Generator; 3]),
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
    }
}

pub(crate) fn priority_name(priority: Priority) -> &'static str {
    match priority {
        Priority::Simple => "simple",
        Priority::Composable => "composable",
        Priority::Secure => "secure",
    }
}

fn generator_for_priority(priority: Priority) -> Generator {
    priority.top_generator()
}

fn default_ranking() -> [Generator; 3] {
    [Generator::Simple, Generator::Composable, Generator::Secure]
}

/// Ranking to persist for `priority`, preserving the relative order of the
/// other two pillars from `config` rather than resetting to the default.
fn resolved_ranking(config: &ToposConfig, priority: Priority) -> [Generator; 3] {
    let base = config.preferences.unwrap_or_else(default_ranking);
    move_first(base, generator_for_priority(priority))
}

fn move_first(ranking: [Generator; 3], first: Generator) -> [Generator; 3] {
    let rest: Vec<_> = ranking.into_iter().filter(|g| *g != first).collect();
    [first, rest[0], rest[1]]
}

fn ranking_text(ranking: [Generator; 3]) -> String {
    ranking.map(Generator::as_str).join(" > ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_requires_a_permutation() {
        assert!(parse_ranking("secure,simple,composable").is_ok());
        assert!(parse_ranking("secure,secure,simple").is_err());
        assert!(parse_ranking("secure,simple").is_err());
    }

    #[test]
    fn moving_priority_preserves_the_other_order() {
        assert_eq!(
            move_first(default_ranking(), Generator::Secure),
            [Generator::Secure, Generator::Simple, Generator::Composable]
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

        write_settings(
            &dir,
            [Generator::Secure, Generator::Simple, Generator::Composable],
        )
        .unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# keep this"));
        assert!(updated.contains("[[secure.allow]]"));
        assert!(updated.contains("priority = [\"secure\", \"simple\", \"composable\"]"));
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
        let ranking = [Generator::Composable, Generator::Secure, Generator::Simple];

        write_settings(&dir, ranking).unwrap();

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

        write_settings(
            &dir,
            [Generator::Secure, Generator::Composable, Generator::Simple],
        )
        .unwrap();

        let updated = fs::read_to_string(path).unwrap();
        assert!(updated.contains("priority = [\"secure\", \"composable\", \"simple\"]"));
        assert!(!updated.contains("preferences"));
        let loaded = load_topos_config(&dir);
        assert_eq!(
            loaded.preferences,
            Some([Generator::Secure, Generator::Composable, Generator::Simple])
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resolved_ranking_preserves_existing_order_around_the_new_priority() {
        let config = ToposConfig {
            preferences: Some([Generator::Composable, Generator::Secure, Generator::Simple]),
            ..Default::default()
        };
        assert_eq!(
            resolved_ranking(&config, Priority::Secure),
            [Generator::Secure, Generator::Composable, Generator::Simple]
        );
    }

    #[test]
    fn resolved_ranking_falls_back_to_default_order_without_existing_preferences() {
        let config = ToposConfig::default();
        assert_eq!(
            resolved_ranking(&config, Priority::Secure),
            [Generator::Secure, Generator::Simple, Generator::Composable]
        );
    }

    #[test]
    fn a_single_pillar_parses_as_priority_and_a_list_parses_as_ranking() {
        assert!(matches!(
            parse_priority_input("secure").unwrap(),
            PriorityInput::Single(Priority::Secure)
        ));
        assert!(matches!(
            parse_priority_input("secure,simple,composable").unwrap(),
            PriorityInput::Ranking([Generator::Secure, Generator::Simple, Generator::Composable])
        ));
    }
}
