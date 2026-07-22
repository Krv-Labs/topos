//! `gitnexus cypher` subprocess adapter — the COMPOSABLE read path for
//! GitNexus ≥ 1.5 LadybugDB binary stores (issue #198).
//!
//! Current GitNexus writes `.gitnexus/lbug` as a packed binary file. There
//! is no Rust LadybugDB client, but the installed CLI can query the store:
//!
//! ```text
//! gitnexus cypher -r <project_root> [--branch <name>] '<query>'
//! ```
//!
//! Output is JSON wrapping a markdown table (`{ "markdown": "|…|", "row_count": N }`)
//! aimed at LLM consumers. This module parses that shape into row maps the
//! MDG loader can consume.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::gitnexus::{gitnexus_available, GITNEXUS_CMD};
use super::process::{command_on_path, run_with_timeout, RunError};

/// Default ceiling for a single `gitnexus cypher` call. Node/edge dumps on
/// large repos can take tens of seconds; keep this below the analyze
/// timeout but high enough that a cold store open doesn't fail the load.
pub const DEFAULT_CYPHER_TIMEOUT_S: f64 = 120.0;

/// Parsed pipe-table result from one `gitnexus cypher` invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CypherTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl CypherTable {
    /// Row cells as `(header, value)` pairs, skipping blank values.
    pub fn row_maps(&self) -> Vec<Vec<(String, String)>> {
        self.rows
            .iter()
            .map(|row| {
                self.headers
                    .iter()
                    .zip(row.iter())
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(h, v)| (h.clone(), v.clone()))
                    .collect()
            })
            .collect()
    }

    /// Column value for `header` on `row`, if present and non-empty.
    pub fn get<'a>(&'a self, row: &'a [String], header: &str) -> Option<&'a str> {
        let idx = self.headers.iter().position(|h| h == header)?;
        let value = row.get(idx)?;
        if value.is_empty() {
            None
        } else {
            Some(value.as_str())
        }
    }
}

/// Why a cypher query failed.
#[derive(Debug)]
pub enum CypherError {
    /// `gitnexus` is not on `$PATH`.
    NotAvailable,
    TimedOut,
    Io(std::io::Error),
    /// Non-zero exit or unparseable stdout.
    Failed(String),
    Json(serde_json::Error),
}

impl std::fmt::Display for CypherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CypherError::NotAvailable => write!(
                f,
                "gitnexus CLI not found — cannot query the LadybugDB binary store"
            ),
            CypherError::TimedOut => write!(f, "gitnexus cypher timed out"),
            CypherError::Io(e) => write!(f, "gitnexus cypher could not be executed: {e}"),
            CypherError::Failed(msg) => write!(f, "{msg}"),
            CypherError::Json(e) => write!(f, "gitnexus cypher returned invalid JSON: {e}"),
        }
    }
}

impl std::error::Error for CypherError {}

/// Split one markdown table row into cells.
fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_start_matches('|').trim_end_matches('|');
    trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn is_separator_row(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

/// Parse a GitHub-flavored markdown pipe table into headers + rows.
pub fn parse_markdown_table(markdown: &str) -> CypherTable {
    let lines: Vec<&str> = markdown
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .collect();
    if lines.is_empty() {
        return CypherTable::default();
    }
    let headers = split_row(lines[0]);
    let data_start = if lines.len() > 1 && is_separator_row(lines[1]) {
        2
    } else {
        1
    };
    let rows = lines[data_start..]
        .iter()
        .map(|line| {
            let mut cells = split_row(line);
            // Pad / truncate so every row matches header width.
            cells.resize(headers.len(), String::new());
            cells
        })
        .collect();
    CypherTable { headers, rows }
}

/// Parse the JSON envelope `gitnexus cypher` prints to stdout.
pub fn parse_cypher_stdout(stdout: &str) -> Result<CypherTable, CypherError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "[]" || trimmed == "null" {
        // GitNexus prints a bare JSON empty array / null when a query
        // matches zero rows — not the markdown envelope used for hits.
        return Ok(CypherTable::default());
    }
    // GitNexus's CLI truncates cypher stdout at 64 KiB; a truncated markdown
    // envelope fails mid-string and must be retried with a smaller page.
    if trimmed.len() >= 65536 {
        return Err(CypherError::Failed(
            "gitnexus cypher output truncated at 64 KiB — reduce page size".to_string(),
        ));
    }
    // Prefer a whole-document JSON object; fall back to the first `{…}` span
    // if the CLI ever prefixes diagnostic text.
    let json_text = if trimmed.starts_with('{') {
        trimmed
    } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        return Err(CypherError::Failed(format!(
            "gitnexus cypher produced no JSON object: {trimmed}"
        )));
    };
    let value: Value = serde_json::from_str(json_text).map_err(CypherError::Json)?;
    let markdown = value.get("markdown").and_then(Value::as_str).unwrap_or("");
    Ok(parse_markdown_table(markdown))
}

