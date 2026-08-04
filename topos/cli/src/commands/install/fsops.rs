//! Filesystem primitives shared by install, uninstall and status: atomic
//! writes with optional backups, JSON and JSONC parsing, and directory pruning.
//!
//! Two rules here exist because the draft got them wrong in ways that only show
//! up on a real machine:
//!
//! * **The caller decides whether to back up**, rather than `atomic_write`
//!   inferring it from the file's existence. Backing up whenever a file exists
//!   means the second install overwrites the pristine pre-install snapshot with
//!   one that already contains topos's own output. See [`atomic_write`].
//! * **A write follows a symlink and replaces its target.** Users symlink these
//!   configs into dotfile repositories; renaming over the link would silently
//!   convert it to a regular file.
//!
//! Every write reports the directories it had to create, so uninstall can undo
//! exactly that much instead of guessing from a static list.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Suffix for the snapshot taken before topos first modifies a file.
const BACKUP_SUFFIX: &str = ".topos.backup";
/// Suffix for the temp file an atomic write renames into place.
const TMP_SUFFIX: &str = ".topos.tmp";

/// What a write had to bring into existence, so uninstall can undo exactly that
/// much.
pub(crate) struct WriteOutcome {
    /// Directories created by this write, shallowest first.
    pub(crate) created_dirs: Vec<PathBuf>,
    /// True when the file itself did not exist before this write.
    pub(crate) created_file: bool,
}

pub(crate) fn backup_path(path: &Path) -> PathBuf {
    suffixed(path, BACKUP_SUFFIX)
}

fn tmp_path(path: &Path) -> PathBuf {
    suffixed(path, TMP_SUFFIX)
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write `contents` to `path` via a temp file and a rename, creating parent
/// directories as needed.
///
/// `backup` snapshots the previous contents to `<name>.topos.backup`. Pass
/// `true` **only when the current contents were not written by topos**: a
/// snapshot taken over our own output destroys the pristine one, and
/// self-healing path repair makes re-writing an already-configured file
/// routine. A backup is skipped for a file that does not exist yet — there is
/// nothing of the user's to preserve.
pub(crate) fn atomic_write(
    path: &Path,
    contents: &str,
    backup: bool,
) -> Result<WriteOutcome, String> {
    let created_dirs = create_parents(path)?;
    // Resolve before writing so the temp file lands beside the real file and
    // the rename cannot replace a symlink with a regular file.
    let target = resolve_symlink(path);
    let created_file = !target.exists();
    #[cfg(unix)]
    let permissions = if created_file {
        None
    } else {
        Some(
            fs::metadata(&target)
                .map_err(|e| format!("reading permissions for {}: {e}", target.display()))?
                .permissions(),
        )
    };
    if backup && !created_file {
        fs::copy(&target, backup_path(&target))
            .map_err(|e| format!("backing up {}: {e}", target.display()))?;
    }
    let tmp = tmp_path(&target);
    fs::write(&tmp, contents).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    if let Some(permissions) = permissions {
        fs::set_permissions(&tmp, permissions)
            .map_err(|e| format!("setting permissions on {}: {e}", tmp.display()))?;
    }
    fs::rename(&tmp, &target).map_err(|e| format!("replacing {}: {e}", target.display()))?;
    Ok(WriteOutcome {
        created_dirs,
        created_file,
    })
}

/// The file a write should actually land on.
///
/// `~/.claude.json`, `~/.gemini/settings.json`, `~/.codex/config.toml` and
/// `~/.cursor/mcp.json` are all commonly symlinked into a dotfile repository by
/// stow or chezmoi. [`atomic_write`] ends in a rename, which would replace the
/// link with a regular file, orphan the dotfile repository, and leave the file
/// behind after uninstall. A broken link has no target to follow, so it is
/// written literally.
pub(crate) fn resolve_symlink(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    if !is_link {
        return path.to_path_buf();
    }
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Create every missing ancestor of `path`, returning those actually created,
/// shallowest first.
///
/// Built from `create_dir` per level rather than `create_dir_all` because the
/// caller needs to know which directories it brought into existence — that list
/// is the whole basis for leaving no trace on uninstall.
fn create_parents(path: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    let missing = missing_ancestors(parent);
    for dir in &missing {
        // `AlreadyExists` is not an error: a concurrent process or a
        // case-insensitive filesystem can win the race harmlessly.
        if let Err(e) = fs::create_dir(dir) {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(format!("creating {}: {e}", dir.display()));
            }
        }
    }
    Ok(missing)
}

/// Ancestors of `dir` that do not exist yet, shallowest first.
fn missing_ancestors(dir: &Path) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|ancestor| !ancestor.exists())
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    missing
}

