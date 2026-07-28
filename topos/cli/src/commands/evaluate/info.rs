//! Explicit drill-down for the weakest files from `topos evaluate --info`.

use std::path::{Path, PathBuf};

use console::{Key, Term};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::evaluation::preferences::Generator;
use topos_engine::evaluation::suggestions::suggest_refactors;
use topos_mcp::diagnostics::overlay_for_file;
use topos_mcp::metric_locations::build_metric_locations;
use topos_mcp::refactor_targets::build_refactor_targets;
use topos_mcp::schemas::GeneratorInput;

use super::info_render::{detail_lines, FileDetails};
use super::summary::{attention_lines, failure_file_indices, failure_lines, ranked_file_indices};

enum BrowserAction {
    Stay,
    Back,
    Open,
    Close,
}

#[derive(Clone, Copy)]
enum SelectorKind<'a> {
    WeakSpots,
    Failures(&'a str),
}

pub(crate) fn can_browse_evaluation_info(file_count: usize) -> bool {
    file_count > 1 && Term::stderr().is_term() && Term::stdout().is_term()
}

pub(crate) fn show_evaluation_info(
    files: &[PathBuf],
    results: &[ClassificationResult],
    language: &str,
    ranking: Option<&[Generator; 3]>,
) -> Result<(), String> {
    let ranked = ranked_file_indices(files, results, 5);
    show_ranked_info(
        files,
        results,
        &ranked,
        language,
        ranking,
        SelectorKind::WeakSpots,
    )
}

pub(crate) fn show_pillar_failures(
    files: &[PathBuf],
    results: &[ClassificationResult],
    language: &str,
    pillar: &str,
    inspect: bool,
    ranking: Option<&[Generator; 3]>,
) -> Result<(), String> {
    let mut ranked = failure_file_indices(files, results, pillar);
    if !inspect || ranked.is_empty() {
        for line in failure_lines(
            files,
            results,
            &ranked,
            pillar,
            None,
            std::env::var_os("NO_COLOR").is_none(),
            terminal_width(&Term::stdout(), 120),
        ) {
            println!("{line}");
        }
        return Ok(());
    }
    ranked.truncate(5);
    show_ranked_info(
        files,
        results,
        &ranked,
        language,
        ranking,
        SelectorKind::Failures(pillar),
    )
}

fn show_ranked_info(
    files: &[PathBuf],
    results: &[ClassificationResult],
    ranked: &[usize],
    language: &str,
    ranking: Option<&[Generator; 3]>,
    kind: SelectorKind<'_>,
) -> Result<(), String> {
    if can_browse_evaluation_info(ranked.len()) {
        browse_files(files, results, ranked, language, ranking, kind)?;
        for line in selector_lines_for(
            files,
            results,
            ranked,
            None,
            terminal_width(&Term::stdout(), 120),
            kind,
        ) {
            println!("{line}");
        }
        return Ok(());
    }
    if ranked.len() > 1 {
        for line in selector_lines_for(
            files,
            results,
            ranked,
            None,
            terminal_width(&Term::stdout(), 120),
            kind,
        ) {
            println!("{line}");
        }
    }
    let Some(selected) = ranked.first().copied() else {
        return Ok(());
    };
    let details = details_for_file(&files[selected], &results[selected], language, ranking)?;
    println!();
    for line in detail_lines(
        &files[selected],
        &results[selected],
        &details,
        100,
        Term::stdout().is_term() && std::env::var_os("NO_COLOR").is_none(),
        false,
        1,
        ranked.len(),
    ) {
        println!("{line}");
    }
    Ok(())
}

fn browse_files(
    files: &[PathBuf],
    results: &[ClassificationResult],
    ranked: &[usize],
    language: &str,
    ranking: Option<&[Generator; 3]>,
    kind: SelectorKind<'_>,
) -> Result<(), String> {
    let term = Term::stderr();
    let width = terminal_width(&term, 100);
    let mut selected = 0;
    let mut rendered = 0;
    let mut detail: Option<FileDetails> = None;
    term.hide_cursor().map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        loop {
            if rendered > 0 {
                term.clear_last_lines(rendered).map_err(|e| e.to_string())?;
            }
            let lines = match &detail {
                Some(details) => detail_lines(
                    &files[ranked[selected]],
                    &results[ranked[selected]],
                    details,
                    width,
                    std::env::var_os("NO_COLOR").is_none(),
                    true,
                    selected + 1,
                    ranked.len(),
                ),
                None => selector_lines_for(files, results, ranked, Some(selected), width, kind),
            };
            rendered = lines.len();
            for line in lines {
                term.write_line(&line).map_err(|e| e.to_string())?;
            }
            let key = term.read_key().map_err(|e| e.to_string())?;
            match browser_action(key, detail.is_some(), &mut selected, ranked.len()) {
                BrowserAction::Stay => {}
                BrowserAction::Back => detail = None,
                BrowserAction::Open => {
                    detail = Some(details_for_file(
                        &files[ranked[selected]],
                        &results[ranked[selected]],
                        language,
                        ranking,
                    )?);
                }
                BrowserAction::Close => return Ok(()),
            }
        }
    })();
    if rendered > 0 {
        term.clear_last_lines(rendered).ok();
    }
    term.show_cursor().ok();
    result
}

fn browser_action(
    key: Key,
    showing_detail: bool,
    selected: &mut usize,
    item_count: usize,
) -> BrowserAction {
    if showing_detail {
        return detail_action(key);
    }
    list_action(key, selected, item_count)
}

fn detail_action(key: Key) -> BrowserAction {
    match key {
        Key::Escape | Key::ArrowLeft | Key::Char('h') => BrowserAction::Back,
        Key::CtrlC | Key::Char('q') => BrowserAction::Close,
        _ => BrowserAction::Stay,
    }
}