fn resolve_cypher_timeout() -> Option<Duration> {
    let secs = std::env::var("TOPOS_CYPHER_TIMEOUT")
        .ok()
        .and_then(|raw| raw.parse::<f64>().ok())
        .unwrap_or(DEFAULT_CYPHER_TIMEOUT_S);
    if secs > 0.0 {
        Some(Duration::from_secs_f64(secs))
    } else {
        None
    }
}

/// Run `gitnexus cypher` against the indexed store for `project_root`.
pub fn run_cypher(
    project_root: &Path,
    branch: Option<&str>,
    query: &str,
) -> Result<CypherTable, CypherError> {
    run_cypher_with_cmd(GITNEXUS_CMD, project_root, branch, query)
}

/// Same as [`run_cypher`], but with an injectable command name for tests.
pub fn run_cypher_with_cmd(
    cmd_name: &str,
    project_root: &Path,
    branch: Option<&str>,
    query: &str,
) -> Result<CypherTable, CypherError> {
    if cmd_name == GITNEXUS_CMD && !gitnexus_available() && !command_on_path(cmd_name) {
        return Err(CypherError::NotAvailable);
    }
    let mut cmd = Command::new(cmd_name);
    cmd.arg("cypher").arg("-r").arg(project_root);
    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    }
    cmd.arg(query);

    match run_with_timeout(cmd, Some(project_root), true, resolve_cypher_timeout()) {
        Err(RunError::TimedOut) => Err(CypherError::TimedOut),
        Err(RunError::Io(e)) => Err(CypherError::Io(e)),
        Ok(output) if output.status_code.unwrap_or(-1) != 0 => {
            let detail = if !output.stderr.trim().is_empty() {
                output.stderr.trim()
            } else {
                output.stdout.trim()
            };
            Err(CypherError::Failed(if detail.is_empty() {
                "gitnexus cypher failed".to_string()
            } else {
                detail.to_string()
            }))
        }
        Ok(output) => parse_cypher_stdout(&output.stdout),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_markdown_table_reads_headers_and_rows() {
        let markdown = "\
| id | name | type |\n\
| --- | --- | --- |\n\
| 0 | File | NODE |\n\
| 236 | CodeRelation | REL |\n";
        let table = parse_markdown_table(markdown);
        assert_eq!(table.headers, vec!["id", "name", "type"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.get(&table.rows[0], "name"), Some("File"));
        assert_eq!(table.get(&table.rows[1], "type"), Some("REL"));
    }

    #[test]
    fn parse_markdown_table_handles_empty_cells() {
        let markdown = "| a | b |\n| --- | --- |\n| x |  |\n";
        let table = parse_markdown_table(markdown);
        assert_eq!(table.get(&table.rows[0], "a"), Some("x"));
        assert_eq!(table.get(&table.rows[0], "b"), None);
    }

    #[test]
    fn parse_cypher_stdout_extracts_markdown_envelope() {
        let stdout = r#"{
  "markdown": "| id | filePath |\n| --- | --- |\n| File:a.rs | a.rs |",
  "row_count": 1
}"#;
        let table = parse_cypher_stdout(stdout).unwrap();
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.get(&table.rows[0], "id"), Some("File:a.rs"));
        assert_eq!(table.get(&table.rows[0], "filePath"), Some("a.rs"));
    }

    #[test]
    fn parse_cypher_stdout_rejects_non_json() {
        let err = parse_cypher_stdout("not json at all").unwrap_err();
        assert!(matches!(err, CypherError::Failed(_)));
    }

    #[test]
    fn parse_cypher_stdout_treats_empty_array_as_empty_table() {
        let table = parse_cypher_stdout("[]").unwrap();
        assert!(table.headers.is_empty());
        assert!(table.rows.is_empty());
    }
}
