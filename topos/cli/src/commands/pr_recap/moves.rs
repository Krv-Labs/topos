//! Refactor awareness: whether a pillar a file lost, or a score it
//! dropped, came with code moved in from elsewhere in the range.
//!
//! Moving a complex function into a file that used to be simple fails
//! that file's SIMPLE gate, but the complexity predates the change: it
//! only changed address. Every scored file's functions, both sides, go
//! into one [`match_functions`] ledger, so a move is recognized between
//! any two files in the range, split or not. The gates then report such a
//! loss as `moved_pillar` rather than `pillar_lost`. Nothing is hidden: the
//! finding stays, under a gate of its own, naming where the code came from.
//!
//! A move only explains a regression it could have caused. Code that grew
//! on the way, new logic beyond a small allowance, or a rise in SECURE
//! findings over the whole range all keep the original gate.

use std::collections::{BTreeMap, HashMap};

use topos_engine::functors::profunctors::uast::ledger::{
    match_functions, snapshot_functions, FunctionMatch, FunctionSnapshot, MatchKind,
};
use topos_engine::graphs::ast::dispatch::parse_source;
use topos_engine::graphs::mdg::split::SymbolMove;

use super::clusters::secure_findings;
use super::git::{show_file, skip_reason, DiffEntry};
use super::score::Scored;
use crate::commands::lang::detect_language;

/// A file that only received moved code may still add this much new
/// logic and have its loss read as the move's: the glue a move needs (an
/// import, a wrapper, a call site) is new logic too. It bounds two things,
/// in integer math: new function complexity, as a share of the file's
/// complexity at base (`new_logic * 100 <= MAX_NEW_LOGIC_PERCENT *
/// before_total`), and added lines the moved code does not account for
/// (`moved_lines * 100 >= (100 - MAX_NEW_LOGIC_PERCENT) * lines_added`),
/// which catches top-level code no function snapshot sees.
pub(super) const MAX_NEW_LOGIC_PERCENT: usize = 10;

/// Past this many before × after comparisons the ledger is not built and
/// nothing is read as a move. The last rung of [`match_functions`] is a
/// pairwise edit distance, and a range this large is a rewrite, not a move
/// worth excusing. Bailing only withholds the exemption: every finding
/// keeps its original gate.
const MAX_MATCH_PAIRS: usize = 100_000;

/// Why a regression is read as the move's, and where the code came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MoveCause {
    /// The file the code moved from, when one is known.
    pub(super) from: Option<String>,
}

/// One function at head that the ledger traced to a different address.
#[derive(Debug, Clone)]
struct Arrival {
    name: String,
    qualified_name: String,
    start_line: usize,
    kind: MatchKind,
    complexity_delta: i64,
    /// The base file, when the function changed files.
    from: Option<String>,
}

impl Arrival {
    fn is(&self, function: &str) -> bool {
        self.qualified_name == function || self.name == function
    }

    /// Carried across files without growing.
    fn moved_intact(&self) -> bool {
        self.from.is_some()
            && match self.kind {
                MatchKind::MovedIdentical => true,
                MatchKind::MovedModified | MatchKind::Renamed => self.complexity_delta <= 0,
                _ => false,
            }
    }
}

/// Complexity arithmetic for one file, over non-nested functions.
#[derive(Debug, Clone, Default)]
struct FileTotals {
    before_total: usize,
    in_place_delta: i64,
    new_logic: usize,
    /// Functions or symbols that arrived from another file.
    moved_in: usize,
    /// Complexity the arrivals gained on the way.
    moved_growth: i64,
    /// Lines the arrivals from other files span at head. A graph move has
    /// no span and adds nothing.
    moved_lines: usize,
    /// Lines the diff added to the file.
    lines_added: usize,
    /// How many arrivals came from each file.
    sources: BTreeMap<String, usize>,
}

/// Every move in the range, indexed by the file the code landed in.
#[derive(Debug, Default)]
pub(super) struct RangeMoves {
    arrivals: HashMap<String, Vec<Arrival>>,
    totals: HashMap<String, FileTotals>,
    /// Graph moves into existing files, when coupling was measured.
    graph_moves: Vec<SymbolMove>,
    /// The range's SECURE findings rose, so no SECURE loss is excused.
    secure_rose: bool,
}