/// Read a JSON object config. A missing or whitespace-only file is an empty
/// map; a parse failure or a non-object top level is an error, so callers report
/// a conflict instead of clobbering something they cannot read.
pub(crate) fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let Some(text) = read_optional(path)? else {
        return Ok(Map::new());
    };
    parse_object(&text, path)
}

/// Read a JSONC object config — VS Code's `mcp.json`. The flag is true when the
/// source contained comments, which means the caller must refuse to rewrite it
/// rather than destroy them.
pub(crate) fn read_jsonc_object(path: &Path) -> Result<(Map<String, Value>, bool), String> {
    let Some(text) = read_optional(path)? else {
        return Ok((Map::new(), false));
    };
    let (stripped, had_comments) = strip_jsonc(&text);
    parse_object(&stripped, path).map(|map| (map, had_comments))
}

/// File contents, or `None` when the file is missing or holds only whitespace.
fn read_optional(path: &Path) -> Result<Option<String>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    Ok(Some(text).filter(|t| !t.trim().is_empty()))
}

fn parse_object(text: &str, path: &Path) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!(
            "{} top-level value must be an object",
            path.display()
        )),
        Err(e) => Err(format!("parsing {}: {e}", path.display())),
    }
}

/// Serialize and [`atomic_write`] a JSON object, with a trailing newline.
pub(crate) fn write_json_object(
    path: &Path,
    data: &Map<String, Value>,
    backup: bool,
) -> Result<WriteOutcome, String> {
    let contents = serde_json::to_string_pretty(data).map_err(|e| e.to_string())? + "\n";
    atomic_write(path, &contents, backup)
}

/// Replace `//` and `/* */` comments with spaces and drop trailing commas,
/// respecting string literals and escapes.
///
/// Comments become spaces rather than disappearing so that offsets and line
/// numbers in a `serde_json` parse error still point at the right place in the
/// user's file.
pub(crate) fn strip_jsonc(text: &str) -> (String, bool) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut had_comments = false;
    while index < chars.len() {
        index += match comment_at(&chars, index) {
            Some(len) => {
                had_comments = true;
                blank_out(&chars[index..index + len], &mut out);
                len
            }
            None => copy_token(&chars, index, &mut out),
        };
    }
    (drop_trailing_commas(&out), had_comments)
}

/// The length of the comment starting at `index`, or `None` if none does.
fn comment_at(chars: &[char], index: usize) -> Option<usize> {
    if chars[index] != '/' {
        return None;
    }
    match chars.get(index + 1) {
        Some('/') => Some(scan_to(chars, index + 2, |c, _| c == '\n')),
        Some('*') => Some(scan_to(chars, index + 2, |c, next| c == '*' && next == Some('/')) + 2),
        _ => None,
    }
}

/// Distance from `start` to the first position satisfying `end`, or to the end
/// of input. Returns the length measured from the comment's opening delimiter.
fn scan_to(chars: &[char], start: usize, end: impl Fn(char, Option<char>) -> bool) -> usize {
    let mut index = start;
    while index < chars.len() && !end(chars[index], chars.get(index + 1).copied()) {
        index += 1;
    }
    index.min(chars.len()) - start + 2
}

/// Emit spaces for a comment's characters, keeping newlines so line numbers in
/// a later parse error still line up with the user's file.
fn blank_out(comment: &[char], out: &mut String) {
    out.extend(comment.iter().map(|c| if *c == '\n' { '\n' } else { ' ' }));
}

/// Copy one token — a whole string literal, or a single character — and report
/// how many characters were consumed.
///
/// Strings are copied whole so a `//` or `/*` inside one is never mistaken for
/// a comment, and a backslash consumes the character after it so an escaped
/// quote does not end the literal.
fn copy_token(chars: &[char], index: usize, out: &mut String) -> usize {
    if chars[index] != '"' {
        out.push(chars[index]);
        return 1;
    }
    let mut cursor = index + 1;
    while cursor < chars.len() && chars[cursor] != '"' {
        cursor += if chars[cursor] == '\\' { 2 } else { 1 };
    }
    let end = (cursor + 1).min(chars.len());
    out.extend(chars[index..end].iter());
    end - index
}

/// Remove a comma that is followed only by whitespace and a closing brace or
/// bracket. Runs after comment stripping, so `[1, /* two */]` is handled too.
fn drop_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &current) in chars.iter().enumerate() {
        let toggles = current == '"' && !escaped;
        escaped = in_string && current == '\\' && !escaped;
        if toggles {
            in_string = !in_string;
        }
        if !in_string && current == ',' && closes_next(&chars, index) {
            out.push(' ');
        } else {
            out.push(current);
        }
    }
    out
}

