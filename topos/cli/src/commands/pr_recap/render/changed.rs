//! `--verbose`'s **Changed files** table: one row per file whose medal,
//! pillars or scores moved, marked by its worst finding.
//!
//! The CHANGE column names pillars with `evaluate`'s marks, `X SIMPLE
//! lost · ✓ NAVIGABLE gained · ↓ SIMPLE 69 → 64`, so a trade that kept
//! its medal still says what it lost.

use std::cmp::Reverse;

use console::Style;
use topos_engine::config::Severity;

use super::findings::severity_mark;
use super::layout::{budget, dim_line, header_dim_line, pad, rail};
use super::medal_cell;
use crate::commands::pr_recap::model::{FileRecap, PrRecap};
use crate::commands::pr_recap::view::{common_parent, relative, visible_shift, PILLARS};
use crate::commands::render::{guide, paint, RenderOptions};

/// Blank columns between the MEDAL cell and CHANGE.
const MEDAL_GAP: usize = 4;

/// Every file worth a row, worst finding first, new files after the
/// changed ones. Split children are the splits table's.
///
/// Within a severity, rows follow the finding list: a file ranks where
/// its most important finding does, so the largest drop leads. Path
/// breaks the remaining ties.
pub(in crate::commands::pr_recap) fn changed_rows(recap: &PrRecap) -> Vec<&FileRecap> {
    let rank = |file: &FileRecap| {
        recap
            .findings
            .iter()
            .position(|finding| finding.path == file.path)
            .unwrap_or(usize::MAX)
    };
    let mut rows: Vec<&FileRecap> = recap
        .files
        .iter()
        .filter(|file| !file.is_split_child() && moved(file))
        .collect();
    rows.sort_by_cached_key(|file| {
        (
            Reverse(file.severity),
            file.is_new(),
            rank(file),
            file.path.clone(),
        )
    });
    rows
}

/// How many scored files, split children aside, have no row.
pub(in crate::commands::pr_recap) fn kept_count(recap: &PrRecap, rows: &[&FileRecap]) -> usize {
    recap
        .files
        .iter()
        .filter(|file| !file.is_split_child())
        .count()
        - rows.len()
}

fn moved(file: &FileRecap) -> bool {
    let medal_moved = match (&file.medal_before, &file.medal_after) {
        (Some(before), Some(after)) => before.tier != after.tier,
        _ => false,
    };
    file.is_new()
        || medal_moved
        || file.cosmetic
        || file.severity >= Some(Severity::Warn)
        || file.pillars.values().any(|delta| {
            delta.lost() || delta.cleared() || delta.shift().is_some_and(visible_shift)
        })
}

/// The heading, the table, and how many files kept their medal; nothing
/// at all when no file was scored.
pub(super) fn changed_lines(recap: &PrRecap, options: RenderOptions) -> Vec<String> {
    let rows = changed_rows(recap);
    let kept = kept_count(recap, &rows);
    if rows.is_empty() && kept == 0 {
        return Vec::new();
    }
    let parent = common_parent(rows.iter().map(|file| file.path.as_str()));
    let mut heading = format!(
        "{}  {}",
        guide('│', options),
        paint("CHANGED FILES", Style::new().cyan().bold(), options)
    );
    if let Some(parent) = &parent {
        heading.push_str(&format!("  {}", paint(parent, Style::new().dim(), options)));
    }
    let mut lines = vec![heading];

    if !rows.is_empty() {
        let file_width = rows
            .iter()
            .map(|file| relative(&file.path, parent.as_deref()).chars().count())
            .chain(["FILE".len()])
            .max()
            .unwrap_or(0)
            + 1;
        let medal_width = rows
            .iter()
            .map(|file| medal_cell(file).chars().count())
            .max()
            .unwrap_or(0)
            + MEDAL_GAP;
        let change_at = 2 + file_width + medal_width;
        lines.push(header_dim_line(
            &format!(
                "{}{}CHANGE",
                pad("FILE", 2 + file_width),
                pad("MEDAL", medal_width)
            ),
            options,
        ));
        for file in &rows {
            let lead = format!(
                "{} {}",
                severity_mark(file.severity.unwrap_or(Severity::Off)),
                pad(relative(&file.path, parent.as_deref()), file_width)
            );
            let medal = medal_cell(file);
            let gap = " ".repeat(medal_width.saturating_sub(medal.chars().count()));
            let available = budget(options).saturating_sub(change_at).max(12);
            for (index, chunk) in change_lines(&change_text(file), available)
                .into_iter()
                .enumerate()
            {
                let text = if index == 0 {
                    format!("{lead}{}{gap}{chunk}", dim_before(&medal, options))
                } else {
                    format!("{}{chunk}", " ".repeat(change_at))
                };
                lines.push(rail(&text, options));
            }
        }
    }
    if kept > 0 {
        lines.push(dim_line(
            &if kept == 1 {
                "1 file kept its medal".to_string()
            } else {
                format!("{kept} files kept their medal")
            },
            options,
        ));
    }
    lines
}

/// `GOLD → BRONZE` with the before tier dim: the eye lands on where the
/// file is now. Neither tier is colored.
fn dim_before(medal: &str, options: RenderOptions) -> String {
    match medal.split_once(" → ") {
        Some((before, after)) => {
            format!("{} → {after}", paint(before, Style::new().dim(), options))
        }
        None => medal.to_string(),
    }
}

/// The CHANGE segments: pillars lost, then gained, then the scores that
/// moved at least a point without crossing a gate.
pub(in crate::commands::pr_recap) fn change_text(file: &FileRecap) -> Vec<String> {
    let mut parts = Vec::new();
    if file.is_new() {
        parts.push("new".to_string());
        if file.severity >= Some(Severity::Warn) {
            for key in PILLARS {
                if file
                    .pillars
                    .get(key)
                    .is_some_and(|delta| delta.after_passed == Some(false))
                {
                    parts.push(format!("X {} fails", key.to_ascii_uppercase()));
                }
            }
        }
    } else {
        let deltas = || {
            PILLARS
                .iter()
                .filter_map(|key| Some((key.to_ascii_uppercase(), file.pillars.get(*key)?)))
        };
        parts.extend(
            deltas()
                .filter(|(_, delta)| delta.lost())
                .map(|(name, _)| format!("X {name} lost")),
        );
        parts.extend(
            deltas()
                .filter(|(_, delta)| delta.cleared())
                .map(|(name, _)| format!("✓ {name} gained")),
        );
        for (name, delta) in deltas() {
            let (Some(before), Some(after), Some(shift)) =
                (delta.before_score, delta.after_score, delta.shift())
            else {
                continue;
            };
            if delta.lost() || delta.cleared() || !visible_shift(shift) {
                continue;
            }
            let arrow = if shift < 0.0 { '↓' } else { '↑' };
            parts.push(format!("{arrow} {name} {before:.0} → {after:.0}"));
        }
    }
    if file.cosmetic {
        parts.push("cosmetic".to_string());
    }
    if parts.is_empty() {
        parts.push("held".to_string());
    }
    parts
}

/// The segments packed onto lines of `width`, split only between
/// segments, so `X SIMPLE lost` never breaks in two.
fn change_lines(parts: &[String], width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for part in parts {
        match lines.last_mut() {
            Some(last) if last.chars().count() + 3 + part.chars().count() <= width => {
                last.push_str(" · ");
                last.push_str(part);
            }
            _ => lines.push(part.clone()),
        }
    }
    lines
}
