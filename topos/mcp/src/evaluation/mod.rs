//! Shared evaluation helpers used by the evaluate / assess / inspect tools.
//!
//! Keeps the core pipeline in one place:
//!
//! 1. Build a `ProgramMorphism`.
//! 2. Attach CFG / academic PDG / CPG / Abstractness (always — they're
//!    derived from the morphism itself and require no external tooling).
//! 3. Optionally attach a module-level `ModuleDependencyGraph` from
//!    GitNexus.
//! 4. Call `CharacteristicMorphism::classify_detailed`.

mod build;
mod classify;
mod depgraph;
mod freshness;
mod gitref;

pub use build::{
    depgraph_analyze_timeout, ensure_gitnexus_dir, ensure_gitnexus_dir_with_cancel,
    ensure_gitnexus_dir_with_progress, graph_build_lock, run_locked_depgraph_generation,
    GitnexusEnsureOutcome, LockedGenerateOutcome,
};
pub use classify::{classify_code_string, classify_file, classify_morphism, detect_language};
pub use depgraph::{
    clear_caches, depgraph_status, last_store_load_ms, load_dep_graph, DepgraphStatus,
};
pub use freshness::clear_freshness_cache;
pub use freshness::{all_source_suffixes, graph_freshness};
pub use gitref::{git_head_mtime, git_head_sha, gitnexus_mtime};

use std::path::{Path, PathBuf};

use crate::security::{
    composable_default_root, resolve_existing_prefix, resolve_file_root, resolve_path_within,
    resolve_project_path,
};

/// Stable prefixes shared by the producer (this module) and the
/// agent-contract consumer (`formatting::composable_contract_signals`) so an
/// invalid/denied override is matched on a single marker.
pub const INVALID_GITNEXUS_MARKERS: [&str; 2] =
    ["gitnexus_dir rejected", "gitnexus_dir unavailable"];

/// Marker inside a "COMPOSABLE not scored" warning meaning the currently
/// checked-out branch has no indexed store.
pub const BRANCH_NOT_INDEXED_MARKER: &str = "no gitnexus store indexed for branch";

/// Stable prefix for staleness warnings.
pub const STALE_GITNEXUS_MARKER: &str = "gitnexus index may be stale";

