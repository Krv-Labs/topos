//! pi's second artifact: a reference to the topos skill directory in
//! `~/.pi/agent/settings.json`.
//!
//! Every other harness needs exactly one artifact because every other harness
//! speaks MCP. pi does not — its README says "No MCP. […] build an extension
//! that adds MCP support" — so the `mcp.json` entry
//! ([`super::paths::pi_config`]) is inert until the user installs
//! `pi-mcp-adapter`. The route that works today is pi's own skills mechanism,
//! and `settings.json`'s `skills` key is a documented array of paths for
//! exactly this ("skills": ["~/.claude/skills", "~/.codex/skills"]).
//!
//! **This module never writes skill content.** It appends one directory path to
//! an array. Skills themselves belong to ClawHub / Hermes / openclaw — see
//! [`super::residue`], which reports a topos `SKILL.md` as someone else's
//! artifact — so if no skill is installed there is nothing to point at and
//! this artifact does not apply at all.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::artifact::{Inspection, State};
use super::fsops::{read_json_object, write_json_object, WriteOutcome};

/// The settings key holding extra skill search paths.
const SKILLS_KEY: &str = "skills";

/// Skill directories other installers use, in the order they are preferred.
/// `~/.agents/skills` first: it is openclaw's shared namespace, and the
/// per-harness directories are symlink farms into it.
const CANDIDATE_DIRS: [&str; 2] = [".agents/skills", ".claude/skills"];

pub(crate) const ACTIVE_MSG: &str = "topos skill directory referenced in ~/.pi/agent/settings.json";
pub(crate) const ABSENT_MSG: &str =
    "topos skill directory not referenced in ~/.pi/agent/settings.json";

/// Reported when pi finds the skill on its own — a good outcome, not a gap.
pub(crate) const DISCOVERED_MSG: &str =
    "topos skill already discovered in ~/.pi/agent/skills — no reference needed";

/// Advice when the skill is not installed anywhere this can point at.
pub(crate) const NO_SKILL_MSG: &str =
    "no topos skill found to reference — install it with `openclaw skills install @Krv-Labs/topos`, \
     then re-run `topos install pi`";

/// The directory pi scans for skills with no configuration at all.
fn native_skill_dir(home: &Path) -> PathBuf {
    home.join(".pi/agent/skills")
}

/// Why this artifact does or does not apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SkillSource {
    /// A topos skill already sits in a directory pi scans by default —
    /// commonly as an openclaw symlink into `~/.agents/skills`. Referencing it
    /// again would be a second path to the same file.
    Discovered,
    /// The skill is installed outside pi's default scan; `skills` needs this
    /// directory.
    Referenceable(PathBuf),
    /// No topos skill anywhere this knows to look.
    Missing,
}

pub(crate) fn config_path(home: &Path) -> PathBuf {
    home.join(".pi/agent/settings.json")
}

pub(crate) fn skill_source(home: &Path) -> SkillSource {
    if holds_topos_skill(&native_skill_dir(home)) {
        return SkillSource::Discovered;
    }
    CANDIDATE_DIRS
        .iter()
        .map(|relative| home.join(relative))
        .find(|dir| holds_topos_skill(dir))
        .map_or(SkillSource::Missing, SkillSource::Referenceable)
}

/// The directory this artifact would add to `skills`, if any.
pub(crate) fn skill_dir(home: &Path) -> Option<PathBuf> {
    match skill_source(home) {
        SkillSource::Referenceable(dir) => Some(dir),
        _ => None,
    }
}

/// `is_file` rather than `exists`, and deliberately symlink-following: the
/// per-harness skill directories are symlink farms into openclaw's shared
/// namespace, and a link into a live skill counts as that skill being here.
fn holds_topos_skill(dir: &Path) -> bool {
    dir.join("topos").join("SKILL.md").is_file()
}

/// `None` when there is nothing to reference — either pi already found the
/// skill or none is installed. The caller says which and moves on; neither is
/// a failure.
pub(crate) fn inspect(home: &Path) -> Option<Inspection> {
    let wanted = skill_dir(home)?;
    let path = config_path(home);
    let map = match read_json_object(&path) {
        Ok(map) => map,
        Err(message) => return Some(Inspection::conflict(message)),
    };
    Some(match map.get(SKILLS_KEY) {
        // A non-array `skills` is not ours to reinterpret.
        Some(value) if !value.is_array() => Inspection::conflict(format!(
            "{} `{SKILLS_KEY}` must be an array",
            path.display()
        )),
        Some(value) if lists(value, home, &wanted) => Inspection::plain(State::Active),
        _ => Inspection::plain(State::Absent),
    })
}

