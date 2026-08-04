//! Things `topos install` can see but will never touch.
//!
//! The governing invariant of this command is that **topos mutates only files
//! it registers an MCP server into**. That leaves a second category: artifacts
//! an earlier draft of this branch wrote, files owned by a different
//! distribution channel, and registrations the user made by hand. Removing any
//! of them is either unsafe or not topos's call, but leaving them invisible is
//! how a machine ends up with two `topos mcp` processes and a stale `@import`
//! nobody can account for. So this module *reports* and nothing else: there is
//! no write path, no removal path, and no repair path in here.
//!
//! Why each item is report-only:
//!
//! - `~/.copilot/copilot-instructions.md` and `~/.gemini/GEMINI.md` are shared
//!   with other tools and hand-edited by users. An earlier draft wrote into
//!   both; deleting our block back out risks eating adjacent user content.
//! - `~/.claude/skills/topos/SKILL.md` and `~/.agents/skills/topos/SKILL.md`
//!   belong to openclaw / ClawHub / Hermes, not to this command. `~/.agents`
//!   is openclaw's namespace, tracked in `~/.agents/.skill-lock.json`, and the
//!   per-harness skill directories are symlink farms into it — deleting
//!   through one would destroy openclaw's shared files. The advice for these
//!   therefore names `openclaw`, never a topos command.
//! - A duplicate MCP registration under a key other than `topos` is the user's
//!   own entry. It is worth reporting, because two registrations mean
//!   duplicate tool names and two servers, but renaming or deleting somebody
//!   else's config entry is not something an installer should do.
//!
//! Detection is a table of `(path, predicate)` rules plus one pass over
//! [`HARNESSES`], rather than a chain of conditionals, so adding a rule is a
//! row and never a branch.

use std::path::{Path, PathBuf};

use super::harness::{HarnessSpec, HARNESSES};

/// Something topos will never modify but must not leave invisible: draft-era
/// artifacts from an earlier version of this branch, and hand-made duplicate
/// registrations.
pub(crate) struct Residue {
    pub(crate) path: PathBuf,
    /// What was found, e.g. "instruction block left by an earlier topos draft".
    pub(crate) what: String,
    /// What the user should do, e.g. "remove the `<!-- topos:start -->` block".
    pub(crate) advice: String,
}

/// One report-only detection: a fixed location under `$HOME` plus a predicate
/// that decides whether what is there is ours to mention.
struct Rule {
    /// Location relative to `$HOME`. Spelled literally rather than pulled from
    /// [`super::paths`] because none of these are files topos registers into,
    /// so none of them belong in the config-path table.
    relative: &'static str,
    /// Read-only test of the candidate path.
    detected: fn(&Path) -> bool,
    what: &'static str,
    advice: &'static str,
}

/// Every non-MCP artifact worth surfacing, in the order `topos status` prints
/// them: draft leftovers first, then the separately-distributed skill files.
const RULES: [Rule; 5] = [
    Rule {
        relative: ".copilot/copilot-instructions.md",
        detected: has_instruction_block,
        what: "instruction block left by an earlier topos draft",
        advice: "remove the `<!-- topos:start -->` block by hand — this file is shared with GitHub Copilot, so topos will not edit it",
    },
    Rule {
        relative: ".gemini/GEMINI.md",
        detected: imports_topos_skill,
        what: "`@import` of `topos-skill.md` left by an earlier topos draft",
        advice: "delete that one `@import` line by hand — this file is shared with Gemini CLI and holds your own rules, so topos will not edit it",
    },
    Rule {
        relative: ".gemini/topos-skill.md",
        detected: Path::exists,
        what: "skill text written by an earlier topos draft",
        advice: "delete this file by hand once nothing `@import`s it — current topos versions never write it",
    },
    Rule {
        relative: ".claude/skills/topos/SKILL.md",
        detected: Path::exists,
        what: "topos agent skill, installed by openclaw rather than by `topos install`",
        advice: "remove it with `openclaw skills uninstall @Krv-Labs/topos`, or delete the `topos` skill directory by hand",
    },
    Rule {
        relative: ".agents/skills/topos/SKILL.md",
        detected: Path::exists,
        what: "topos agent skill in openclaw's shared namespace, tracked in `~/.agents/.skill-lock.json`",
        advice: "remove it with `openclaw skills uninstall @Krv-Labs/topos` — the per-harness skill directories are symlinks into here, so never delete through one of those",
    },
];

