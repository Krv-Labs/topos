//! Shared markdown renderer for the `topos_refactor` tool family.
//!
//! All three refactor targets (cycles, dependencies, process) report the
//! same "ranked hotspot" row shape, so they share one table renderer.

use crate::schemas::RefactorHotspot;

/// Render a ranked list of [`RefactorHotspot`] rows as markdown.
pub fn render_hotspots_md(title: &str, hotspots: &[RefactorHotspot]) -> String {
    if hotspots.is_empty() {
        return format!("**{title}:** none found.");
    }

    let mut lines = vec![
        format!("## {title}"),
        String::new(),
        "| Kind | Label | Location | Score | Suggestion |".to_string(),
        "| --- | --- | --- | ---: | --- |".to_string(),
    ];
    for h in hotspots {
        let mut location = h.filepath.clone();
        if let Some(start) = h.line_start {
            location.push_str(&format!(":{start}"));
            if let Some(end) = h.line_end {
                if end != start {
                    location.push_str(&format!("-{end}"));
                }
            }
        }
        let safe_label = h.label.replace('\n', " ").replace('|', "\\|");
        let safe_suggestion = h.suggestion.replace('\n', " ").replace('|', "\\|");
        lines.push(format!(
            "| `{}` | `{safe_label}` | `{location}` | {:.3} | {safe_suggestion} |",
            h.kind, h.score
        ));
    }
    lines.join("\n")
}
