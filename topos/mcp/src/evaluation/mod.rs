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

mod classify;
mod depgraph;
mod freshness;
mod gitref;

pub use classify::{classify_code_string, classify_file, classify_morphism, detect_language};
pub use depgraph::{clear_caches, depgraph_status, load_dep_graph, DepgraphStatus};
pub use freshness::{all_source_suffixes, graph_freshness};
pub use gitref::{git_head_mtime, git_head_sha, gitnexus_mtime};

use std::path::{Path, PathBuf};

use topos_engine::adapters::gitnexus::{generate_depgraph, gitnexus_available};

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

/// Outcome of [`ensure_gitnexus_dir`]: the dir to attach (if any), plus a
/// note describing a generation attempt — `Some` only when generation was
/// attempted and didn't result in a usable graph (not found on `$PATH`, or
/// the `gitnexus analyze` run itself failed). `None` in every other case
/// (already present/fresh, skipped, or generation succeeded), since
/// [`gitnexus_warnings`] already explains any remaining unavailability
/// (invalid override, schema mismatch, branch not indexed, ...) from the
/// resulting `gitnexus_dir` state.
pub struct GitnexusEnsureOutcome {
    pub gitnexus_dir: Option<PathBuf>,
    pub generation_note: Option<String>,
}

/// Resolve the gitnexus dir to attach for COMPOSABLE, generating/refreshing
/// it first when missing or stale — the shared "ensure" decision behind
/// both the CLI's default `topos evaluate` and the MCP evaluate tools'
/// default behavior, so the two standardize on one "always try to score all
/// three pillars" policy. `skip=true` reproduces the old read-only behavior
/// (just [`resolve_gitnexus_dir`], no generation). `capture` is forwarded to
/// `generate_depgraph`: `false` streams GitNexus's own output to the
/// inherited stdio (the CLI, where a human is watching), `true` collects it
/// into the result instead (MCP, over a stdio transport already carrying
/// the protocol).
///
/// Never blocks indefinitely: `generate_depgraph` bounds the `gitnexus
/// analyze` subprocess with `TOPOS_DEPGRAPH_TIMEOUT` (default 300s).
/// Callers running this synchronously on an async runtime should offload it
/// (e.g. `tokio::task::spawn_blocking`) so a slow/first-time generation on a
/// large repo cannot stall the transport.
pub fn ensure_gitnexus_dir(
    override_dir: Option<&str>,
    project_root: &Path,
    skip: bool,
    capture: bool,
) -> GitnexusEnsureOutcome {
    let resolve = || resolve_gitnexus_dir(override_dir, project_root);
    if skip {
        return GitnexusEnsureOutcome {
            gitnexus_dir: resolve(),
            generation_note: None,
        };
    }

    let status = depgraph_status(override_dir, project_root, &project_root.to_string_lossy());
    if !matches!(status.state, "missing" | "stale") {
        // present, or a problem generating won't fix (invalid_dir,
        // schema_mismatch, branch_not_indexed, load_error) — let
        // gitnexus_warnings explain it from the resolved state.
        return GitnexusEnsureOutcome {
            gitnexus_dir: resolve(),
            generation_note: None,
        };
    }

    if !gitnexus_available() {
        return GitnexusEnsureOutcome {
            gitnexus_dir: resolve(),
            generation_note: Some(
                "GitNexus not found on $PATH — COMPOSABLE not scored. Install it with \
                 `npm install -g gitnexus` to enable COMPOSABLE."
                    .to_string(),
            ),
        };
    }

    let result = generate_depgraph(project_root, capture, None);
    let generation_note = (!result.ok).then(|| generation_failure_note(&result.message));
    GitnexusEnsureOutcome {
        gitnexus_dir: resolve(),
        generation_note,
    }
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
fn generation_failure_note(message: &str) -> String {
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

/// Return the graphify output dir to use, or None if not available.
///
/// Preference: explicit override > Graphify's own default resolution
/// (`topos_engine::adapters::graphify::graphify_out_dir`, which itself honors
/// `GRAPHIFY_OUT`) — so the read side (this function) and the generate side
/// never disagree about where to look.
pub fn resolve_graphify_dir(override_dir: Option<&str>, project_root: &Path) -> Option<PathBuf> {
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
    let default = topos_engine::adapters::graphify::graphify_out_dir(project_root);
    default.exists().then_some(default)
}

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
    let resolved = joined.canonicalize().unwrap_or(joined);
    if !resolved.starts_with(project_root) {
        return Some(vec![format!(
            "{} — override must be inside TOPOS_MCP_FILE_ROOT. Got: {}",
            INVALID_GITNEXUS_MARKERS[0],
            resolved.display()
        )]);
    }
    if !resolved.exists() {
        return Some(vec![format!(
            "{} — override path does not exist. Got: {}",
            INVALID_GITNEXUS_MARKERS[1],
            resolved.display()
        )]);
    }
    None
}

/// Whether a dep-graph load error is a storage/schema version mismatch.
pub(crate) fn is_schema_mismatch(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["storage version", "different version", "ladybug"]
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
        _ => vec![
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
    if let Some(raw) = override_dir {
        if let Some(warn) = check_override_warning(raw, project_root) {
            return warn;
        }
    } else if gitnexus_dir.is_none() {
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
    use topos_engine::adapters::gitnexus::gitnexus_available;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_evaluation_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_gitnexus_dir_skip_reproduces_plain_resolve_without_shelling_out() {
        // skip=true must behave exactly like the old read-only
        // resolve_gitnexus_dir — no depgraph_status/gitnexus_available/
        // generate_depgraph call at all, so this is deterministic
        // regardless of whether gitnexus happens to be on the test
        // machine's PATH.
        let project_root = temp_dir("skip_root");
        let outcome = ensure_gitnexus_dir(None, &project_root, true, false);
        assert!(outcome.gitnexus_dir.is_none());
        assert!(outcome.generation_note.is_none());
        std::fs::remove_dir_all(&project_root).ok();
    }

    #[test]
    fn ensure_gitnexus_dir_returns_none_for_override_outside_project_root_without_shelling_out() {
        // An override outside project_root is rejected by
        // resolve_gitnexus_dir/depgraph_status before any availability
        // check or subprocess call — deterministic either way.
        let project_root = temp_dir("invalid_root");
        let outside = temp_dir("invalid_outside");

        let outcome = ensure_gitnexus_dir(
            Some(&outside.to_string_lossy()),
            &project_root,
            false,
            false,
        );
        assert!(outcome.gitnexus_dir.is_none());
        // invalid_dir is a state generation can't fix, so no generation
        // is attempted and no generation_note is set — gitnexus_warnings
        // (fed by depgraph_status separately) is what explains it.
        assert!(outcome.generation_note.is_none());

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn ensure_gitnexus_dir_degrades_gracefully_when_gitnexus_missing_from_path() {
        if gitnexus_available() {
            // Dev boxes may have GitNexus installed; skip rather than
            // shell out to the real binary from a unit test.
            return;
        }
        let project_root = temp_dir("missing_no_gitnexus");
        let outcome = ensure_gitnexus_dir(None, &project_root, false, true);
        assert!(outcome.gitnexus_dir.is_none());
        assert!(outcome
            .generation_note
            .as_deref()
            .is_some_and(|n| n.contains("not found on $PATH")));
        std::fs::remove_dir_all(&project_root).ok();
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
}
