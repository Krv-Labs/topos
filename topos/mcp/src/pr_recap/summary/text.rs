//! The summary as compact markdown, for clients that show only the text
//! block of a tool result.

use crate::shaping::{clip, omitted_line, sig3};

use super::super::model::{FileRecap, Medal};
use super::paths::NotFound;
use super::types::{AttentionFile, Summary, SummaryFinding};

/// Finding bullets in the text; the rest are in `structuredContent`.
const TEXT_FINDINGS: usize = 8;
/// Hotspots printed for a requested file record; the rest are counted.
const TEXT_HOTSPOTS: usize = 2;
/// Characters of `reason` in the text.
const TEXT_REASON_CHARS: usize = 400;

impl Summary {
    pub fn to_markdown(&self) -> String {
        let mut out = self.header();
        out.push_str(&self.direction_line());
        out.push_str(&self.incomplete_line());
        out.push_str(&self.findings_block());
        out.push_str(&attention_block(&self.attention));
        for file in &self.files {
            out.push_str(&format!("File {}\n", file_line(file)));
        }
        out.push_str(&not_found_block(&self.paths_not_found));
        let counts = self.omitted.counts();
        if let Some(line) = omitted_line(&counts, self.more.as_deref()) {
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    fn header(&self) -> String {
        let scoring = self
            .scope
            .scoring_ms
            .map_or(String::new(), |ms| format!(" ({ms} ms scoring)"));
        format!(
            "**PR Recap ({})**: `{}` — {}\n`{}...{}` · {} file(s) scored in {} ms{scoring} · gate {}\n",
            self.check,
            self.readiness.word(),
            clip(&self.reason, TEXT_REASON_CHARS),
            short(&self.base),
            short(&self.head),
            self.scope.counts.files_scored,
            self.scope.elapsed_ms,
            self.gate.preset,
        )
    }

    fn direction_line(&self) -> String {
        let Some(direction) = self.worst_file_direction else {
            return String::new();
        };
        let name = serde_json::to_value(direction)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let file = self
            .worst_file
            .as_deref()
            .map_or(String::new(), |path| format!(" in `{path}`"));
        format!(
            "Worst file direction: {name}{file} (the most severe single file's structural \
             change, independent of readiness)\n"
        )
    }

    fn incomplete_line(&self) -> String {
        if !self.incomplete {
            return String::new();
        }
        format!(
            "Incomplete: {} file(s) over the max_files cap went unscored.\n",
            self.scope.counts.files_capped
        )
    }

    fn findings_block(&self) -> String {
        if self.findings_total.is_empty() {
            return String::new();
        }
        let totals: Vec<String> = self
            .findings_total
            .iter()
            .map(|(severity, count)| format!("{severity} {count}"))
            .collect();
        let mut out = format!("Findings ({}):\n", totals.join(", "));
        for finding in self.findings.iter().take(TEXT_FINDINGS) {
            out.push_str(&format!("- {}\n", bullet(finding)));
        }
        if self.findings.len() > TEXT_FINDINGS {
            let rest = self.findings.len() - TEXT_FINDINGS;
            out.push_str(&format!("- {rest} more in structuredContent\n"));
        }
        out
    }
}

/// `Not found` lines for requested paths that matched no scored file.
pub fn not_found_block(missing: &[NotFound]) -> String {
    missing
        .iter()
        .map(|item| format!("Not found `{}`: {}\n", item.path, item.reason))
        .collect()
}

fn attention_block(attention: &[AttentionFile]) -> String {
    if attention.is_empty() {
        return String::new();
    }
    let mut out = "Attention:\n".to_string();
    for file in attention {
        out.push_str(&format!("{}\n", attention_line(file)));
    }
    out
}

fn attention_line(file: &AttentionFile) -> String {
    let mut line = format!("- `{}` ({})", file.path, file.why);
    if file.findings > 1 {
        line.push_str(&format!(" ×{}", file.findings));
    }
    if let Some(spot) = &file.hotspot {
        let function = spot.function.as_deref().unwrap_or("-");
        line.push_str(&format!(" · {function}:{} {}", spot.line, spot.detail));
    }
    line
}

fn bullet(finding: &SummaryFinding) -> String {
    let location = match finding.line {
        Some(line) => format!("`{}:{line}`", finding.path),
        None => format!("`{}`", finding.path),
    };
    let function = finding
        .function
        .as_deref()
        .map_or(String::new(), |name| format!(" {name}"));
    let Some(metric) = &finding.metric else {
        let text = clip(&finding.text, 160);
        return format!("{} {location}{function} {text}", finding.severity);
    };
    format!(
        "{} {location}{function} {metric}{}{}",
        finding.severity,
        values(finding),
        limit(finding)
    )
}

fn values(finding: &SummaryFinding) -> String {
    match (finding.before, finding.after) {
        (Some(before), Some(after)) => format!(" {before}→{after}"),
        (None, Some(after)) => format!(" {after}"),
        _ => String::new(),
    }
}

fn limit(finding: &SummaryFinding) -> String {
    match (finding.bound, finding.limit) {
        (Some(bound), Some(limit)) => format!(" ({bound} {limit})"),
        _ => String::new(),
    }
}

/// One requested file record in a line: pillars, medals, worst function,
/// hotspots.
pub fn file_line(file: &FileRecap) -> String {
    let mut parts = vec![format!("`{}`", file.path)];
    let pillars = pillar_scores(file);
    if !pillars.is_empty() {
        parts.push(pillars.join(", "));
    }
    parts.push(format!(
        "medal {}→{}",
        medal(&file.medal_before),
        medal(&file.medal_after)
    ));
    if let Some(worst) = &file.worst_function_after {
        parts.push(format!(
            "worst {}:{} complexity {}",
            worst.name, worst.line, worst.complexity
        ));
    }
    parts.extend(hotspot_parts(file));
    parts.join(" · ")
}

fn pillar_scores(file: &FileRecap) -> Vec<String> {
    file.pillars
        .iter()
        .filter_map(
            |(name, delta)| match (delta.before_score, delta.after_score) {
                (Some(before), Some(after)) => {
                    Some(format!("{name} {}→{}", sig3(before), sig3(after)))
                }
                (None, Some(after)) => Some(format!("{name} {}", sig3(after))),
                _ => None,
            },
        )
        .collect()
}

fn medal(medal: &Option<Medal>) -> String {
    medal
        .as_ref()
        .map_or_else(|| "-".to_string(), |m| format!("{} {}", m.symbol, m.tier))
}

/// The first hotspots of the record, and a count of the rest.
fn hotspot_parts(file: &FileRecap) -> Vec<String> {
    let mut parts: Vec<String> = file
        .hotspots
        .iter()
        .take(TEXT_HOTSPOTS)
        .map(|spot| format!("hotspot {}:{} {}", spot.metric, spot.line, spot.detail))
        .collect();
    if file.hotspots.len() > TEXT_HOTSPOTS {
        parts.push(format!(
            "+{} more hotspots",
            file.hotspots.len() - TEXT_HOTSPOTS
        ));
    }
    parts
}

fn short(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::super::build;
    use super::super::tests::{abs, fixture, info_on, summarize, TIMING};
    use crate::pr_recap::tests::recap;

    #[test]
    fn text_covers_info_only_incomplete_direction_and_paths() {
        let (_keep, repo) = fixture(3, 1);
        let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let first = recap.files[0].path.clone();
        info_on(&mut recap, &first);
        recap.reason_path = Some(first);
        let text = summarize(&recap, &repo).to_markdown();
        assert!(text.contains("Findings (info 1):"), "{text}");
        assert!(text.contains("Worst file direction:"), "{text}");
        assert!(text.contains("independent of readiness"), "{text}");
        assert!(text.contains("ms scoring"), "{text}");
        assert!(!text.contains("Incomplete:"));

        recap.incomplete = true;
        recap.scope.files_capped = 4;
        recap.reason = "x".repeat(900);
        let text = summarize(&recap, &repo).to_markdown();
        assert!(
            text.contains("Incomplete: 4 file(s) over the max_files cap"),
            "{text}"
        );
        assert!(text.contains('…'));

        let asked = ["src/big1.py".to_string(), "nope.py".to_string()];
        let text = build(&recap, &repo, &asked, TIMING).to_markdown();
        assert!(
            text.contains(&format!("File `{}`", abs(&repo, "src/big1.py"))),
            "{text}"
        );
        assert!(text.contains("medal"), "{text}");
        assert!(
            text.contains("hotspot ast.max_function_complexity"),
            "{text}"
        );
        assert!(
            text.contains("Not found `nope.py`: not changed in this range"),
            "{text}"
        );
    }

    #[test]
    fn a_requested_record_counts_the_hotspots_it_does_not_print() {
        let (_keep, repo) = fixture(2, 0);
        let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let spot = recap.files[0].hotspots[0].clone();
        recap.files[0].hotspots = vec![spot; 5];
        let path = recap.files[0].path.clone();
        let text = build(&recap, &repo, &[path], TIMING).to_markdown();
        assert_eq!(
            text.matches("hotspot ast.max_function_complexity").count(),
            2
        );
        assert!(text.contains("+3 more hotspots"), "{text}");
    }
}