fn list_action(key: Key, selected: &mut usize, item_count: usize) -> BrowserAction {
    match key {
        Key::ArrowUp | Key::Char('k') => *selected = (*selected).saturating_sub(1),
        Key::ArrowDown | Key::Char('j') => *selected = (*selected + 1).min(item_count - 1),
        Key::Enter => return BrowserAction::Open,
        Key::Escape | Key::CtrlC | Key::Char('q') => return BrowserAction::Close,
        _ => {}
    }
    BrowserAction::Stay
}

fn selector_lines_for(
    files: &[PathBuf],
    results: &[ClassificationResult],
    ranked: &[usize],
    selected: Option<usize>,
    width: usize,
    kind: SelectorKind<'_>,
) -> Vec<String> {
    match kind {
        SelectorKind::WeakSpots => attention_lines(
            files,
            results,
            selected,
            std::env::var_os("NO_COLOR").is_none(),
            width,
        ),
        SelectorKind::Failures(pillar) => failure_lines(
            files,
            results,
            ranked,
            pillar,
            selected,
            std::env::var_os("NO_COLOR").is_none(),
            width,
        ),
    }
}

fn details_for_file(
    path: &Path,
    result: &ClassificationResult,
    language: &str,
    ranking: Option<&[Generator; 3]>,
) -> Result<FileDetails, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("reading {} for details: {e}", path.display()))?;
    Ok(details_for_source(path, result, &source, language, ranking))
}

pub(crate) fn details_for_source(
    path: &Path,
    result: &ClassificationResult,
    source: &str,
    language: &str,
    ranking: Option<&[Generator; 3]>,
) -> FileDetails {
    let locations = build_metric_locations(source, language, result);
    let overlay = overlay_for_file(path, result, &[]);
    let findings = overlay
        .as_ref()
        .map(|value| value.active_findings.as_slice())
        .unwrap_or(&[]);
    let core_findings: Vec<_> = findings.iter().map(|finding| finding.to_core()).collect();
    let mapped_ranking = ranking.map(|values| values.map(generator_input));
    FileDetails {
        targets: build_refactor_targets(
            &path.to_string_lossy(),
            result,
            findings,
            &locations,
            mapped_ranking.as_ref().map(|values| values.as_slice()),
            3,
        ),
        suggestions: suggest_refactors(result, &core_findings),
    }
}

fn generator_input(generator: Generator) -> GeneratorInput {
    match generator {
        Generator::Simple => GeneratorInput::Simple,
        Generator::Composable => GeneratorInput::Composable,
        Generator::Secure => GeneratorInput::Secure,
    }
}

fn terminal_width(term: &Term, fallback: usize) -> usize {
    let width = usize::from(term.size().1);
    if width == 0 {
        fallback
    } else {
        width
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use topos_engine::core::omega::EvaluationValue;

    use super::*;

    fn result(simple: f64, secure: f64) -> ClassificationResult {
        ClassificationResult {
            is_parseable: true,
            scores: HashMap::from([
                ("simple".to_string(), simple),
                ("secure".to_string(), secure),
            ]),
            lattice_element: EvaluationValue::Slop,
            ..Default::default()
        }
    }

    #[test]
    fn selector_shows_rank_score_and_relative_path() {
        let files = vec![
            PathBuf::from("repo/src/a.rs"),
            PathBuf::from("repo/src/b.rs"),
        ];
        let mut passing = result(0.5, 1.0);
        passing
            .dimensions
            .insert("simple".to_string(), EvaluationValue::Simple);
        let mut failing = result(0.2, 1.0);
        failing
            .dimensions
            .insert("simple".to_string(), EvaluationValue::Slop);
        let results = vec![passing, failing];
        let ranked = ranked_file_indices(&files, &results, 5);
        let lines = selector_lines_for(
            &files,
            &results,
            &ranked,
            Some(0),
            100,
            SelectorKind::WeakSpots,
        );
        let output = lines.join("\n");
        assert!(output.contains("↑↓ move · enter open · esc close"));
        assert!(output.contains("›  1  b.rs"));
        assert!(output.contains("60% avg"));
        assert!(output.contains("›  1"));
    }

    #[test]
    fn selector_rows_fit_a_narrow_terminal() {
        let files = vec![PathBuf::from(
            "repo/a/very/long/source/path/that/must/be/truncated.rs",
        )];
        let results = vec![result(0.2, 1.0)];
        let ranked = ranked_file_indices(&files, &results, 5);
        let lines = selector_lines_for(
            &files,
            &results,
            &ranked,
            Some(0),
            72,
            SelectorKind::WeakSpots,
        );
        assert!(lines.iter().all(|line| line.chars().count() <= 72));
    }

    #[test]
    fn failure_selector_names_the_pillar_and_uses_its_score() {
        let files = vec![
            PathBuf::from("repo/src/a.rs"),
            PathBuf::from("repo/src/b.rs"),
        ];
        let mut passing = result(0.5, 1.0);
        passing
            .dimensions
            .insert("simple".to_string(), EvaluationValue::Simple);
        let mut failing = result(0.2, 1.0);
        failing
            .dimensions
            .insert("simple".to_string(), EvaluationValue::Slop);
        let results = vec![passing, failing];
        let lines = selector_lines_for(
            &files,
            &results,
            &[1],
            Some(0),
            100,
            SelectorKind::Failures("simple"),
        );
        let output = lines.join("\n");
        assert!(output.contains("SIMPLE failures · 1 file"));
        assert!(output.contains("›  1  b.rs"));
        assert!(output.contains("20% score"));
        assert!(!output.contains("avg"));
    }
}