impl RangeMoves {
    /// The moves in `scored`, plus the functions of `deleted` files at
    /// base, so code moved out of a deleted file is traced too. Each scored
    /// file's added lines come from its recap.
    /// `moved_between_existing` is the split report's, empty without
    /// coupling.
    pub(super) fn build(
        repo: &std::path::Path,
        base: &str,
        scored: &[Scored],
        deleted: &[String],
        moved_between_existing: &[SymbolMove],
    ) -> RangeMoves {
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut lines_added = HashMap::new();
        for file in scored {
            lines_added.insert(file.recap.path.clone(), file.recap.lines_added);
            before.extend(file.before_snapshots.iter().flatten().cloned());
            after.extend(file.after_snapshots.iter().flatten().cloned());
        }
        before.extend(deleted_snapshots(repo, base, deleted));
        // A deleted file's SECURE findings are not scored, so the range
        // total covers the scored files on both sides only.
        let secure_before: usize = scored
            .iter()
            .map(|file| secure_findings(&file.before))
            .sum();
        let secure_after: usize = scored.iter().map(|file| secure_findings(&file.after)).sum();
        RangeMoves::new(
            before,
            after,
            &lines_added,
            secure_after > secure_before,
            moved_between_existing,
        )
    }

    fn new(
        mut before: Vec<FunctionSnapshot>,
        mut after: Vec<FunctionSnapshot>,
        lines_added: &HashMap<String, usize>,
        secure_rose: bool,
        moved_between_existing: &[SymbolMove],
    ) -> RangeMoves {
        let mut totals: HashMap<String, FileTotals> = HashMap::new();
        for (path, added) in lines_added {
            totals.entry(path.clone()).or_default().lines_added = *added;
        }
        for snapshot in &before {
            totals
                .entry(snapshot.file.clone())
                .or_default()
                .before_total += counted(Some(snapshot)) as usize;
        }
        drop_unchanged(&mut before, &mut after);
        let mut moves = RangeMoves {
            graph_moves: moved_between_existing.to_vec(),
            secure_rose,
            ..RangeMoves::default()
        };
        if before.len().saturating_mul(after.len()) > MAX_MATCH_PAIRS {
            return moves;
        }
        for row in match_functions(before, after).matches {
            moves.record(&row, &mut totals);
        }
        for moved in &moves.graph_moves {
            let file = totals.entry(moved.to.clone()).or_default();
            file.moved_in += 1;
            *file.sources.entry(moved.from.clone()).or_default() += 1;
        }
        moves.totals = totals;
        moves
    }

    fn record(&mut self, row: &FunctionMatch, totals: &mut HashMap<String, FileTotals>) {
        let delta = counted(row.after.as_ref()) - counted(row.before.as_ref());
        let Some(after) = &row.after else {
            // Removed: nothing arrived anywhere.
            return;
        };
        let from = row
            .before
            .as_ref()
            .filter(|before| before.file != after.file)
            .map(|before| before.file.clone());
        let file = totals.entry(after.file.clone()).or_default();
        match (&from, row.kind) {
            (_, MatchKind::New) => file.new_logic += delta.unsigned_abs() as usize,
            (Some(source), _) => {
                file.moved_in += 1;
                file.moved_growth += delta.max(0);
                // A nested function's lines are its parent's already.
                if !after.nested {
                    file.moved_lines += (after.end_line + 1).saturating_sub(after.start_line);
                }
                *file.sources.entry(source.clone()).or_default() += 1;
            }
            // In place, or renamed without leaving the file.
            (None, _) => file.in_place_delta += delta,
        }
        self.arrivals
            .entry(after.file.clone())
            .or_default()
            .push(Arrival {
                name: after.name.clone(),
                qualified_name: after.qualified_name.clone(),
                start_line: after.start_line,
                kind: row.kind,
                complexity_delta: row.complexity_delta,
                from,
            });
    }

