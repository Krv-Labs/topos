//! Function-level move ledger — what actually happened to the logic when
//! a change moved code between files.
//!
//! A diff answers "which lines changed". When one parent file is split
//! into N children, a line diff says *everything* was deleted and
//! *everything* was added, which tells an agent nothing about whether
//! behaviour moved verbatim, moved with an edit, or was genuinely new.
//! The ledger answers the question a reviewer actually has: for each
//! callable, did it stay, move unchanged, move modified, get renamed,
//! appear, or disappear — and does the complexity arithmetic close.
//!
//! Closing the arithmetic is the point. [`LedgerTotals::balanced`] is
//! computed, never assumed: the total complexity delta across the change
//! must equal the sum of the per-bucket deltas. A split that claims "pure
//! move" but fails the balance check has smuggled new logic in, and the
//! ledger names exactly which entries carry it.
//!
//! Everything here is deterministic. Snapshots come from the shared
//! [`crate::functors::probes::ast::scopes`] walk (same callables as the
//! `ast.max_function_complexity` gate), all iteration is over pools
//! sorted by `(file, start_line)`, and the structural hash is BLAKE2b —
//! not [`std::collections::hash_map::DefaultHasher`], whose output is not
//! stable across Rust releases.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use serde::Serialize;

use crate::functors::probes::ast::complexity::calculate_function_complexity_entries;
use crate::functors::probes::ast::scopes::{function_scopes, ANONYMOUS_NAME};
use crate::functors::probes::uast::signature::uast_dfs_kind_sequence;
use crate::functors::profunctors::uast::compare::uast_edit_distance;
use crate::graphs::uast::models::{AttributeValue, UASTNode};

/// Minimum similarity for the residue pass to call two callables the
/// same logic moved or renamed.
pub const MOVED_MODIFIED_MIN_SIMILARITY: f64 = 0.8;

/// Minimum similarity for a *name*-driven cross-file match. Lower than
/// [`MOVED_MODIFIED_MIN_SIMILARITY`] because a surviving qualified name
/// is itself evidence; the threshold only rejects name collisions
/// between unrelated bodies.
pub const NAME_MATCH_MIN_SIMILARITY: f64 = 0.5;

/// Callables with fewer UAST nodes than this are "trivial" — a one-line
/// `return`, an empty body. Structurally they match nearly anything, so
/// they are excluded from similarity-driven pairing and may only be
/// hash-paired when their names agree too (see [`match_functions`]).
const TRIVIAL_MIN_NODES: usize = 3;

/// `Unknown`-kind nodes are excluded from every sequence in this module
/// so that "same hash" and "zero edit distance" agree by construction.
const INCLUDE_UNKNOWN: bool = false;

/// One callable as it exists in one version of one file.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionSnapshot {
    pub file: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub complexity: usize,
    /// A closure declared inside another callable. Its complexity is
    /// already counted inside its enclosing function's subtree, so it is
    /// excluded from the totals to avoid double counting — but it still
    /// appears in [`Ledger::matches`], because "the retry closure moved
    /// to the other file" is exactly the kind of thing a reviewer wants.
    pub nested: bool,
    /// Body fingerprint: identical bodies produce identical hashes.
    ///
    /// Covers the DFS `kind` sequence, the text of every identifier, and
    /// every `operator` attribute. Operators matter because complexity
    /// counts short-circuit `&&`/`||` from the attribute alone — without
    /// it, `a && b` and `a + b` would hash alike while differing in
    /// complexity, and a "moved identical" pair would silently unbalance
    /// the ledger.
    pub structural_hash: u64,
    /// Owned subtree, kept for the similarity passes.
    #[serde(skip)]
    pub node: UASTNode,
}

/// What happened to one callable across the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// Same file, same identity — may still have been edited in place.
    InPlace,
    /// Different file, byte-for-byte identical body.
    MovedIdentical,
    /// Different file, body edited on the way.
    MovedModified,
    /// Same body, different name.
    Renamed,
    /// Present only after the change.
    New,
    /// Present only before the change.
    Removed,
}