/// The marker an earlier draft wrapped its prose instructions in.
const BLOCK_MARKER: &str = "<!-- topos:start -->";

/// The skill file an earlier draft dropped beside `GEMINI.md` and `@import`ed.
const SKILL_FILE: &str = "topos-skill.md";

/// Read-only scan. Never writes, never deletes.
pub(crate) fn scan(home: &Path, binary: &Path) -> Vec<Residue> {
    let drafts = RULES.iter().filter_map(|rule| rule.check(home));
    let duplicates = HARNESSES
        .iter()
        .flat_map(|spec| duplicates_in(spec, home, binary));
    drafts.chain(duplicates).collect()
}

impl Rule {
    /// The residue this rule describes, when its path is present and matches.
    fn check(&self, home: &Path) -> Option<Residue> {
        let path = home.join(self.relative);
        (self.detected)(&path).then(|| Residue {
            path,
            what: self.what.to_string(),
            advice: self.advice.to_string(),
        })
    }
}

/// True when an earlier draft's delimited instruction block is still in place.
fn has_instruction_block(path: &Path) -> bool {
    read(path).is_some_and(|text| text.contains(BLOCK_MARKER))
}

/// True when any line pulls in the draft's skill file.
///
/// Line-scoped and `topos-skill.md`-specific on purpose: a user's own
/// `@import` lines are none of our business and must not be reported.
fn imports_topos_skill(path: &Path) -> bool {
    read(path).is_some_and(|text| text.lines().any(is_topos_import))
}

fn is_topos_import(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("@import") && trimmed.contains(SKILL_FILE)
}

/// Contents of a text file, or `None` when it is missing or unreadable. An
/// unreadable file is silently skipped: `scan` runs inside `topos status`,
/// where an I/O error on a file topos does not own is not news.
fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// MCP registrations in one harness config that point at topos under some key
/// other than `topos` itself.
fn duplicates_in(spec: &HarnessSpec, home: &Path, binary: &Path) -> Vec<Residue> {
    let path = (spec.config_path)(home);
    spec.artifact
        .duplicate_keys(&path, binary)
        .into_iter()
        .map(|key| duplicate(spec, path.clone(), &key))
        .collect()
}