    /// Whether `file`'s regression on `pillar`, at `function` when the
    /// finding names one, came with moved code:
    ///
    /// - the function itself arrived from another file without growing;
    /// - or the file only received code: something moved in, nothing
    ///   already there grew, new logic stays within
    ///   [`MAX_NEW_LOGIC_PERCENT`] of its complexity at base, and the moved
    ///   code spans all but that share of the lines the file gained;
    /// - or the dependency graph saw the function move in from another
    ///   existing file, and the ledger does not say it grew or is new.
    ///
    /// SECURE is never excused when the range as a whole gained SECURE
    /// findings: moving a dangerous call is fine, adding one is not.
    pub(super) fn caused(
        &self,
        file: &str,
        pillar: &str,
        function: Option<&str>,
        line: Option<usize>,
    ) -> Option<MoveCause> {
        if pillar == "secure" && self.secure_rose {
            return None;
        }
        let arrival = function.and_then(|function| self.arrival(file, function, line));
        if let Some(arrival) = arrival.filter(|arrival| arrival.moved_intact()) {
            return Some(MoveCause {
                from: arrival.from.clone(),
            });
        }
        if let Some(totals) = self.totals.get(file).filter(|totals| receive_only(totals)) {
            return Some(MoveCause {
                from: main_source(totals),
            });
        }
        let function = function?;
        let contradicted = arrival
            .is_some_and(|arrival| arrival.kind == MatchKind::New || arrival.complexity_delta > 0);
        self.graph_moves
            .iter()
            .find(|moved| moved.to == file && leaf(function) == leaf(&moved.name))
            .filter(|_| !contradicted)
            .map(|moved| MoveCause {
                from: Some(moved.from.clone()),
            })
    }

    /// The ledger row for `function` in `file`, the one at `line` first
    /// when two share a name.
    fn arrival(&self, file: &str, function: &str, line: Option<usize>) -> Option<&Arrival> {
        let candidates = self.arrivals.get(file)?;
        let named = || candidates.iter().filter(|arrival| arrival.is(function));
        named()
            .find(|arrival| Some(arrival.start_line) == line)
            .or_else(|| named().next())
    }
}

/// Condition (b): the file received code and did little else. Function
/// complexity alone would miss top-level code (module statements, class
/// bodies, constants), so the moved code must also account for most of
/// the lines the file gained.
fn receive_only(totals: &FileTotals) -> bool {
    totals.moved_in > 0
        && totals.in_place_delta <= 0
        && totals.moved_growth <= 0
        && totals.new_logic * 100 <= MAX_NEW_LOGIC_PERCENT * totals.before_total
        && totals.moved_lines * 100 >= (100 - MAX_NEW_LOGIC_PERCENT) * totals.lines_added
}

/// The file most of the arrivals came from, the first by path on a tie.
fn main_source(totals: &FileTotals) -> Option<String> {
    let most = totals.sources.values().copied().max()?;
    totals
        .sources
        .iter()
        .find(|(_, count)| **count == most)
        .map(|(path, _)| path.clone())
}

/// `Cache.get` → `get`: graph symbols and ledger names disagree on how
/// much of the enclosing chain they spell out.
fn leaf(name: &str) -> &str {
    name.rsplit(['.', ':']).next().unwrap_or(name)
}

/// A function's complexity, zero for a nested closure, as the ledger's
/// own totals count it.
fn counted(snapshot: Option<&FunctionSnapshot>) -> i64 {
    snapshot
        .filter(|snapshot| !snapshot.nested)
        .map_or(0, |snapshot| snapshot.complexity as i64)
}

/// Take out every function unchanged in place: same file, same name, same
/// body on both sides. They cannot have moved, and matching them would
/// only cost time. It also means a body *copied* into a second file,
/// while the original stays put, reads as new logic rather than a move.
fn drop_unchanged(before: &mut Vec<FunctionSnapshot>, after: &mut Vec<FunctionSnapshot>) {
    type Key = (String, String, u64);
    let key = |snapshot: &FunctionSnapshot| -> Key {
        (
            snapshot.file.clone(),
            snapshot.qualified_name.clone(),
            snapshot.structural_hash,
        )
    };
    let mut at_head: HashMap<Key, usize> = HashMap::new();
    for snapshot in after.iter() {
        *at_head.entry(key(snapshot)).or_default() += 1;
    }
    let mut unchanged: HashMap<Key, usize> = HashMap::new();
    before.retain(|snapshot| {
        let key = key(snapshot);
        match at_head.get_mut(&key).filter(|left| **left > 0) {
            Some(left) => {
                *left -= 1;
                *unchanged.entry(key).or_default() += 1;
                false
            }
            None => true,
        }
    });
    after.retain(|snapshot| match unchanged.get_mut(&key(snapshot)) {
        Some(left) if *left > 0 => {
            *left -= 1;
            false
        }
        _ => true,
    });
}