fn closes_next(chars: &[char], index: usize) -> bool {
    chars[index + 1..]
        .iter()
        .find(|c| !c.is_whitespace())
        .is_some_and(|c| *c == '}' || *c == ']')
}

/// File names that do not count when deciding whether a directory is empty.
///
/// `.DS_Store` is the important one: it exists in `~/.cursor`, `~/.copilot` and
/// `~/.codex/skills` on a real macOS machine and can never be reproduced in a
/// scratch `$HOME`, so without this every uninstall would leave those
/// directories behind. The rest are topos's own leftovers.
pub(crate) fn is_ignorable(name: &str) -> bool {
    name == ".DS_Store" || name.ends_with(BACKUP_SUFFIX) || name.ends_with(TMP_SUFFIX)
}

/// True when `dir` exists and holds nothing but [`is_ignorable`] entries.
pub(crate) fn is_effectively_empty(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .all(|entry| is_ignorable(&entry.file_name().to_string_lossy()))
}

/// Remove `dirs` child before parent, skipping any that still hold real
/// content. Returns the directories removed, or under `dry_run` the ones that
/// would be.
pub(crate) fn prune_dirs(dirs: &[PathBuf], dry_run: bool) -> Vec<PathBuf> {
    // Deepest first, so a parent is only considered after its children are gone.
    let mut ordered: Vec<&PathBuf> = dirs.iter().collect();
    ordered.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    ordered
        .into_iter()
        .filter(|dir| is_effectively_empty(dir))
        .filter(|dir| dry_run || remove_dir(dir))
        .cloned()
        .collect()
}