/// Resolve `override_dir` (if any) into an absolute path: joining a relative
/// override against `default_root` first, then following symlinks on the
/// existing prefix (see [`crate::security::resolve_existing_prefix`]) so a
/// not-yet-generated store path still resolves through any real symlink
/// ahead of the missing tail. `None` when no override is given.
///
/// Downstream resolvers ([`resolve_gitnexus_dir`], [`check_override_warning`],
/// [`depgraph_status`]) join a *relative* override against whatever project
/// root they're handed. Once an override has been used to derive a
/// non-default project root (see [`resolve_composable_project_root`]), that
/// root has already absorbed the override's relative path — rejoining the
/// original, still-relative string against it a second time would double it
/// (`--gitnexus-dir repo/.gitnexus` from `$HOME` deriving root `$HOME/repo`,
/// then rejoining `repo/.gitnexus` against that root, landing on
/// `$HOME/repo/repo/.gitnexus`). Pass this function's result — already
/// absolute — as the override for anything called alongside a project root
/// derived from the same override, so it's used as-is instead of rejoined.
pub fn resolve_override_for_root(
    override_dir: Option<&str>,
    default_root: &Path,
) -> Option<String> {
    let raw = override_dir?;
    let path = PathBuf::from(raw);
    let joined = if path.is_absolute() {
        path
    } else {
        default_root.join(path)
    };
    Some(
        resolve_existing_prefix(&joined)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Derive the COMPOSABLE **project root** used for freshness fingerprinting
/// and `gitnexus analyze`.
///
/// - When `override_dir` (`--gitnexus-dir` / MCP `gitnexus_dir`) is set, the
///   project root is the parent of that store path (typically the parent of
///   `.gitnexus`). Relative overrides resolve against `default_root` first.
/// - When unset, returns `default_root` (CLI cwd / MCP file root).
pub fn resolve_composable_project_root(override_dir: Option<&str>, default_root: &Path) -> PathBuf {
    let Some(resolved) = resolve_override_for_root(override_dir, default_root) else {
        return default_root.to_path_buf();
    };
    let resolved = PathBuf::from(resolved);
    resolved.parent().map(Path::to_path_buf).unwrap_or(resolved)
}

/// MCP helper: same as [`resolve_composable_project_root`], but if a
/// `gitnexus_dir` override would place the project root outside the trusted
/// file root, fall back to `file_root` so existing invalid-override checks
/// reject the store without fingerprinting an escaped tree.
pub fn resolve_mcp_composable_project_root(
    override_dir: Option<&str>,
    file_root: &Path,
) -> PathBuf {
    let derived = resolve_composable_project_root(override_dir, file_root);
    if override_dir.is_some() && !derived.starts_with(file_root) {
        file_root.to_path_buf()
    } else {
        derived
    }
}

/// Return the gitnexus dir to use, or None if not available.
///
/// Preference: explicit override > `<project_root>/.gitnexus` if it exists.
pub fn resolve_gitnexus_dir(override_dir: Option<&str>, project_root: &Path) -> Option<PathBuf> {
    if let Some(raw) = override_dir {
        let path = PathBuf::from(raw);
        let path = if path.is_absolute() {
            path
        } else {
            project_root.join(path)
        };
        let path = path.canonicalize().ok()?;
        if !path.starts_with(project_root) {
            return None;
        }
        return path.exists().then_some(path);
    }
    let default = project_root.join(".gitnexus");
    default.exists().then_some(default)
}

/// Resolved target root and status override for `topos_depgraph_status` and
/// `topos_generate_depgraph`.
pub(crate) struct DepgraphTarget {
    pub target_dir: PathBuf,
    pub status_override: Option<String>,
}

fn resolve_depgraph_directory(directory: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    match directory {
        Some(dir) => resolve_project_path(dir),
        None => {
            let root = resolve_file_root()?;
            Ok((root.clone(), root))
        }
    }
}

pub(crate) fn resolve_generate_project_root(
    directory: Option<&Path>,
    gitnexus_dir: Option<&str>,
    file_root: &Path,
) -> PathBuf {
    match directory {
        Some(dir) => dir.to_path_buf(),
        None => resolve_mcp_composable_project_root(gitnexus_dir, file_root),
    }
}

/// Choose the `gitnexus_dir` override to pass to [`depgraph_status`]
/// alongside the root [`resolve_generate_project_root`] just derived.
pub(crate) fn resolve_generate_status_override(
    directory: Option<&Path>,
    gitnexus_dir: Option<&str>,
    file_root: &Path,
) -> Option<String> {
    match directory {
        Some(_) => gitnexus_dir.map(str::to_string),
        None => resolve_override_for_root(gitnexus_dir, file_root),
    }
}

/// Validate `directory` and `gitnexus_dir` for depgraph MCP tools and
/// optionally inspect the current [`DepgraphStatus`].
pub(crate) fn resolve_depgraph_target(
    directory: Option<&str>,
    gitnexus_dir: Option<&str>,
    for_generate: bool,
    include_status: bool,
) -> Result<(DepgraphTarget, Option<DepgraphStatus>), String> {
    let (resolved_directory, detected_project) = resolve_depgraph_directory(directory)?;
    let composable_root = composable_default_root(&detected_project);
    if let Some(dir) = gitnexus_dir {
        resolve_path_within(dir, &composable_root)?;
    }
    if for_generate && !resolved_directory.is_dir() {
        return Err(format!("Not a directory: {}", resolved_directory.display()));
    }
    let dir_opt = for_generate.then_some(resolved_directory.as_path());
    let target_dir = resolve_generate_project_root(dir_opt, gitnexus_dir, &composable_root);
    let status_override = resolve_generate_status_override(dir_opt, gitnexus_dir, &composable_root);
    let status = include_status.then(|| {
        depgraph_status(
            status_override.as_deref(),
            &target_dir,
            &target_dir.to_string_lossy(),
        )
    });
    Ok((
        DepgraphTarget {
            target_dir,
            status_override,
        },
        status,
    ))
}

/// Upper bound, in bytes, on how much GitNexus child output is echoed into an
/// agent-visible warning.
const GENERATION_DETAIL_CAP: usize = 200;

/// Wrap a failed `generate_depgraph` message into the agent-visible
/// "COMPOSABLE not scored" warning, capping the child's own output.
///
/// The cap lives here rather than in `topos_engine::adapters::gitnexus`
/// because the two consumers of `DepgraphGenerationResult::message` want
/// opposite things: the CLI prints it verbatim for a human debugging a broken
/// `gitnexus analyze`, and a full Node.js stack trace is exactly what that
/// human needs. On this path the same string is interpolated into
/// `warnings[0]` and duplicated into `interpretation["mdg.unavailable"]`, so
/// those kilobytes are spent out of the agent's context window on a detail
/// the agent cannot act on — the actionable part is the wrapper text.
///
/// Truncating is safe for the marker contract that
/// `formatting::composable_contract_signals` builds `blocked_by` / `risk_flags`
/// from: truncation is monotonic, so it can only remove substring matches,
/// never invent one. It could in principle drop a marker that happened to
/// appear inside child stderr, but a *generation* failure is not a graph-state
/// signal — any marker matched out of a stack trace was incidental coupling.
/// The elision suffix is deliberately worded to contain none of the markers.
pub(super) fn generation_failure_note(message: &str) -> String {
    format!(
        "GitNexus generation failed ({}) — COMPOSABLE not scored.",
        cap_generation_detail(message)
    )
}

/// Bound `detail` to [`GENERATION_DETAIL_CAP`] bytes, appending an explicit
/// elision suffix so the reader can tell Topos cut the output rather than the
/// child emitting a stunted message.
///
/// `floor_char_boundary` backs the cut point off to the nearest `char`
/// boundary, so multi-byte UTF-8 in the child's output (accented paths, CJK
/// identifiers) can never be sliced mid-character into a panic.
pub(crate) fn cap_generation_detail(detail: &str) -> String {
    if detail.len() <= GENERATION_DETAIL_CAP {
        return detail.to_string();
    }
    let end = detail.floor_char_boundary(GENERATION_DETAIL_CAP);
    format!(
        "{} … [+{} bytes elided — re-run `topos depgraph generate` for the full output]",
        &detail[..end],
        detail.len() - end
    )
}

/// Classify a `gitnexus_dir` override that cannot be used.
///
/// Only **escapes** are invalid: an override whose resolved path lies outside
/// `project_root`. A path that is inside the root but does not exist yet is
/// **not** invalid — that is the first-run case (`depgraph_status` →
/// `missing` → [`ensure_gitnexus_dir`] generates into the store's parent).
/// Collapsing "not created yet" into `invalid_dir` previously disabled
/// auto-generation for the documented `--gitnexus-dir ~/repo/.gitnexus`
/// workflow (see #287).
pub(crate) fn check_override_warning(
    override_dir: &str,
    project_root: &Path,
) -> Option<Vec<String>> {
    let path = PathBuf::from(override_dir);
    let joined = if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    };
    // Resolve symlinks and `..` components on the existing prefix so a
    // non-existent store path with parent traversal cannot lexically appear
    // inside `project_root`.
    let resolved = resolve_existing_prefix(&joined);
    let canon_root = resolve_existing_prefix(project_root);
    if !resolved.starts_with(&canon_root) {
        return Some(vec![format!(
            "{} — override must be inside TOPOS_MCP_FILE_ROOT. Got: {}",
            INVALID_GITNEXUS_MARKERS[0],
            resolved.display()
        )]);
    }
    None
}

/// Whether a dep-graph load error is a storage/schema version mismatch.
///
/// Matched on the version wording only. A bare "ladybug" term used to match
/// here, but every native open error names LadybugDB — a dirty WAL left by an
/// interrupted `gitnexus analyze`, a locked store, or an `fts` extension whose
/// binary is ABI-incompatible with this build's `lbug` crate. Those are
/// transient and fixed by re-running `gitnexus analyze`, whereas a real schema
/// mismatch is not; collapsing them reported the wrong cause *and* routed a
/// recoverable store into the terminal branch of
/// [`ensure_gitnexus_dir_with_progress`], leaving COMPOSABLE silently dead.
pub(crate) fn is_schema_mismatch(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["storage version", "different version"]
        .iter()
        .any(|term| lower.contains(term))
}

pub(crate) fn is_branch_not_indexed(message: &str) -> bool {
    message.to_lowercase().contains(BRANCH_NOT_INDEXED_MARKER)
}

fn dep_graph_load_warning(load_error: Option<&str>) -> Vec<String> {
    match load_error {
        Some(err) if is_branch_not_indexed(err) => {
            vec![format!("COMPOSABLE not scored — {err}")]
        }
        Some(err) if is_schema_mismatch(err) => vec![format!(
            "COMPOSABLE not scored — LadybugDB storage version mismatch: {err}"
        )],
        // Keep the underlying error: the actionable part is usually in it
        // (dirty WAL, locked store, unloadable `fts` extension), and dropping
        // it left users with an unattributable "could not be loaded".
        Some(err) => vec![format!(
            "COMPOSABLE not scored — .gitnexus exists but the dependency graph could not \
             be loaded ({}); re-run 'topos depgraph generate' and ensure GitNexus \
             dependencies are installed.",
            cap_generation_detail(err)
        )],
        None => vec![
            "COMPOSABLE not scored — .gitnexus exists but the dependency graph could not \
             be loaded; re-run 'topos depgraph generate' and ensure GitNexus dependencies \
             are installed."
                .to_string(),
        ],
    }
}

/// Explain why COMPOSABLE is unavailable or risky.
pub fn gitnexus_warnings(
    override_dir: Option<&str>,
    project_root: &Path,
    gitnexus_dir: Option<&Path>,
    dep_graph_loaded: bool,
    load_error: Option<&str>,
) -> Vec<String> {
    // Escape / outside-root overrides are hard rejects — explain and stop.
    if let Some(raw) = override_dir {
        if let Some(warn) = check_override_warning(raw, project_root) {
            return warn;
        }
    }
    // Missing store applies with or without an override. An in-root override
    // that has not been generated yet is not invalid (see
    // [`check_override_warning`]); once ensure has run, a still-missing dir
    // needs the same recovery guidance as the default path.
    if gitnexus_dir.is_none() {
        return vec![
            "COMPOSABLE not scored — no .gitnexus directory found; run 'topos depgraph \
             generate' to score this generator."
                .to_string(),
        ];
    }

    let mut warnings = Vec::new();
    if let Some(dir) = gitnexus_dir {
        if !dep_graph_loaded {
            warnings.extend(dep_graph_load_warning(load_error));
        }
        if let (_, Some(detail)) = graph_freshness(project_root, dir) {
            warnings.push(detail);
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_evaluation_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_composable_project_root_uses_default_without_override() {
        let home = temp_dir("home_default");
        let root = resolve_composable_project_root(None, &home);
        assert_eq!(root, home);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn resolve_composable_project_root_derives_parent_of_gitnexus_dir() {
        // From $HOME with --gitnexus-dir <repo>/.gitnexus, fingerprint/analyze
        // must target <repo>, not $HOME.
        let home = temp_dir("home_parent");
        let repo = home.join("phil");
        let store = repo.join(".gitnexus");
        std::fs::create_dir_all(&store).unwrap();
        let root = resolve_composable_project_root(Some(store.to_str().unwrap()), &home);
        assert_eq!(root, repo.canonicalize().unwrap());
        assert_ne!(root, home.canonicalize().unwrap());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn resolve_gitnexus_dir_double_joins_a_relative_override_against_the_derived_root() {
        // Documents the hazard resolve_override_for_root exists to route
        // around: a *relative* override with a subdirectory component
        // (e.g. `repo/.gitnexus` run from $HOME) re-joined against the
        // project root resolve_composable_project_root already derived from
        // it lands on `repo/repo/.gitnexus`, not the real store. Callers
        // must use resolve_override_for_root's result (see the next test),
        // never the original raw string, once a root has been derived.
        let home = temp_dir("relative_override_double_join_home");
        let repo = home.join("repo");
        std::fs::create_dir_all(repo.join(".gitnexus")).unwrap();
        let raw_override = "repo/.gitnexus";

        let project_root = resolve_composable_project_root(Some(raw_override), &home);
        assert_eq!(
            resolve_gitnexus_dir(Some(raw_override), &project_root),
            None,
            "re-joining the raw relative override must miss the real store"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn resolve_override_for_root_avoids_the_double_join() {
        let home = temp_dir("relative_override_fixed_home");
        let repo = home.join("repo");
        let store = repo.join(".gitnexus");
        std::fs::create_dir_all(&store).unwrap();
        let raw_override = "repo/.gitnexus";

        let project_root = resolve_composable_project_root(Some(raw_override), &home);
        let resolved_override = resolve_override_for_root(Some(raw_override), &home).unwrap();
        assert_eq!(
            resolve_gitnexus_dir(Some(&resolved_override), &project_root),
            Some(store.canonicalize().unwrap()),
            "the resolved (absolute) override must find the real store"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn resolve_mcp_composable_project_root_falls_back_when_override_escapes_file_root() {
        let file_root = temp_dir("mcp_file_root");
        let outside = temp_dir("mcp_outside_repo");
        let store = outside.join(".gitnexus");
        std::fs::create_dir_all(&store).unwrap();
        let root = resolve_mcp_composable_project_root(Some(store.to_str().unwrap()), &file_root);
        assert_eq!(root, file_root);
        std::fs::remove_dir_all(&file_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[cfg(unix)]
    #[test]
    fn resolve_mcp_composable_project_root_falls_back_when_override_escapes_via_symlink() {
        // Regression: a gitnexus_dir override through a symlinked directory
        // whose store doesn't exist yet used to canonicalize-fail on the
        // whole path and fall back to lexical normalize, which never
        // follows the symlink — so `linkdir/.gitnexus` (linkdir -> outside)
        // derived a project root that lexically looked contained under
        // file_root but really resolved outside it.
        // Callers always pass an already-canonical file_root (resolve_file_root()
        // canonicalizes), so canonicalize it here too — on macOS /tmp itself is
        // a symlink (/var -> /private/var), and comparing a raw temp_dir()
        // path against a canonicalized derived path would spuriously "escape"
        // on that alone, masking the real symlink-escape behavior under test.
        let file_root = temp_dir("mcp_symlink_file_root").canonicalize().unwrap();
        let outside = temp_dir("mcp_symlink_outside_secret")
            .canonicalize()
            .unwrap();
        let link = file_root.join("linkdir");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let override_dir = link.join(".gitnexus");
        let root =
            resolve_mcp_composable_project_root(Some(override_dir.to_str().unwrap()), &file_root);
        assert_eq!(root, file_root);
        std::fs::remove_dir_all(&file_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn in_root_missing_override_classifies_as_missing_not_invalid() {
        // #287: `--gitnexus-dir <repo>/.gitnexus` before the store exists must
        // not be `invalid_dir` — that short-circuits ensure and silently
        // disables COMPOSABLE. In-root absence is the first-run `missing` case.
        let home = temp_dir("first_run_home");
        let repo = home.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = repo.join(".gitnexus");
        assert!(!store.exists());

        let project_root = resolve_composable_project_root(Some(store.to_str().unwrap()), &home);
        let resolved = resolve_override_for_root(Some(store.to_str().unwrap()), &home).unwrap();
        assert!(
            check_override_warning(&resolved, &project_root).is_none(),
            "in-root missing store is not an invalid override"
        );

        let status = depgraph_status(Some(&resolved), &project_root, &repo.to_string_lossy());
        assert_eq!(status.state, "missing");

        // gitnexus_warnings must not emit invalid_dir markers for this path.
        let warns = gitnexus_warnings(Some(&resolved), &project_root, None, false, None);
        assert!(
            warns
                .iter()
                .all(|w| !INVALID_GITNEXUS_MARKERS.iter().any(|m| w.contains(m))),
            "{warns:?}"
        );
        assert!(
            warns
                .iter()
                .any(|w| w.contains("no .gitnexus directory found")),
            "{warns:?}"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn resolve_generate_project_root_derives_from_gitnexus_dir_like_status() {
        let file_root = temp_dir("generate_root").canonicalize().unwrap();
        let nested = file_root.join("nested");
        std::fs::create_dir_all(nested.join(".gitnexus")).unwrap();
        let store = nested.join(".gitnexus");

        let derived =
            resolve_generate_project_root(None, Some(store.to_str().unwrap()), &file_root);
        assert_eq!(derived, nested.canonicalize().unwrap());

        let explicit = resolve_generate_project_root(
            Some(file_root.as_path()),
            Some(store.to_str().unwrap()),
            &file_root,
        );
        assert_eq!(explicit, file_root);

        std::fs::remove_dir_all(&file_root).ok();
    }

    #[test]
    fn resolve_generate_status_override_matches_the_root_it_pairs_with() {
        let file_root = temp_dir("generate_status_override").canonicalize().unwrap();
        std::fs::create_dir_all(file_root.join("repo/.gitnexus")).unwrap();
        let raw_override = "repo/.gitnexus";

        let derived_override =
            resolve_generate_status_override(None, Some(raw_override), &file_root);
        assert_eq!(
            derived_override.as_deref(),
            file_root.join("repo/.gitnexus").to_str(),
            "must resolve to the real store, not double the subdirectory"
        );

        let explicit_override = resolve_generate_status_override(
            Some(file_root.as_path()),
            Some(raw_override),
            &file_root,
        );
        assert_eq!(explicit_override.as_deref(), Some(raw_override));

        std::fs::remove_dir_all(&file_root).ok();
    }

    /// Marker the cap appends; tests split on it instead of re-spelling the
    /// whole suffix, so rewording the suffix doesn't churn every assertion.
    const ELISION: &str = " … [+";

    /// Pull the child-output detail back out of a wrapped note.
    fn detail_of(note: &str) -> &str {
        note.strip_prefix("GitNexus generation failed (")
            .and_then(|rest| rest.strip_suffix(") — COMPOSABLE not scored."))
            .expect("note must keep the wrapper text")
    }

    #[test]
    fn generation_failure_note_keeps_wrapper_text_around_short_details() {
        let note = generation_failure_note("gitnexus analyze failed.");
        assert_eq!(
            note,
            "GitNexus generation failed (gitnexus analyze failed.) — COMPOSABLE not scored."
        );
        // Short details are passed through untouched — no elision noise.
        assert!(!note.contains(ELISION));
    }

    #[test]
    fn generation_failure_note_caps_multi_kilobyte_child_stderr() {
        // A Node.js stack trace out of `gitnexus analyze` is kilobytes; all of
        // it used to land in warnings[0] and interpretation["mdg.unavailable"].
        let stderr = format!(
            "Error: Cannot find module 'tree-sitter'\n{}",
            "    at Module._resolveFilename (node:internal/modules/cjs/loader:1145:15)\n"
                .repeat(64)
        );
        assert!(stderr.len() > 4096, "fixture must be multi-kilobyte");

        let note = generation_failure_note(&stderr);
        let detail = detail_of(&note);
        let (kept, _) = detail.split_once(ELISION).expect("cap must mark elision");
        assert!(
            kept.len() <= GENERATION_DETAIL_CAP,
            "kept {} bytes, cap is {GENERATION_DETAIL_CAP}",
            kept.len()
        );
        // The head of the trace — the part that actually names the failure —
        // survives.
        assert!(kept.starts_with("Error: Cannot find module 'tree-sitter'"));
        assert!(note.len() < 400, "note still {} bytes", note.len());
    }

    #[test]
    fn cap_generation_detail_truncates_multibyte_payloads_on_a_char_boundary() {
        // '日' is 3 bytes, so the cap (200) lands mid-character at 200 and
        // floor_char_boundary must back off to 198 rather than panic.
        let payload = "日".repeat(400);
        let capped = cap_generation_detail(&payload);
        let (kept, _) = capped.split_once(ELISION).expect("cap must mark elision");
        assert_eq!(kept.len(), 198);
        assert_eq!(kept.chars().count(), 66);
        assert!(kept.chars().all(|c| c == '日'));

        // Every cap offset within a multi-byte run must be boundary-safe.
        for len in 1..=400usize {
            let _ = cap_generation_detail(&"é".repeat(len));
            let _ = cap_generation_detail(&"🜁".repeat(len));
        }
    }

    #[test]
    fn capped_notes_never_synthesize_contract_markers() {
        // The elision suffix is agent-visible prose sitting in warnings[0];
        // if it ever drifted into containing a marker, every truncated note
        // would fabricate a blocked_by code.
        let note = generation_failure_note(&"x".repeat(4096));
        let lower = note.to_lowercase();
        for marker in INVALID_GITNEXUS_MARKERS {
            assert!(!lower.contains(marker), "elision suffix leaked {marker}");
        }
        assert!(!lower.contains(BRANCH_NOT_INDEXED_MARKER));
        assert!(!lower.contains(STALE_GITNEXUS_MARKER));

        let signals = crate::formatting::composable_contract_signals(false, &[note], false);
        assert!(signals.blocked_by.is_empty(), "{:?}", signals.blocked_by);
    }

    #[test]
    fn marker_warnings_still_drive_composable_contract_signals() {
        // The cap must not disturb the substring contract between these
        // producers and formatting::composable_contract_signals.
        let project_root = temp_dir("marker_root");
        let outside = temp_dir("marker_outside");
        let invalid = check_override_warning(&outside.to_string_lossy(), &project_root)
            .expect("override outside the root must warn");
        assert!(
            crate::formatting::composable_contract_signals(false, &invalid, false)
                .blocked_by
                .contains(&"invalid_gitnexus_dir".to_string())
        );

        let branch = dep_graph_load_warning(Some(
            "no gitnexus store indexed for branch 'feature/x'; run gitnexus analyze",
        ));
        assert!(
            crate::formatting::composable_contract_signals(false, &branch, false)
                .blocked_by
                .contains(&"branch_not_indexed_gitnexus_dir".to_string())
        );

        // Staleness needs a real .gitnexus store to produce organically, so
        // mirror the shape freshness.rs emits.
        let stale = vec![format!(
            "{STALE_GITNEXUS_MARKER} — source tree content changed since the graph was built"
        )];
        assert!(
            crate::formatting::composable_contract_signals(true, &stale, false)
                .blocked_by
                .contains(&"stale_gitnexus_dir".to_string())
        );

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn dep_graph_load_warning_is_not_capped() {
        // Deliberately uncapped: BRANCH_NOT_INDEXED_MARKER can sit anywhere in
        // this passthrough, and truncating it would silently drop a
        // blocked_by code.
        let err = format!(
            "no gitnexus store indexed for branch 'feature/x' {}",
            "(candidate store rejected) ".repeat(64)
        );
        let warnings = dep_graph_load_warning(Some(&err));
        assert!(warnings[0].contains(&err));
        assert!(warnings[0].len() > GENERATION_DETAIL_CAP * 5);
    }

    /// Only a real version mismatch may claim `schema_mismatch` — that is the
    /// one dep-graph state `ensure_gitnexus_dir` refuses to regenerate, so a
    /// transient native-open failure landing there leaves COMPOSABLE dead
    /// until the user re-runs GitNexus by hand.
    #[test]
    fn transient_ladybug_open_failures_are_not_schema_mismatches() {
        let extension_abi = "read_only open failed (IO exception: Failed to load library: \
             ~/.lbdb/extension/0.17.0/osx_arm64/fts/libfts.lbug_extension which is needed \
             by extension: fts. Error: dlopen(...): symbol not found in flat namespace)";
        assert!(!is_schema_mismatch(extension_abi));
        assert!(!is_schema_mismatch(
            "LadybugDB: could not acquire lock on the database"
        ));

        assert!(is_schema_mismatch(
            "Database file was created with a different version of LadybugDB"
        ));
        assert!(is_schema_mismatch(
            "storage version 42 is newer than this build supports"
        ));
    }

    /// The load warning must carry the underlying error: "could not be
    /// loaded" alone is unattributable, and the cause (dirty WAL, locked
    /// store, unloadable extension) is what tells the user what to do.
    #[test]
    fn generic_load_warning_keeps_the_underlying_error() {
        let warnings = dep_graph_load_warning(Some("dlopen failed for extension: fts"));
        assert!(warnings[0].contains("dlopen failed for extension: fts"));
    }

    #[test]
    fn check_override_warning_rejects_nonexistent_parent_traversal_escape() {
        let project_root = temp_dir("parent_traversal_root");
        let warn = check_override_warning("../outside_nonexistent/.gitnexus", &project_root);
        assert!(
            warn.is_some(),
            "parent-traversal override escaping project_root must be rejected even when missing"
        );
        std::fs::remove_dir_all(&project_root).ok();
    }
}