fn duplicate(spec: &HarnessSpec, path: PathBuf, key: &str) -> Residue {
    Residue {
        path,
        what: format!(
            "a second MCP registration, `{key}`, also runs the topos binary in {}",
            spec.name
        ),
        advice: format!(
            "two registrations mean duplicate tool names and two `topos mcp` processes — remove `{key}` by hand, since topos never edits an entry it did not write"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A scratch `$HOME`. Every test needs its own `label`: `cargo test` is
    /// threaded and the process id is shared, so two tests reusing a label
    /// would wipe each other's seeds.
    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("topos-residue-{label}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Seed `home/relative` with `text`, creating parents.
    fn seed(home: &Path, relative: &str, text: &str) -> PathBuf {
        let path = home.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// A real, executable stand-in for the topos binary, mirroring the helper
    /// in `json_entry`'s tests so duplicate detection has an identity to match.
    fn fake_binary(dir: &Path) -> PathBuf {
        let path = dir.join("topos");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// Every file under `dir`, as `(relative path, bytes)`, sorted — enough to
    /// prove a scan neither edited nor created anything.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        collect(dir, dir, &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn collect(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                out.push((relative, fs::read(&path).unwrap_or_default()));
            }
        }
    }

    /// The five file-backed rules, each with content that must trigger it.
    const SEEDS: [(&str, &str); 5] = [
        (
            ".copilot/copilot-instructions.md",
            "my notes\n<!-- topos:start -->\nblock\n<!-- topos:end -->\n",
        ),
        // Indented, so `trim_start` is exercised rather than assumed.
        (".gemini/GEMINI.md", "# rules\n  @import ./topos-skill.md\n"),
        (".gemini/topos-skill.md", "# topos\n"),
        (".claude/skills/topos/SKILL.md", "---\nname: topos\n---\n"),
        (".agents/skills/topos/SKILL.md", "---\nname: topos\n---\n"),
    ];

    #[test]
    fn every_single_condition_is_detected_on_its_own() {
        let root = scratch("single");
        let binary = fake_binary(&root);
        for (index, (relative, text)) in SEEDS.iter().enumerate() {
            let home = root.join(format!("home{index}"));
            fs::create_dir_all(&home).unwrap();
            let path = seed(&home, relative, text);

            let found = scan(&home, &binary);
            assert_eq!(found.len(), 1, "{relative} produced {} rows", found.len());
            assert_eq!(found[0].path, path);
            assert!(!found[0].what.is_empty(), "{relative} has no description");
            assert!(!found[0].advice.is_empty(), "{relative} has no advice");
        }
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_hand_made_second_registration_is_reported_but_named_as_the_users_own() {
        let root = scratch("duplicate");
        let binary = fake_binary(&root);
        let home = root.join("home");
        // Antigravity's config, the one place a hand-rolled `topos-mcp` key was
        // actually observed.
        let path = seed(
            &home,
            ".gemini/config/mcp_config.json",
            r#"{"mcpServers": {
                 "topos": {"command": "/usr/local/bin/topos", "args": ["mcp"]},
                 "topos-mcp": {"command": "topos", "args": ["mcp"]},
                 "unrelated": {"command": "uvx", "args": ["other"]}
               }}"#,
        );

        let found = scan(&home, &binary);
        assert_eq!(found.len(), 1, "expected exactly one duplicate");
        assert_eq!(found[0].path, path);
        assert!(found[0].what.contains("topos-mcp"), "{}", found[0].what);
        assert!(found[0].advice.contains("by hand"), "{}", found[0].advice);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_clean_home_has_no_residue() {
        let home = scratch("clean");
        let binary = fake_binary(&home);
        assert!(scan(&home, &binary).is_empty());
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_users_own_gemini_rules_and_imports_are_not_our_business() {
        let root = scratch("foreign-import");
        let binary = fake_binary(&root);
        let home = root.join("home");
        seed(
            &home,
            ".gemini/GEMINI.md",
            // Both halves of the rule's AND are violated here: imports that are
            // not ours, and a mention of `topos-skill.md` that is not an import.
            "# my rules\n@import ./team-style.md\n@import ../shared/notes.md\nSee topos-skill.md for background.\nUse tabs.\n",
        );

        assert!(scan(&home, &binary).is_empty(), "a user's imports leaked");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn skill_advice_never_suggests_a_topos_command() {
        let root = scratch("skill-advice");
        let binary = fake_binary(&root);
        let home = root.join("home");
        for relative in [
            ".claude/skills/topos/SKILL.md",
            ".agents/skills/topos/SKILL.md",
        ] {
            seed(&home, relative, "---\nname: topos\n---\n");
        }

        let found = scan(&home, &binary);
        assert_eq!(found.len(), 2);
        for residue in &found {
            assert!(residue.advice.contains("openclaw"), "{}", residue.advice);
            assert!(
                !residue.advice.contains("topos install")
                    && !residue.advice.contains("topos uninstall"),
                "skill removal must never route through topos: {}",
                residue.advice
            );
        }
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn scanning_writes_nothing_at_all() {
        let root = scratch("read-only");
        let binary = fake_binary(&root);
        let home = root.join("home");
        for (relative, text) in SEEDS {
            seed(&home, relative, text);
        }
        seed(
            &home,
            ".gemini/config/mcp_config.json",
            r#"{"mcpServers": {"topos-mcp": {"command": "topos", "args": ["mcp"]}}}"#,
        );

        let before = snapshot(&home);
        let found = scan(&home, &binary);
        let after = snapshot(&home);

        assert_eq!(found.len(), SEEDS.len() + 1, "seeded residue went missing");
        // Byte-for-byte, and the same set of files: no edits, no backups, no
        // stray temp files, no directories brought into existence.
        assert_eq!(before, after, "scan modified the filesystem");
        fs::remove_dir_all(root).ok();
    }
}