/// One row of the ledger. Exactly one of `before`/`after` is `None` for
/// [`MatchKind::New`] / [`MatchKind::Removed`]; both are `Some` otherwise.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionMatch {
    pub kind: MatchKind,
    pub before: Option<FunctionSnapshot>,
    pub after: Option<FunctionSnapshot>,
    pub similarity: f64,
    pub complexity_delta: i64,
}

/// Complexity arithmetic across the whole change, over non-nested
/// snapshots only.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LedgerTotals {
    pub before_total: usize,
    pub after_total: usize,
    pub in_place_delta: i64,
    /// Deltas for everything that crossed a file or changed name:
    /// [`MatchKind::MovedModified`], [`MatchKind::Renamed`], and
    /// [`MatchKind::MovedIdentical`] (whose delta is zero in practice,
    /// but is folded in rather than assumed so the balance check can
    /// never be satisfied by an assumption).
    pub moved_modified_delta: i64,
    pub new_logic: usize,
    pub removed: usize,
    pub moved_identical: usize,
    pub renamed: usize,
    /// `after_total - before_total == in_place_delta +
    /// moved_modified_delta + new_logic - removed`.
    pub balanced: bool,
}

/// The full ledger: one row per callable on either side, plus totals.
#[derive(Debug, Clone, Serialize)]
pub struct Ledger {
    pub matches: Vec<FunctionMatch>,
    pub totals: LedgerTotals,
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/// Every identifier's source text, in DFS pre-order, whitespace-trimmed.
///
/// UAST nodes don't carry token text, so it has to be sliced from
/// `source` by span — same technique as
/// [`crate::functors::probes::ast::scopes`]. JavaScript method names are
/// `property_identifier` natively with no UAST kind of their own, so that
/// native kind is accepted too.
fn identifier_texts(node: &UASTNode, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind == "Identifier" || current.native.node_kind == "property_identifier" {
            if let Some(text) = source.get(current.span.start_byte..current.span.end_byte) {
                out.push(text.trim().to_string());
            }
        }
        if let Some(AttributeValue::Str(op)) = current.attributes.get("operator") {
            out.push(format!("op:{op}"));
        }
        stack.extend(current.children.iter().rev());
    }
    out
}

fn structural_hash(node: &UASTNode, source: &str) -> u64 {
    let mut hasher = Blake2bVar::new(8).expect("8 is a valid BLAKE2b-var digest size");
    for kind in uast_dfs_kind_sequence(node, INCLUDE_UNKNOWN) {
        hasher.update(kind.as_bytes());
        hasher.update(b"\x1f");
    }
    hasher.update(b"\x1e");
    for text in identifier_texts(node, source) {
        hasher.update(text.as_bytes());
        hasher.update(b"\x1f");
    }
    let mut digest = [0u8; 8];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest buffer matches the requested output size");
    u64::from_be_bytes(digest)
}

/// Snapshot every callable in one file, sorted by `start_line`.
///
/// Complexity comes from
/// [`calculate_function_complexity_entries`], which is itself a map over
/// [`function_scopes`] — so the two vectors are index-aligned by
/// construction, not by coincidence. The `debug_assert` below is the net
/// in case that implementation is ever reordered.
pub fn snapshot_functions(uast_root: &UASTNode, source: &str, file: &str) -> Vec<FunctionSnapshot> {
    let scopes = function_scopes(uast_root, source);
    let entries = calculate_function_complexity_entries(uast_root, source);
    debug_assert_eq!(
        scopes.len(),
        entries.len(),
        "scopes and complexity entries come from the same walk"
    );

    let mut snapshots: Vec<FunctionSnapshot> = scopes
        .into_iter()
        .zip(entries)
        .map(|(scope, entry)| {
            debug_assert_eq!(
                (scope.qualified_name.as_str(), scope.start_line),
                (entry.qualified_name.as_str(), entry.start_line),
                "complexity entry must describe the same callable as its scope"
            );
            FunctionSnapshot {
                file: file.to_string(),
                name: scope.name.clone(),
                qualified_name: scope.qualified_name.clone(),
                kind: scope.kind.to_string(),
                start_line: scope.start_line,
                end_line: scope.end_line,
                complexity: entry.complexity,
                nested: scope.kind == "closure",
                structural_hash: structural_hash(scope.node, source),
                node: scope.node.clone(),
            }
        })
        .collect();

    snapshots.sort_by(|a, b| a.start_line.cmp(&b.start_line).then(a.name.cmp(&b.name)));
    snapshots
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

fn sort_pool(pool: &mut [FunctionSnapshot]) {
    pool.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.start_line.cmp(&b.start_line))
            .then(a.qualified_name.cmp(&b.qualified_name))
    });
}

