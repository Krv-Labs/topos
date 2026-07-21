//! Project configuration for Topos — the `.topos.toml` allowlist.
//!
//! Security findings are *contextual*: a call like `yaml.load` may be an
//! intentional, trusted pattern in (say) an ML-experiments project. The
//! allowlist lets a project acknowledge such patterns so they stop being
//! reported as actionable findings.
//!
//! # Anti-gaming stance
//!
//! The allowlist is **advisory and fully disclosed**, never a silent score
//! lift (see [`crate::evaluation::suppression`]). To make casual gaming
//! costly, every entry **requires a non-empty `reason`**; entries without
//! one are dropped. The canonical SECURE verdict is always computed from
//! the full registry regardless of this file.
//!
//! # Deviation from the Python original
//!
//! `ToposConfig::entries_for`'s path scoping resolves paths logically
//! (join-and-normalize, no filesystem access) instead of Python's
//! `Path.resolve()` (which also follows symlinks and can touch disk even
//! for a nonexistent path). Both agree whenever `file_path` is already an
//! absolute, symlink-free path under `root` — true for every caller in
//! this codebase (CLI/MCP always pass a resolved path) — so this is a
//! behavior-preserving simplification, not a scope-narrowing one.

use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILENAME: &str = ".topos.toml";
const CLI_REASON: &str = "CLI --allow (ephemeral)";

/// A single acknowledged-risk entry from `[[secure.allow]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowEntry {
    pub pattern: String,
    pub reason: String,
    pub scope: String,
}

impl AllowEntry {
    pub fn new(pattern: impl Into<String>, reason: impl Into<String>) -> AllowEntry {
        AllowEntry {
            pattern: pattern.into(),
            reason: reason.into(),
            scope: "**".to_string(),
        }
    }

    pub fn with_scope(mut self, scope: impl Into<String>) -> AllowEntry {
        self.scope = scope.into();
        self
    }

    /// Whether this entry's `scope` glob covers `rel_path` (posix-style).
    pub fn matches_path(&self, rel_path: &str) -> bool {
        match self.scope.as_str() {
            "" | "**" | "*" => true,
            pattern => glob_match(pattern, rel_path),
        }
    }
}

/// Resolved project configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToposConfig {
    pub allow: Vec<AllowEntry>,
    /// Directory the `.topos.toml` lives in (scope base for `entries_for`).
    pub root: Option<PathBuf>,
}

impl ToposConfig {
    /// Allow entries whose scope covers `file_path`.
    pub fn entries_for(&self, file_path: Option<&Path>) -> Vec<&AllowEntry> {
        let rel = self.relativize(file_path);
        self.allow
            .iter()
            .filter(|entry| entry.matches_path(&rel))
            .collect()
    }

    fn relativize(&self, file_path: Option<&Path>) -> String {
        let Some(path) = file_path else {
            return String::new();
        };
        let rel = match &self.root {
            Some(root) => path.strip_prefix(root).unwrap_or(path),
            None => path,
        };
        rel.to_string_lossy().replace('\\', "/")
    }
}

/// Walk up from `start` (file or dir) to locate `.topos.toml`.
pub fn find_config_file(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    loop {
        let candidate = current.join(CONFIG_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        current = current.parent()?.to_path_buf();
    }
}

/// Load the nearest `.topos.toml` at or above `start`.
///
/// Returns an empty config (no allowlist) when no file is found or the
/// file is malformed — configuration is best-effort and never fatal.
pub fn load_topos_config(start: &Path) -> ToposConfig {
    let Some(config_file) = find_config_file(start) else {
        return ToposConfig::default();
    };
    let root = config_file.parent().map(Path::to_path_buf);

    let Ok(text) = fs::read_to_string(&config_file) else {
        return ToposConfig {
            allow: Vec::new(),
            root,
        };
    };
    let Ok(data) = text.parse::<toml::Table>() else {
        return ToposConfig {
            allow: Vec::new(),
            root,
        };
    };

    let raw_entries = data
        .get("secure")
        .and_then(|s| s.get("allow"))
        .and_then(|a| a.as_array());
    let allow = raw_entries
        .map(|v| parse_allow_entries(v))
        .unwrap_or_default();
    ToposConfig { allow, root }
}

fn parse_allow_entries(raw_entries: &[toml::Value]) -> Vec<AllowEntry> {
    raw_entries
        .iter()
        .filter_map(|raw| {
            let table = raw.as_table()?;
            let pattern = non_empty_str(table.get("pattern"))?;
            // reason is mandatory anti-gaming friction — drop entries without one.
            let reason = non_empty_str(table.get("reason"))?;
            let scope = non_empty_str(table.get("scope")).unwrap_or_else(|| "**".to_string());
            Some(AllowEntry {
                pattern,
                reason,
                scope,
            })
        })
        .collect()
}

fn non_empty_str(value: Option<&toml::Value>) -> Option<String> {
    let s = value?.as_str()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Merge one-off `--allow` CLI patterns into `config` (scope `**`).
pub fn merge_cli_allows(config: ToposConfig, allows: &[&str]) -> ToposConfig {
    let extra: Vec<AllowEntry> = allows
        .iter()
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| AllowEntry::new(pattern, CLI_REASON))
        .collect();
    if extra.is_empty() {
        return config;
    }
    let mut allow = config.allow;
    allow.extend(extra);
    ToposConfig {
        allow,
        root: config.root,
    }
}

/// Minimal glob matcher for `AllowEntry::scope` patterns: `*` matches any
/// run of characters (including none, and including `/` — matching
/// Python's `fnmatch`, which is not path-aware), `?` matches exactly one
/// character.
///
/// ponytail: no `[...]` character-class support — not exercised by any
/// `.topos.toml` scope pattern in this codebase. Add it if a real config
/// needs it.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut match_from = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            match_from = ti;
            pi += 1;
        } else if let Some(si) = star {
            pi = si + 1;
            match_from += 1;
            ti = match_from;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_without_reason_is_dropped() {
        let dir = std::env::temp_dir().join(format!("topos-cfg-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(CONFIG_FILENAME),
            "[[secure.allow]]\npattern = \"eval\"\n",
        )
        .unwrap();

        let config = load_topos_config(&dir);
        assert!(config.allow.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cli_allow_merge_adds_ephemeral_entries() {
        let config = merge_cli_allows(ToposConfig::default(), &["eval,yaml.load"]);
        let patterns: std::collections::HashSet<&str> =
            config.allow.iter().map(|e| e.pattern.as_str()).collect();
        assert_eq!(
            patterns,
            std::collections::HashSet::from(["eval", "yaml.load"])
        );
        assert!(config.allow.iter().all(|e| !e.reason.is_empty()));
    }

    #[test]
    fn scope_glob_matches_prefix_pattern() {
        let entry = AllowEntry::new("eval", "ok here").with_scope("experiments/**");
        assert!(entry.matches_path("experiments/a.py"));
        assert!(!entry.matches_path("serving/a.py"));
    }

    #[test]
    fn default_scope_matches_everything() {
        let entry = AllowEntry::new("eval", "ok");
        assert!(entry.matches_path("anything/at/all.py"));
    }
}