/// `Ok(None)` means the reference was already there and nothing was written.
pub(crate) fn apply(home: &Path) -> Result<Option<WriteOutcome>, String> {
    let Some(inspection) = inspect(home) else {
        return Ok(None);
    };
    match inspection.state {
        State::Active => Ok(None),
        State::Conflict => Err(inspection.detail.unwrap_or_else(|| {
            format!("{} cannot be updated safely", config_path(home).display())
        })),
        state => write(home, state == State::Absent).map(Some),
    }
}

fn write(home: &Path, backup: bool) -> Result<WriteOutcome, String> {
    let wanted = skill_dir(home).ok_or_else(|| "no topos skill to reference".to_string())?;
    let path = config_path(home);
    let mut map = read_json_object(&path)?;
    let entry = map
        .entry(SKILLS_KEY.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Value::Array(paths) = entry else {
        return Err(format!(
            "{} `{SKILLS_KEY}` must be an array",
            path.display()
        ));
    };
    // Append rather than replace: the array is the user's list of skill
    // sources, and every other entry in it belongs to someone else.
    paths.push(Value::String(wanted.display().to_string()));
    write_json_object(&path, &map, backup)
}

/// Remove only the path this module would have written. `Ok(false)` when there
/// was nothing of ours in the array.
pub(crate) fn remove(home: &Path, dry_run: bool) -> Result<bool, String> {
    let path = config_path(home);
    if !path.is_file() {
        return Ok(false);
    }
    // Not `skill_dir`: uninstall has to find the entry even after the skill
    // itself was removed, so match any of the directories install could have
    // written rather than the one it would write now.
    let mut map = read_json_object(&path)?;
    let Some(Value::Array(paths)) = map.get_mut(SKILLS_KEY) else {
        return Ok(false);
    };
    let before = paths.len();
    paths.retain(|value| !ours(value, home));
    if paths.len() == before {
        return Ok(false);
    }
    // An emptied `skills` is dropped so `settings.json` can go back to being
    // an empty object, which is what authorizes deleting a file we created.
    if paths.is_empty() {
        map.remove(SKILLS_KEY);
    }
    if !dry_run {
        write_json_object(&path, &map, false)?;
    }
    Ok(true)
}

/// True when the array already lists `wanted`, in either the absolute form this
/// module writes or the `~/`-prefixed form pi's own docs use.
fn lists(value: &Value, home: &Path, wanted: &Path) -> bool {
    value
        .as_array()
        .is_some_and(|paths| paths.iter().any(|entry| same_dir(entry, home, wanted)))
}

fn same_dir(entry: &Value, home: &Path, wanted: &Path) -> bool {
    entry
        .as_str()
        .is_some_and(|text| expand(text, home) == wanted)
}

/// True for any candidate directory, so uninstall is not defeated by the skill
/// having moved or been removed since install ran.
fn ours(entry: &Value, home: &Path) -> bool {
    let Some(text) = entry.as_str() else {
        return false;
    };
    let expanded = expand(text, home);
    CANDIDATE_DIRS
        .iter()
        .any(|relative| expanded == home.join(relative))
}

fn expand(text: &str, home: &Path) -> PathBuf {
    match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(text),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::commands::install::testing::tmp_dir;

    fn install_skill(home: &Path, relative: &str) -> PathBuf {
        let dir = home.join(relative);
        fs::create_dir_all(dir.join("topos")).unwrap();
        fs::write(dir.join("topos/SKILL.md"), "---\nname: topos\n---\n").unwrap();
        dir
    }

    fn settings(home: &Path) -> serde_json::Map<String, Value> {
        read_json_object(&config_path(home)).unwrap()
    }

    #[test]
    fn with_no_skill_installed_the_artifact_does_not_apply() {
        let home = tmp_dir("pi-no-skill");
        assert_eq!(skill_source(&home), SkillSource::Missing);
        assert!(skill_dir(&home).is_none());
        assert!(inspect(&home).is_none());
        // And applying is a no-op rather than an error, so install still passes.
        assert!(apply(&home).unwrap().is_none());
        assert!(!config_path(&home).exists(), "wrote settings.json anyway");
        fs::remove_dir_all(home).ok();
    }

    /// pi scans its own skills directory with no configuration, so a skill
    /// sitting there must not also be referenced by path — and must be
    /// distinguishable from "no skill at all", which gets opposite advice.
    #[test]
    fn a_skill_pi_already_finds_needs_no_reference() {
        let home = tmp_dir("pi-native-skill");
        install_skill(&home, ".pi/agent/skills");
        install_skill(&home, ".agents/skills");
        assert_eq!(skill_source(&home), SkillSource::Discovered);
        assert!(skill_dir(&home).is_none());
        fs::remove_dir_all(home).ok();
    }

    /// The real shape on a machine with openclaw: `~/.pi/agent/skills` is a
    /// symlink farm into `~/.agents/skills`, so the skill is already on pi's
    /// default scan path even though the bytes live elsewhere.
    #[cfg(unix)]
    #[test]
    fn an_openclaw_symlink_into_pis_own_directory_counts_as_discovered() {
        let home = tmp_dir("pi-symlink-farm");
        let shared = install_skill(&home, ".agents/skills");
        let native = home.join(".pi/agent/skills");
        fs::create_dir_all(&native).unwrap();
        std::os::unix::fs::symlink(shared.join("topos"), native.join("topos")).unwrap();

        assert_eq!(skill_source(&home), SkillSource::Discovered);
        assert!(apply(&home).unwrap().is_none());
        assert!(
            !config_path(&home).exists(),
            "referenced a skill pi already had"
        );
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn openclaws_namespace_is_preferred_over_a_per_harness_copy() {
        let home = tmp_dir("pi-prefers-agents");
        install_skill(&home, ".claude/skills");
        let shared = install_skill(&home, ".agents/skills");
        assert_eq!(skill_dir(&home), Some(shared));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn the_reference_round_trips_and_is_idempotent() {
        let home = tmp_dir("pi-round-trip");
        let dir = install_skill(&home, ".agents/skills");

        assert_eq!(inspect(&home).unwrap().state, State::Absent);
        assert!(apply(&home).unwrap().is_some());
        assert_eq!(inspect(&home).unwrap().state, State::Active);
        assert_eq!(
            settings(&home)["skills"],
            json_array(&[dir.display().to_string()])
        );

        // Re-running install must not append a second copy.
        assert!(apply(&home).unwrap().is_none());
        assert_eq!(settings(&home)["skills"].as_array().unwrap().len(), 1);

        assert!(remove(&home, false).unwrap());
        assert!(
            settings(&home).get("skills").is_none(),
            "empty key survived"
        );
        assert!(!remove(&home, false).unwrap());
        fs::remove_dir_all(home).ok();
    }

    fn json_array(items: &[String]) -> Value {
        Value::Array(items.iter().cloned().map(Value::String).collect())
    }

    /// pi's own docs write these paths with `~/`, so a hand-added entry in that
    /// form is already correct and must not be duplicated.
    #[test]
    fn a_tilde_path_counts_as_already_referenced() {
        let home = tmp_dir("pi-tilde");
        install_skill(&home, ".agents/skills");
        let path = config_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"skills": ["~/.agents/skills"]}"#).unwrap();

        assert_eq!(inspect(&home).unwrap().state, State::Active);
        assert!(apply(&home).unwrap().is_none());
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn foreign_skill_paths_and_settings_survive_both_directions() {
        let home = tmp_dir("pi-foreign");
        install_skill(&home, ".agents/skills");
        let path = config_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"theme": "dark", "skills": ["~/.codex/skills"]}"#).unwrap();

        apply(&home).unwrap();
        assert_eq!(settings(&home)["skills"].as_array().unwrap().len(), 2);

        remove(&home, false).unwrap();
        let after = settings(&home);
        assert_eq!(after["theme"], "dark");
        assert_eq!(
            after["skills"],
            json_array(&["~/.codex/skills".to_string()])
        );
        fs::remove_dir_all(home).ok();
    }

    /// The entry has to come out even when the skill it pointed at is gone.
    #[test]
    fn uninstall_finds_the_entry_after_the_skill_is_deleted() {
        let home = tmp_dir("pi-skill-deleted");
        let dir = install_skill(&home, ".agents/skills");
        apply(&home).unwrap();
        fs::remove_dir_all(&dir).unwrap();

        assert!(
            skill_dir(&home).is_none(),
            "fixture did not remove the skill"
        );
        assert!(remove(&home, false).unwrap());
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_non_array_skills_key_is_a_conflict_and_is_never_rewritten() {
        let home = tmp_dir("pi-bad-skills");
        install_skill(&home, ".agents/skills");
        let path = config_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"skills": "everything"}"#).unwrap();

        assert_eq!(inspect(&home).unwrap().state, State::Conflict);
        assert!(apply(&home).is_err());
        assert_eq!(settings(&home)["skills"], "everything");
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_dry_run_removal_reports_without_writing() {
        let home = tmp_dir("pi-dry-run");
        install_skill(&home, ".agents/skills");
        apply(&home).unwrap();

        assert!(remove(&home, true).unwrap());
        assert_eq!(inspect(&home).unwrap().state, State::Active);
        fs::remove_dir_all(home).ok();
    }
}