fn node_size(snapshot: &FunctionSnapshot) -> usize {
    uast_dfs_kind_sequence(&snapshot.node, INCLUDE_UNKNOWN).len()
}

fn is_trivial(snapshot: &FunctionSnapshot) -> bool {
    node_size(snapshot) < TRIVIAL_MIN_NODES
}

/// Synthetic labels for callables the grammar never named. Two unrelated
/// arrow functions both called `<anonymous>@7` are not the same function,
/// so name-driven matching must never fire on them.
fn is_anonymous(name: &str) -> bool {
    name.starts_with(ANONYMOUS_NAME)
}

fn similarity(before: &FunctionSnapshot, after: &FunctionSnapshot) -> f64 {
    if before.structural_hash == after.structural_hash {
        return 1.0;
    }
    1.0 - uast_edit_distance(&before.node, &after.node, INCLUDE_UNKNOWN).normalized_distance
}

/// The trailing segment of a qualified name — `Cache.get` and
/// `Store.get` differ only by the enclosing chain.
fn leaf_name(snapshot: &FunctionSnapshot) -> &str {
    &snapshot.name
}

/// Build the ledger from two snapshot pools.
///
/// The ladder runs cheapest-evidence-first, and each rung consumes the
/// entries it matches, so later rungs only ever see genuine residue:
///
/// 0. identical [`FunctionSnapshot::structural_hash`] → `InPlace` (same
///    file) or `MovedIdentical`. Trivial bodies (see
///    [`TRIVIAL_MIN_NODES`]) additionally require equal names: an empty
///    body is not evidence of identity on its own, and anonymous
///    callables can share a hash without sharing an origin.
/// 1. same `qualified_name` in the same file → `InPlace`.
/// 2. same `qualified_name`, or same leaf `name`, across files →
///    `MovedModified` when similarity clears
///    [`NAME_MATCH_MIN_SIMILARITY`].
/// 3. residue, scored pairwise and taken greedily in descending
///    similarity above [`MOVED_MODIFIED_MIN_SIMILARITY`] → `Renamed` (or
///    `MovedModified` if the names happen to agree). Trivial bodies are
///    skipped here entirely: they match everything.
/// 4. whatever is left → `Removed` / `New`.
pub fn match_functions(before: Vec<FunctionSnapshot>, after: Vec<FunctionSnapshot>) -> Ledger {
    let mut before = before;
    let mut after = after;
    sort_pool(&mut before);
    sort_pool(&mut after);

    let mut before_taken = vec![false; before.len()];
    let mut after_taken = vec![false; after.len()];
    // (before index, after index, kind, similarity)
    let mut pairs: Vec<(usize, usize, MatchKind, f64)> = Vec::new();

    // --- 0. identical bodies -------------------------------------------------
    for (bi, b) in before.iter().enumerate() {
        if before_taken[bi] {
            continue;
        }
        let trivial = is_trivial(b);
        let found = after.iter().enumerate().find(|(ai, a)| {
            !after_taken[*ai]
                && a.structural_hash == b.structural_hash
                && (!trivial || a.name == b.name)
        });
        if let Some((ai, a)) = found {
            let kind = if a.file == b.file {
                MatchKind::InPlace
            } else {
                MatchKind::MovedIdentical
            };
            before_taken[bi] = true;
            after_taken[ai] = true;
            pairs.push((bi, ai, kind, 1.0));
        }
    }

    // --- 1. same qualified name, same file -----------------------------------
    for (bi, b) in before.iter().enumerate() {
        if before_taken[bi] {
            continue;
        }
        let found = after.iter().enumerate().find(|(ai, a)| {
            !after_taken[*ai] && a.file == b.file && a.qualified_name == b.qualified_name
        });
        if let Some((ai, a)) = found {
            before_taken[bi] = true;
            after_taken[ai] = true;
            let score = similarity(b, a);
            pairs.push((bi, ai, MatchKind::InPlace, score));
        }
    }

    // --- 2. same name across files -------------------------------------------
    for (bi, b) in before.iter().enumerate() {
        if before_taken[bi] || is_anonymous(&b.name) {
            continue;
        }
        // Best candidate rather than the first, so a shared leaf name
        // (`Cache.get` vs `Store.get`) can't shadow an exact
        // qualified-name survivor later in the pool.
        let best = after
            .iter()
            .enumerate()
            .filter(|(ai, a)| {
                !after_taken[*ai]
                    && a.file != b.file
                    && !is_anonymous(&a.name)
                    && (a.qualified_name == b.qualified_name || leaf_name(a) == leaf_name(b))
            })
            .map(|(ai, a)| (ai, similarity(b, a)))
            .max_by(|(ai, sa), (bi2, sb)| {
                sa.total_cmp(sb).then(bi2.cmp(ai)) // ties → lower index wins
            });
        if let Some((ai, score)) = best {
            if score >= NAME_MATCH_MIN_SIMILARITY {
                before_taken[bi] = true;
                after_taken[ai] = true;
                pairs.push((bi, ai, MatchKind::MovedModified, score));
            }
        }
    }

    // --- 3. residue, by similarity -------------------------------------------
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (bi, b) in before.iter().enumerate() {
        if before_taken[bi] || is_trivial(b) {
            continue;
        }
        for (ai, a) in after.iter().enumerate() {
            if after_taken[ai] || is_trivial(a) {
                continue;
            }
            let score = similarity(b, a);
            if score >= MOVED_MODIFIED_MIN_SIMILARITY {
                candidates.push((score, bi, ai));
            }
        }
    }
    candidates.sort_by(|(sa, bia, aia), (sb, bib, aib)| {
        sb.total_cmp(sa).then_with(|| {
            let (ba, aa) = (&before[*bia], &after[*aia]);
            let (bb, ab) = (&before[*bib], &after[*aib]);
            (&ba.file, &ba.name, &aa.file, &aa.name).cmp(&(&bb.file, &bb.name, &ab.file, &ab.name))
        })
    });
    for (score, bi, ai) in candidates {
        if before_taken[bi] || after_taken[ai] {
            continue;
        }
        let b = &before[bi];
        let a = &after[ai];
        let kind = if b.name == a.name {
            // Unreachable after rung 2 for named callables; kept so an
            // anonymous-to-anonymous pair still reports as a move, not a
            // rename to the same name.
            MatchKind::MovedModified
        } else if is_anonymous(&b.name) && is_anonymous(&a.name) {
            // Both sides are synthetic `<anonymous>@<line>` labels, so the
            // name difference is an artifact of line movement, not
            // evidence of a rename — two anonymous callbacks on different
            // lines always "differ" in name.
            if b.file == a.file {
                MatchKind::InPlace
            } else {
                MatchKind::MovedModified
            }
        } else {
            MatchKind::Renamed
        };
        before_taken[bi] = true;
        after_taken[ai] = true;
        pairs.push((bi, ai, kind, score));
    }

    // --- assemble ------------------------------------------------------------
    let mut matches: Vec<FunctionMatch> = pairs
        .into_iter()
        .map(|(bi, ai, kind, score)| FunctionMatch {
            kind,
            complexity_delta: after[ai].complexity as i64 - before[bi].complexity as i64,
            before: Some(before[bi].clone()),
            after: Some(after[ai].clone()),
            similarity: score,
        })
        .collect();
    for (bi, b) in before.iter().enumerate() {
        if !before_taken[bi] {
            matches.push(FunctionMatch {
                kind: MatchKind::Removed,
                complexity_delta: -(b.complexity as i64),
                before: Some(b.clone()),
                after: None,
                similarity: 0.0,
            });
        }
    }
    for (ai, a) in after.iter().enumerate() {
        if !after_taken[ai] {
            matches.push(FunctionMatch {
                kind: MatchKind::New,
                complexity_delta: a.complexity as i64,
                before: None,
                after: Some(a.clone()),
                similarity: 0.0,
            });
        }
    }
    matches.sort_by(|x, y| {
        let key = |m: &FunctionMatch| {
            let s = m
                .before
                .as_ref()
                .or(m.after.as_ref())
                .expect("every match has a side");
            (s.file.clone(), s.start_line, s.qualified_name.clone())
        };
        key(x).cmp(&key(y))
    });

    let totals = totals_for(&before, &after, &matches);
    debug_assert!(totals.balanced, "ledger complexity must close: {totals:?}");
    Ledger { matches, totals }
}