/// Delete a directory along with the ignorable entries keeping it non-empty.
fn remove_dir(dir: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            fs::remove_file(entry.path()).ok();
        }
    }
    fs::remove_dir(dir).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::testing::tmp_dir;

    #[test]
    fn a_write_replaces_contents_and_reports_no_new_directories() {
        let dir = tmp_dir("basic");
        let path = dir.join("config.json");
        fs::write(&path, "old").unwrap();

        let outcome = atomic_write(&path, "new", false).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert!(outcome.created_dirs.is_empty());
        assert!(!outcome.created_file);
        assert!(!backup_path(&path).exists(), "backup was not requested");
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn replacing_an_existing_file_preserves_its_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmp_dir("permissions");
        let path = dir.join("config.toml");
        fs::write(&path, "secret").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        atomic_write(&path, "updated", false).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_backup_captures_the_prior_bytes_only_when_there_are_any() {
        let dir = tmp_dir("backup");
        let fresh = dir.join("fresh.json");

        // Nothing of the user's exists yet, so there is nothing to preserve.
        let outcome = atomic_write(&fresh, "first", true).unwrap();
        assert!(outcome.created_file);
        assert!(!backup_path(&fresh).exists());

        let existing = dir.join("existing.json");
        fs::write(&existing, "user content").unwrap();
        atomic_write(&existing, "ours", true).unwrap();
        assert_eq!(
            fs::read_to_string(backup_path(&existing)).unwrap(),
            "user content"
        );
        fs::remove_dir_all(dir).ok();
    }

    /// The defect this parameter exists to prevent: a second write must not be
    /// able to overwrite the pristine snapshot with topos's own output.
    #[test]
    fn a_second_write_without_backup_leaves_the_pristine_snapshot_intact() {
        let dir = tmp_dir("pristine");
        let path = dir.join("config.json");
        fs::write(&path, "user content").unwrap();

        atomic_write(&path, "ours v1", true).unwrap();
        atomic_write(&path, "ours v2", false).unwrap();

        assert_eq!(
            fs::read_to_string(backup_path(&path)).unwrap(),
            "user content"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlink_keeps_the_link_and_updates_its_target() {
        let dir = tmp_dir("symlink");
        let real = dir.join("dotfiles").join("claude.json");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "original").unwrap();
        let link = dir.join(".claude.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        atomic_write(&link, "updated", true).unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the rename replaced the symlink with a regular file"
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "updated");
        // The backup belongs beside the real file, inside the dotfile repo.
        assert_eq!(fs::read_to_string(backup_path(&real)).unwrap(), "original");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn created_directories_are_reported_shallowest_first() {
        let dir = tmp_dir("dirs");
        let path = dir.join("a/b/c/config.json");

        let outcome = atomic_write(&path, "{}", false).unwrap();

        assert_eq!(
            outcome.created_dirs,
            vec![dir.join("a"), dir.join("a/b"), dir.join("a/b/c")]
        );
        assert!(outcome.created_file);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_or_blank_file_reads_as_an_empty_object() {
        let dir = tmp_dir("read");
        let blank = dir.join("blank.json");
        fs::write(&blank, "   \n").unwrap();

        assert!(read_json_object(&dir.join("absent.json"))
            .unwrap()
            .is_empty());
        assert!(read_json_object(&blank).unwrap().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unreadable_content_is_an_error_rather_than_a_silent_empty_object() {
        let dir = tmp_dir("bad");
        let broken = dir.join("broken.json");
        fs::write(&broken, "{ nope").unwrap();
        let array = dir.join("array.json");
        fs::write(&array, "[1, 2]").unwrap();

        assert!(read_json_object(&broken).is_err());
        let message = read_json_object(&array).unwrap_err();
        assert!(message.contains("must be an object"), "{message}");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn key_order_survives_a_read_write_round_trip() {
        // Guards the `preserve_order` feature: without it every key in a user's
        // `~/.claude.json` is alphabetically re-sorted on install.
        let dir = tmp_dir("order");
        let path = dir.join("config.json");
        fs::write(&path, r#"{"zebra": 1, "apple": 2, "mango": 3}"#).unwrap();

        let map = read_json_object(&path).unwrap();
        write_json_object(&path, &map, false).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let zebra = text.find("zebra").unwrap();
        let apple = text.find("apple").unwrap();
        assert!(zebra < apple, "keys were re-sorted:\n{text}");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn comments_are_stripped_but_string_contents_are_not() {
        let (out, had) = strip_jsonc("{\"url\": \"https://x.dev\"} // trailing\n");
        assert!(had);
        assert!(out.contains("https://x.dev"), "{out}");
        assert!(!out.contains("trailing"), "{out}");

        let (out, had) = strip_jsonc(r#"{"a": "/* not a comment */"}"#);
        assert!(!had);
        assert!(out.contains("/* not a comment */"), "{out}");
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        let (out, had) = strip_jsonc(r#"{"a": "he said \"hi\" // no"}"#);
        assert!(!had, "a comment was found inside a string literal");
        assert!(out.contains("// no"), "{out}");
    }

    #[test]
    fn block_comments_and_trailing_commas_are_removed() {
        let source = "{\n  /* a\n     b */\n  \"x\": 1,\n  \"y\": [2, 3,],\n}\n";
        let (out, had) = strip_jsonc(source);

        assert!(had);
        let parsed: Value = serde_json::from_str(&out).expect(&out);
        assert_eq!(parsed["x"], 1);
        assert_eq!(parsed["y"], serde_json::json!([2, 3]));
        // Line count is preserved so parse-error offsets still make sense.
        assert_eq!(out.lines().count(), source.lines().count());
    }

    #[test]
    fn a_comment_free_file_reports_no_comments() {
        let dir = tmp_dir("jsonc-clean");
        let path = dir.join("mcp.json");
        fs::write(&path, r#"{"servers": {}}"#).unwrap();

        let (map, had_comments) = read_jsonc_object(&path).unwrap();
        assert!(!had_comments);
        assert!(map.contains_key("servers"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pruning_removes_empty_directories_child_first_and_stops_at_real_content() {
        let dir = tmp_dir("prune");
        let empty = dir.join("keep/a/b");
        fs::create_dir_all(&empty).unwrap();
        let occupied = dir.join("occupied");
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join("user.txt"), "mine").unwrap();

        // Deliberately parent-first: the function must reorder.
        let removed = prune_dirs(
            &[
                dir.join("keep"),
                dir.join("keep/a"),
                empty.clone(),
                occupied.clone(),
            ],
            false,
        );

        assert!(!empty.exists());
        assert!(!dir.join("keep").exists());
        assert!(
            occupied.is_dir(),
            "a directory with real content was removed"
        );
        assert_eq!(removed.len(), 3);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_directory_holding_only_ignorable_files_is_still_pruned() {
        let dir = tmp_dir("ignorable");
        let target = dir.join("cursor");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join(".DS_Store"), "").unwrap();
        fs::write(target.join("mcp.json.topos.backup"), "{}").unwrap();

        assert!(is_effectively_empty(&target));
        assert_eq!(
            prune_dirs(std::slice::from_ref(&target), false),
            vec![target.clone()]
        );
        assert!(!target.exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_dry_run_prune_reports_without_removing() {
        let dir = tmp_dir("prune-dry");
        let target = dir.join("empty");
        fs::create_dir_all(&target).unwrap();

        assert_eq!(
            prune_dirs(std::slice::from_ref(&target), true),
            vec![target.clone()]
        );
        assert!(target.is_dir(), "dry run removed the directory");
        fs::remove_dir_all(dir).ok();
    }
}