/// The functions of each deleted source file at base. Parsing is cheap
/// next to scoring, and without them a function moved out of a deleted
/// file would read as new logic wherever it landed. A file that cannot be
/// read or parsed is left out.
fn deleted_snapshots(
    repo: &std::path::Path,
    base: &str,
    deleted: &[String],
) -> Vec<FunctionSnapshot> {
    deleted
        .iter()
        .filter(|path| {
            skip_reason(&DiffEntry {
                status: "D".to_string(),
                path: (*path).clone(),
                old_path: None,
            })
            .is_none()
        })
        .filter_map(|path| {
            let source = show_file(repo, base, path).ok()?;
            let language = detect_language(std::path::Path::new(path));
            let parsed = parse_source(&source, &language, Some(path)).ok()?;
            Some(snapshot_functions(&parsed.uast_root, &source, path))
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A branchy function: complexity well over the SIMPLE gate.
    const COMPLEX: &str = "def pick(x):\n    if x == 1:\n        return 1\n    if x == 2:\n        return 2\n    if x == 3:\n        return 3\n    if x == 4:\n        return 4\n    if x == 5:\n        return 5\n    return 0\n";
    /// `COMPLEX` with one more branch.
    const GROWN: &str = "def pick(x):\n    if x == 1:\n        return 1\n    if x == 2:\n        return 2\n    if x == 3:\n        return 3\n    if x == 4:\n        return 4\n    if x == 5:\n        return 5\n    if x == 6:\n        return 6\n    return 0\n";
    const SIMPLE: &str = "def keep(x):\n    if x:\n        return 1\n    return 0\n";

    fn snap(source: &str, file: &str) -> Vec<FunctionSnapshot> {
        let parsed = parse_source(source, "python", Some(file)).expect("fixture must parse");
        snapshot_functions(&parsed.uast_root, source, file)
    }

    fn joined(parts: &[&str]) -> String {
        parts.join("\n")
    }

    /// The lines b.py gained when `SIMPLE` became `after`, as the diff
    /// counts them for code appended after it.
    fn b_gained(after: &str) -> HashMap<String, usize> {
        let added = after.lines().count() - SIMPLE.lines().count();
        HashMap::from([("b.py".to_string(), added)])
    }

    /// `pick` in a.py and `keep` in b.py at base; b.py is `after` at head.
    fn moves(after: &str, secure_rose: bool) -> RangeMoves {
        moves_beside(after, Vec::new(), secure_rose)
    }

    /// [`moves`], with `also` among the functions at head.
    fn moves_beside(after: &str, also: Vec<FunctionSnapshot>, secure_rose: bool) -> RangeMoves {
        let mut before = snap(COMPLEX, "a.py");
        before.extend(snap(SIMPLE, "b.py"));
        let mut head = also;
        head.extend(snap(after, "b.py"));
        RangeMoves::new(before, head, &b_gained(after), secure_rose, &[])
    }

    #[test]
    fn a_function_moved_verbatim_explains_the_loss() {
        let moves = moves(&joined(&[SIMPLE, COMPLEX]), false);
        let cause = moves.caused("b.py", "simple", Some("pick"), None);
        assert_eq!(
            cause,
            Some(MoveCause {
                from: Some("a.py".to_string())
            })
        );
    }

    #[test]
    fn a_function_that_grew_on_the_way_is_not_excused() {
        let moves = moves(&joined(&[SIMPLE, GROWN]), false);
        assert_eq!(moves.caused("b.py", "simple", Some("pick"), None), None);
    }

    #[test]
    fn a_new_function_is_not_excused() {
        let mut before = snap(SIMPLE, "b.py");
        before.extend(snap("def other():\n    return 1\n", "a.py"));
        let after = joined(&[SIMPLE, COMPLEX]);
        let moves = RangeMoves::new(before, snap(&after, "b.py"), &b_gained(&after), false, &[]);
        assert_eq!(moves.caused("b.py", "simple", Some("pick"), None), None);
    }

    #[test]
    fn secure_is_not_excused_when_the_range_gained_findings() {
        let moves = moves(&joined(&[SIMPLE, COMPLEX]), true);
        assert_eq!(moves.caused("b.py", "secure", Some("pick"), None), None);
        assert!(moves.caused("b.py", "simple", Some("pick"), None).is_some());
    }

    #[test]
    fn a_file_that_only_received_code_is_excused_whatever_the_function() {
        // The finding names no function, or one the ledger never saw.
        let moves = moves(&joined(&[SIMPLE, COMPLEX]), false);
        let cause = moves.caused("b.py", "navigable", None, None);
        assert_eq!(cause.and_then(|cause| cause.from).as_deref(), Some("a.py"));

        // Adding more than MAX_NEW_LOGIC_PERCENT of new logic besides the
        // move ends the excuse.
        let busy = moves_with_extra(COMPLEX);
        assert_eq!(busy.caused("b.py", "navigable", None, None), None);
    }

    /// The move plus a new function `extra` in b.py.
    fn moves_with_extra(extra: &str) -> RangeMoves {
        let extra = extra.replace("def pick", "def fresh");
        moves(&joined(&[SIMPLE, COMPLEX, &extra]), false)
    }

    #[test]
    fn a_trivial_move_does_not_excuse_new_top_level_code() {
        // One small function moves into b.py beside a pile of module-level
        // code no function snapshot sees: the loss is the new code's.
        let trivial = "def tiny():\n    return 1\n";
        let mut top_level = String::new();
        for i in 0..20 {
            top_level.push_str(&format!("if FLAG_{i}:\n    TABLE.append({i})\n"));
        }
        let mut before = snap(trivial, "a.py");
        before.extend(snap(SIMPLE, "b.py"));
        let after = joined(&[SIMPLE, trivial, &top_level]);
        let moves = RangeMoves::new(before, snap(&after, "b.py"), &b_gained(&after), false, &[]);
        let totals = &moves.totals["b.py"];
        assert_eq!((totals.moved_in, totals.new_logic), (1, 0));
        assert!(!receive_only(totals), "{totals:?}");
        assert_eq!(moves.caused("b.py", "navigable", None, None), None);
        // The moved function itself is still excused by name.
        assert!(moves.caused("b.py", "simple", Some("tiny"), None).is_some());
    }

    #[test]
    fn a_body_copied_while_the_original_stays_is_new_logic() {
        let moves = moves_beside(&joined(&[SIMPLE, COMPLEX]), snap(COMPLEX, "a.py"), false);
        assert_eq!(moves.caused("b.py", "simple", Some("pick"), None), None);
    }

    #[test]
    fn a_graph_move_counts_unless_the_ledger_disagrees() {
        let graph = [SymbolMove {
            name: "pick".to_string(),
            kind: "Function".to_string(),
            from: "a.py".to_string(),
            to: "b.py".to_string(),
        }];
        // The ledger never saw `pick`, and b.py grew in place, so neither
        // of the other two conditions holds: only the graph saw the move.
        let grew_in_place = SIMPLE.replace(
            "    return 0\n",
            "    if x > 9:\n        return 9\n    return 0\n",
        );
        let moves = RangeMoves::new(
            snap(SIMPLE, "b.py"),
            snap(&grew_in_place, "b.py"),
            &HashMap::new(),
            false,
            &graph,
        );
        assert!(!receive_only(&moves.totals["b.py"]));
        let cause = moves.caused("b.py", "simple", Some("Mod.pick"), None);
        assert_eq!(cause.and_then(|cause| cause.from).as_deref(), Some("a.py"));

        let mut before = snap(COMPLEX, "a.py");
        before.extend(snap(SIMPLE, "b.py"));
        let grown = snap(&joined(&[SIMPLE, GROWN]), "b.py");
        let moves = RangeMoves::new(before, grown, &HashMap::new(), false, &graph);
        assert_eq!(moves.caused("b.py", "simple", Some("pick"), None), None);
    }
}