/// Complexity a snapshot contributes to the totals: nested closures
/// contribute nothing, because their decision points are already counted
/// inside their enclosing function's subtree.
fn counted(snapshot: Option<&FunctionSnapshot>) -> i64 {
    match snapshot {
        Some(s) if !s.nested => s.complexity as i64,
        _ => 0,
    }
}

fn totals_for(
    before: &[FunctionSnapshot],
    after: &[FunctionSnapshot],
    matches: &[FunctionMatch],
) -> LedgerTotals {
    let mut totals = LedgerTotals {
        before_total: before
            .iter()
            .filter(|s| !s.nested)
            .map(|s| s.complexity)
            .sum(),
        after_total: after
            .iter()
            .filter(|s| !s.nested)
            .map(|s| s.complexity)
            .sum(),
        ..LedgerTotals::default()
    };

    for m in matches {
        // Masked by `counted`, not `complexity_delta`, so a pair that
        // crosses the nested boundary can't unbalance the ledger.
        let delta = counted(m.after.as_ref()) - counted(m.before.as_ref());
        match m.kind {
            MatchKind::InPlace => totals.in_place_delta += delta,
            MatchKind::MovedIdentical => {
                totals.moved_identical += 1;
                totals.moved_modified_delta += delta;
            }
            MatchKind::Renamed => {
                totals.renamed += 1;
                totals.moved_modified_delta += delta;
            }
            MatchKind::MovedModified => totals.moved_modified_delta += delta,
            MatchKind::New => totals.new_logic += delta.unsigned_abs() as usize,
            MatchKind::Removed => totals.removed += (-delta).unsigned_abs() as usize,
        }
    }

    totals.balanced = totals.after_total as i64 - totals.before_total as i64
        == totals.in_place_delta + totals.moved_modified_delta + totals.new_logic as i64
            - totals.removed as i64;
    totals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn snap(source: &str, file: &str) -> Vec<FunctionSnapshot> {
        let language = if file.ends_with(".py") {
            "python"
        } else {
            "typescript"
        };
        let parsed = parse_source(source, language, Some(file)).expect("fixture must parse");
        snapshot_functions(&parsed.uast_root, source, file)
    }

    fn find<'a>(ledger: &'a Ledger, name: &str) -> &'a FunctionMatch {
        ledger
            .matches
            .iter()
            .find(|m| {
                m.before.as_ref().is_some_and(|s| s.name == name)
                    || m.after.as_ref().is_some_and(|s| s.name == name)
            })
            .unwrap_or_else(|| panic!("no ledger row for {name}: {:?}", kinds(ledger)))
    }

    fn kinds(ledger: &Ledger) -> Vec<(String, MatchKind)> {
        ledger
            .matches
            .iter()
            .map(|m| {
                let s = m.before.as_ref().or(m.after.as_ref()).unwrap();
                (s.name.clone(), m.kind)
            })
            .collect()
    }

    const BASE: &str = "function a(x){ if (x) { return 1 } return 0 }\n\
                        function b(y){ for (const i of y) { log(i) } return y }\n\
                        function c(z){ if (z) { return z } return null }\n";

    #[test]
    fn split_reports_in_place_moved_identical_and_moved_modified() {
        let head_base = "function a(x){ if (x) { return 1 } return 0 }\n";
        let head_other = "function b(y){ for (const i of y) { log(i) } return y }\n\
                          function c(z){ if (z) { return z } if (z) { return z } return null }\n";

        let before = snap(BASE, "base.ts");
        let mut after = snap(head_base, "base.ts");
        after.extend(snap(head_other, "other.ts"));

        let ledger = match_functions(before, after);

        assert_eq!(
            find(&ledger, "a").kind,
            MatchKind::InPlace,
            "{:?}",
            kinds(&ledger)
        );
        assert_eq!(find(&ledger, "b").kind, MatchKind::MovedIdentical);
        let c = find(&ledger, "c");
        assert_eq!(c.kind, MatchKind::MovedModified, "{:?}", kinds(&ledger));
        assert_eq!(c.complexity_delta, 1);
        assert!(c.similarity >= NAME_MATCH_MIN_SIMILARITY);

        assert_eq!(ledger.totals.moved_identical, 1);
        assert_eq!(ledger.totals.moved_modified_delta, 1);
        assert_eq!(ledger.totals.in_place_delta, 0);
        assert!(ledger.totals.balanced, "{:?}", ledger.totals);
    }

    #[test]
    fn identical_body_under_a_new_name_is_a_rename() {
        let body = "{ if (xs.length) { return 1 } if (xs) { return 2 } return 0 }";
        let before = snap(&format!("function computeTotal(xs){body}\n"), "m.ts");
        let after = snap(&format!("function sumAll(xs){body}\n"), "m.ts");

        let ledger = match_functions(before, after);
        let row = find(&ledger, "computeTotal");
        assert_eq!(row.kind, MatchKind::Renamed, "{:?}", kinds(&ledger));
        assert_eq!(row.similarity, 1.0);
        assert_eq!(ledger.totals.renamed, 1);
        assert!(ledger.totals.balanced);
    }

    #[test]
    fn added_glue_is_new_logic() {
        let before = snap(BASE, "base.ts");
        let after = snap(
            &format!("{BASE}function wrapper(){{ return a() }}\n"),
            "base.ts",
        );
        let wrapper_complexity = after
            .iter()
            .find(|s| s.name == "wrapper")
            .expect("wrapper snapshot")
            .complexity;

        let ledger = match_functions(before, after);
        assert_eq!(find(&ledger, "wrapper").kind, MatchKind::New);
        assert_eq!(ledger.totals.new_logic, wrapper_complexity);
        assert!(ledger.totals.balanced);
    }

    #[test]
    fn trivial_bodies_pair_by_hash_only_when_the_name_agrees() {
        // Same name, same (empty) body, different file → a move.
        let paired = match_functions(
            snap("function a(){}\n", "base.ts"),
            snap("function a(){}\n", "other.ts"),
        );
        assert_eq!(kinds(&paired).len(), 1);
        assert_eq!(paired.matches[0].kind, MatchKind::MovedIdentical);

        // Different names: the bodies are too small to be evidence of
        // anything, so neither the hash rung nor the similarity rung fires.
        let unpaired = match_functions(
            snap("function a(){}\n", "base.ts"),
            snap("function z(){}\n", "other.ts"),
        );
        assert_eq!(find(&unpaired, "a").kind, MatchKind::Removed);
        assert_eq!(find(&unpaired, "z").kind, MatchKind::New);
        assert!(unpaired.totals.balanced);

        // The case the name clause actually exists for: two anonymous
        // callables with empty bodies hash identically (no identifiers, no
        // operators), so only the `<anonymous>@<line>` labels tell them
        // apart. Without the clause these would report as one move.
        let before = snap("[1].map(function(){});\n", "base.ts");
        let after = snap("log();\n[1].map(function(){});\n", "other.ts");
        assert_eq!(before[0].structural_hash, after[0].structural_hash);
        assert_ne!(before[0].name, after[0].name, "labels must differ by line");
        let anon = match_functions(before, after);
        assert_eq!(
            anon.matches.iter().map(|m| m.kind).collect::<Vec<_>>(),
            vec![MatchKind::Removed, MatchKind::New],
            "{:?}",
            kinds(&anon)
        );
        assert!(anon.totals.balanced);
    }

    #[test]
    fn anonymous_callback_moved_within_file_is_in_place_not_renamed() {
        // Same anonymous callback body, moved to a different line inside
        // the same enclosing function, with a trivial edit (parameter
        // renamed `x` -> `y`) so hash-pairing (rung 0) can't catch it —
        // `structural_hash` covers identifier text — and the pair has to
        // survive the residue pass at rung 3. The kind sequence (and so
        // `uast_edit_distance`) is unaffected by the rename, so similarity
        // stays at 1.0.
        let base = "function outer(items){ items.map((x) => { if (x) { return 1 } return 2 } ) }\n";
        let head =
            "function outer(items){\n\n    items.map((y) => { if (y) { return 1 } return 2 } ) }\n";

        let before = snap(base, "base.ts");
        let after = snap(head, "base.ts");

        let anon_before = before
            .iter()
            .find(|s| is_anonymous(&s.name))
            .expect("anonymous callback before");
        let anon_after = after
            .iter()
            .find(|s| is_anonymous(&s.name))
            .expect("anonymous callback after");
        assert_ne!(
            anon_before.name, anon_after.name,
            "labels must differ by line"
        );
        assert_ne!(
            anon_before.structural_hash, anon_after.structural_hash,
            "the trivial edit must defeat hash-pairing"
        );

        let ledger = match_functions(before.clone(), after.clone());
        assert!(
            ledger.matches.iter().all(|m| m.kind != MatchKind::Renamed),
            "anonymous callables must never be reported as renamed: {:?}",
            kinds(&ledger)
        );
        let anon_match = ledger
            .matches
            .iter()
            .find(|m| m.before.as_ref().is_some_and(|s| is_anonymous(&s.name)))
            .expect("anonymous pair present");
        assert!(
            anon_match.similarity >= MOVED_MODIFIED_MIN_SIMILARITY,
            "similarity too low for the residue rung to pair it: {}",
            anon_match.similarity
        );
        assert_eq!(anon_match.kind, MatchKind::InPlace, "{:?}", kinds(&ledger));
        assert!(ledger.totals.balanced, "{:?}", ledger.totals);

        // The same callback moved to a different file is a move, not a
        // rename either.
        let after_other_file = snap(head, "other.ts");
        let ledger_moved = match_functions(before, after_other_file);
        assert!(
            ledger_moved
                .matches
                .iter()
                .all(|m| m.kind != MatchKind::Renamed),
            "{:?}",
            kinds(&ledger_moved)
        );
        let anon_moved = ledger_moved
            .matches
            .iter()
            .find(|m| m.before.as_ref().is_some_and(|s| is_anonymous(&s.name)))
            .expect("anonymous pair present");
        assert_eq!(
            anon_moved.kind,
            MatchKind::MovedModified,
            "{:?}",
            kinds(&ledger_moved)
        );
        assert!(ledger_moved.totals.balanced, "{:?}", ledger_moved.totals);
    }

    #[test]
    fn ledger_serializes_with_snake_case_kinds_and_no_uast_payload() {
        assert_eq!(
            serde_json::to_value(MatchKind::MovedIdentical).expect("enum serializes"),
            serde_json::json!("moved_identical")
        );

        let ledger = match_functions(snap(BASE, "base.ts"), snap(BASE, "other.ts"));
        let value = serde_json::to_value(&ledger).expect("ledger serializes");
        let row = &value["matches"][0];
        assert_eq!(row["kind"], "moved_identical");
        assert!(
            row["before"].get("node").is_none(),
            "the owned subtree must stay out of the report: {row}"
        );
        assert!(row["before"]["structural_hash"].is_u64());
        assert_eq!(value["totals"]["balanced"], true);
    }

    #[test]
    fn nested_closures_are_reported_but_never_counted() {
        let source = "function outer(xs){ const step = (x) => { if (x) { return x } return 0 }; return xs.map(step) }\n";
        let snapshots = snap(source, "base.ts");
        let closure = snapshots
            .iter()
            .find(|s| s.nested)
            .expect("the arrow function must be a nested closure");
        assert_eq!(closure.kind, "closure");

        let outer_only: usize = snapshots
            .iter()
            .filter(|s| !s.nested)
            .map(|s| s.complexity)
            .sum();

        let ledger = match_functions(snapshots.clone(), snapshots.clone());
        assert_eq!(ledger.totals.before_total, outer_only);
        assert_eq!(ledger.totals.after_total, outer_only);
        assert!(
            ledger
                .matches
                .iter()
                .any(|m| m.before.as_ref().is_some_and(|s| s.nested)),
            "the closure still gets a ledger row"
        );
        assert!(ledger.totals.balanced);
    }

    #[test]
    fn python_function_moved_verbatim_is_moved_identical() {
        let source = "def f(x):\n    if x:\n        return 1\n    return 0\n";
        let ledger = match_functions(snap(source, "a.py"), snap(source, "b.py"));
        assert_eq!(ledger.matches.len(), 1);
        assert_eq!(ledger.matches[0].kind, MatchKind::MovedIdentical);
        assert_eq!(ledger.matches[0].similarity, 1.0);
        assert_eq!(ledger.matches[0].complexity_delta, 0);
        assert!(ledger.totals.balanced);
    }

    #[test]
    fn empty_pools_balance() {
        let ledger = match_functions(Vec::new(), Vec::new());
        assert!(ledger.matches.is_empty());
        assert!(ledger.totals.balanced);
    }

    #[test]
    fn totals_balance_across_every_fixture_combination() {
        let head_other = "function b(y){ for (const i of y) { log(i) } return y }\n\
                          function c(z){ if (z) { return z } if (z) { return z } return null }\n";
        let pools = [
            snap(BASE, "base.ts"),
            snap(head_other, "other.ts"),
            snap("function a(){}\n", "base.ts"),
            snap(
                "def f(x):\n    if x:\n        return 1\n    return 0\n",
                "a.py",
            ),
            Vec::new(),
        ];
        for b in &pools {
            for a in &pools {
                let ledger = match_functions(b.clone(), a.clone());
                assert!(
                    ledger.totals.balanced,
                    "unbalanced for {:?} -> {:?}: {:?}",
                    b.iter().map(|s| &s.name).collect::<Vec<_>>(),
                    a.iter().map(|s| &s.name).collect::<Vec<_>>(),
                    ledger.totals
                );
            }
        }
    }

    #[test]
    fn matching_is_order_independent_and_deterministic() {
        let head_other = "function c(z){ if (z) { return z } if (z) { return z } return null }\n";
        let before = snap(BASE, "base.ts");
        let after = snap(head_other, "other.ts");

        let forward = match_functions(before.clone(), after.clone());
        let mut reversed_before = before;
        reversed_before.reverse();
        let mut reversed_after = after;
        reversed_after.reverse();
        let backward = match_functions(reversed_before, reversed_after);

        assert_eq!(kinds(&forward), kinds(&backward));
    }
}
