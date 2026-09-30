//! Hand-built `PrRecap` documents for the renderer tests.
//!
//! Shared by the card, the GitHub comment and the view model, so all of
//! them are asserted against exactly the same numbers. The shape and the
//! counts are PR #5 from
//! `docs/decisions/pr-recap-refactor-tracing.md`.

use std::collections::HashMap;

use topos_engine::config::PrGateConfig;
use topos_engine::functors::profunctors::uast::ledger::{
    FunctionMatch, FunctionSnapshot, Ledger, LedgerTotals, MatchKind,
};
use topos_engine::graphs::mdg::file_graph::FileGraph;
use topos_engine::graphs::mdg::models::{GraphNode, GraphRelationship};
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_engine::graphs::mdg::split::{NewSymbol, Reach, SymbolMove};
use topos_engine::graphs::uast::models::{NativeRef, SourceSpan, UASTNode};

use super::gates;
use super::model::{
    Cluster, ClusterChild, ClusterMark, ClusterMembership, ClusterRole, CouplingReason,
    CouplingStatus, FileChange, FileRecap, FunctionRef, Headline, Hotspot, Medal, PillarDelta,
    PillarRollup, PrRecap, ProjectRollup as Rollup, PullRequest, Scope, SCHEMA,
};
use super::moves::RangeMoves;
use super::view::PILLARS;

pub(super) fn medal_for(tier: &str) -> Medal {
    let symbol = match tier {
        "PLATINUM" => "🏆",
        "GOLD" => "🥇",
        "SILVER" => "🥈",
        "BRONZE" => "🥉",
        _ => "⚠",
    };
    Medal {
        symbol: symbol.to_string(),
        tier: tier.to_string(),
        verdict: match tier {
            "PLATINUM" => "SIMPLE_COMPOSABLE_SECURE_NAVIGABLE",
            "GOLD" => "COMPOSABLE_SECURE_NAVIGABLE",
            "SILVER" => "SECURE_NAVIGABLE",
            "BRONZE" => "SECURE",
            _ => "NONE",
        }
        .to_string(),
    }
}

struct Spec<'a> {
    path: &'a str,
    change: FileChange,
    status: Headline,
    before: Option<&'a str>,
    after: &'a str,
    /// `(before_passed, after_passed, before_score, after_score)` in
    /// simple, composable, secure, navigable order.
    pillars: [(bool, bool, f64, f64); 4],
    worst: (usize, usize),
    decisions: (usize, usize),
    cluster: Option<(&'a str, ClusterRole)>,
    cosmetic: bool,
}

fn build(spec: Spec<'_>) -> FileRecap {
    let is_new = spec.change == FileChange::Added;
    let pillars = PILLARS
        .iter()
        .zip(spec.pillars)
        .map(|(key, (before, after, before_score, after_score))| {
            (
                (*key).to_string(),
                PillarDelta {
                    measured: true,
                    before_passed: (!is_new).then_some(before),
                    after_passed: Some(after),
                    before_score: (!is_new).then_some(before_score),
                    after_score: Some(after_score),
                    lost_gate: None,
                    gate: None,
                },
            )
        })
        .collect();
    FileRecap {
        path: spec.path.to_string(),
        change: spec.change,
        status: spec.status,
        lines_before: 100,
        lines_after: 120,
        lines_added: 40,
        lines_removed: 20,
        medal_before: spec.before.map(medal_for),
        medal_after: Some(medal_for(spec.after)),
        pillars,
        structural_distance: Some(0.4),
        cosmetic: spec.cosmetic,
        complexity_relocated_within_file: false,
        worst_function_before: (!is_new).then(|| function("worstBefore", spec.worst.0)),
        worst_function_after: Some(function("worstAfter", spec.worst.1)),
        decisions_before: (!is_new).then_some(spec.decisions.0),
        decisions_after: Some(spec.decisions.1),
        fan_in_before: Some(1),
        fan_in_after: Some(2),
        fan_out_before: Some(3),
        fan_out_after: Some(4),
        cluster: spec.cluster.map(|(parent, role)| ClusterMembership {
            parent: parent.to_string(),
            role,
        }),
        hotspots: Vec::new(),
        severity: None,
    }
}

fn function(name: &str, complexity: usize) -> FunctionRef {
    FunctionRef {
        name: name.to_string(),
        line: 42,
        complexity,
    }
}

fn child(path: &str, reach: Reach, importers: usize, moved_in: usize) -> ClusterChild {
    ClusterChild {
        path: path.to_string(),
        reach: Some(reach),
        importers: (0..importers)
            .map(|index| format!("caller{index}.ts"))
            .collect(),
        moved_in,
    }
}

fn snapshot(file: &str, name: &str, complexity: usize) -> FunctionSnapshot {
    FunctionSnapshot {
        file: file.to_string(),
        name: name.to_string(),
        qualified_name: name.to_string(),
        kind: "Function".to_string(),
        start_line: 1,
        end_line: 20,
        complexity,
        nested: false,
        structural_hash: 7,
        node: UASTNode {
            kind: "FunctionDecl".to_string(),
            lang: "typescript".to_string(),
            span: SourceSpan {
                file: None,
                start_byte: 0,
                end_byte: 0,
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 0,
            },
            native: NativeRef {
                parser: "tree-sitter".to_string(),
                parser_version: "0.22".to_string(),
                node_kind: "function_declaration".to_string(),
            },
            attributes: HashMap::new(),
            children: Vec::new(),
            id: String::new(),
        },
    }
}

fn moved(from: &str, to: &str, name: &str, before: usize, after: usize) -> FunctionMatch {
    #[expect(clippy::cast_possible_wrap, reason = "fixture complexities are tiny")]
    let delta = after as i64 - before as i64;
    FunctionMatch {
        kind: if delta == 0 {
            MatchKind::MovedIdentical
        } else {
            MatchKind::MovedModified
        },
        before: Some(snapshot(from, name, before)),
        after: Some(snapshot(to, name, after)),
        similarity: 1.0,
        complexity_delta: delta,
    }
}

fn ledger(matches: Vec<FunctionMatch>) -> Ledger {
    Ledger {
        matches,
        totals: LedgerTotals {
            before_total: 83,
            after_total: 87,
            new_logic: 4,
            balanced: true,
            ..LedgerTotals::default()
        },
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "a fixture cluster has many facts"
)]
fn cluster_of(
    parent: &str,
    children: Vec<ClusterChild>,
    mark: ClusterMark,
    reasons: Vec<String>,
    worst: (usize, usize),
    decisions: (usize, usize),
    lines: (usize, usize),
    fan_out: (usize, usize),
    ledger: Option<Ledger>,
) -> Cluster {
    Cluster {
        parent: parent.to_string(),
        children,
        mark,
        reasons,
        lines_before: lines.0,
        lines_after: lines.1,
        decisions_before: decisions.0,
        decisions_after: decisions.1,
        worst_function_before: Some(function("parentWorst", worst.0)),
        worst_function_after: Some(function("parentWorst", worst.1)),
        parent_fan_out_before: Some(fan_out.0),
        parent_fan_out_after: Some(fan_out.1),
        parent_fan_out_after_excluding_children: Some(fan_out.0),
        secure_findings_before: 0,
        secure_findings_after: 0,
        symbols_moved: Vec::new(),
        symbols_new: Vec::new(),
        symbols_lost: Vec::new(),
        ledger,
    }
}

fn rollup(before: &str, after: &str, regression: bool, lost: &[&str]) -> Rollup {
    let (medal_before, medal_after) = (medal_for(before), medal_for(after));
    /// Per-pillar `(before_score, after_score)` on the displayed scale.
    const SCORES: [(f64, f64); 4] = [(11.0, 38.0), (23.0, 26.0), (100.0, 100.0), (29.0, 70.0)];
    let holds = |verdict: &str, key: &str| {
        verdict
            .split('_')
            .any(|part| part.eq_ignore_ascii_case(key))
    };
    let pillars = PILLARS
        .iter()
        .zip(SCORES)
        .map(|(key, (before_score, after_score))| {
            let lost = lost.contains(key);
            let before_passed = lost || holds(&medal_before.verdict, key);
            let after_passed = !lost && holds(&medal_after.verdict, key);
            (
                (*key).to_string(),
                PillarRollup {
                    before_passed,
                    after_passed,
                    before_score,
                    after_score,
                    files_before: 23,
                    files_after: 23,
                    failing_before: if before_passed { 0 } else { 3 },
                    failing_after: if after_passed { 0 } else { 3 },
                },
            )
        })
        .collect();
    Rollup {
        medal_before,
        medal_after,
        pillars,
        regression,
        files_before: 23,
        files_after: 23,
    }
}

fn scope(files: usize, new: usize, skipped: usize, measured: bool) -> Scope {
    Scope {
        files_scored: files,
        files_new: new,
        lines_added: 2539,
        lines_removed: 1864,
        files_skipped: skipped,
        files_deleted: 0,
        files_capped: 0,
        coupling: CouplingStatus {
            measured,
            note: if measured {
                "built from .git/topos-pr-5".to_string()
            } else {
                "gitnexus not installed".to_string()
            },
            reason: if measured {
                CouplingReason::Built
            } else {
                CouplingReason::GitnexusMissing
            },
            estimate_ms: None,
        },
    }
}

fn recap_of(
    headline: Headline,
    files: Vec<FileRecap>,
    clusters: Vec<Cluster>,
    project: Option<Rollup>,
    scope: Scope,
) -> PrRecap {
    // The readiness comes from the recommended gates, as in `build_recap`.
    let cfg = PrGateConfig::default();
    let mut files = files;
    let (readiness, findings) =
        gates::evaluate(&files, &clusters, 0, &cfg, &RangeMoves::default(), None);
    for file in &mut files {
        file.severity = gates::worst_at(&findings, &file.path);
    }
    let exit_code = readiness.exit_code(cfg.fail_on);
    PrRecap {
        schema: SCHEMA,
        base: "2e352d7aaaaaaa".to_string(),
        head: "7b18166bbbbbbb".to_string(),
        review: Some(PullRequest {
            number: 5,
            head_ref: "refactor/topos".to_string(),
            base_ref: "main".to_string(),
        }),
        gate: gates::summary(&cfg, None),
        readiness,
        exit_code,
        check: if exit_code == 1 { "fail" } else { "pass" },
        findings,
        waivers: Vec::new(),
        direction: headline,
        reason: "the split moved the worst functions down".to_string(),
        priority: "secure",
        incomplete: false,
        scope,
        project,
        added: None,
        clusters,
        files,
        skipped: Vec::new(),
        deleted: Vec::new(),
        hotspots: Vec::new(),
        hotspots_total: 0,
        non_claim: "Structural direction is not proof that tests or behavior still pass.",
    }
}

/// New child of a split: added, every pillar scored, no `before` side.
fn new_child(path: &str, tier: &str, pillars: [bool; 4], worst: usize, parent: &str) -> FileRecap {
    build(Spec {
        path,
        change: FileChange::Added,
        status: Headline::Improvement,
        before: None,
        after: tier,
        pillars: [
            (pillars[0], pillars[0], 0.0, 90.0),
            (pillars[1], pillars[1], 0.0, 90.0),
            (pillars[2], pillars[2], 0.0, 90.0),
            (pillars[3], pillars[3], 0.0, 90.0),
        ],
        worst: (0, worst),
        decisions: (0, 6),
        cluster: Some((parent, ClusterRole::Child)),
        cosmetic: false,
    })
}

/// PR #5: 23 files, four split clusters, one medal up, no regression.
pub(super) fn fixture_pr5() -> PrRecap {
    const POLL: &str = "components/polls/PollShell.tsx";
    const WEEK: &str = "components/polls/WeekGrid.tsx";
    const LINK: &str = "components/links/LinkForm.tsx";
    const CREATE: &str = "lib/bookings/create.ts";

    let mut files = vec![
        build(Spec {
            path: POLL,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("BRONZE"),
            after: "BRONZE",
            pillars: [
                (false, false, 20.0, 20.0),
                (false, false, 30.0, 30.0),
                (true, true, 100.0, 100.0),
                (false, false, 40.0, 40.0),
            ],
            worst: (117, 85),
            decisions: (68, 71),
            cluster: Some((POLL, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: WEEK,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("SILVER"),
            after: "SILVER",
            pillars: [
                (false, false, 22.0, 22.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (false, false, 44.0, 44.0),
            ],
            worst: (134, 93),
            decisions: (83, 87),
            cluster: Some((WEEK, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: LINK,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("BRONZE"),
            after: "BRONZE",
            pillars: [
                (false, false, 18.0, 18.0),
                (false, false, 35.0, 35.0),
                (true, true, 100.0, 100.0),
                (false, false, 41.0, 41.0),
            ],
            worst: (117, 73),
            decisions: (33, 46),
            cluster: Some((LINK, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: CREATE,
            change: FileChange::Modified,
            status: Headline::Improvement,
            before: Some("BRONZE"),
            after: "SILVER",
            pillars: [
                (false, false, 10.0, 10.0),
                (false, false, 27.0, 0.0),
                (true, true, 100.0, 100.0),
                (false, true, 0.0, 100.0),
            ],
            worst: (33, 13),
            decisions: (17, 14),
            cluster: Some((CREATE, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: "lib/polls/ranges.ts",
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 30.0, 30.0),
                (true, true, 88.0, 88.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (9, 9),
            decisions: (12, 12),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "lib/polls/ranges.test.ts",
            change: FileChange::Modified,
            status: Headline::ImprovementScore,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 30.0, 30.0),
                (true, true, 88.0, 88.0),
                (true, true, 100.0, 100.0),
                (true, true, 91.0, 94.0),
            ],
            worst: (8, 8),
            decisions: (10, 10),
            cluster: None,
            cosmetic: false,
        }),
    ];

    let platinum = [true, true, true, true];
    let gold = [false, true, true, true];
    let silver = [false, true, true, false];
    files.extend([
        new_child(
            "components/polls/poll-shell-types.tsx",
            "PLATINUM",
            platinum,
            4,
            POLL,
        ),
        new_child(
            "components/polls/PollSubmittedView.tsx",
            "GOLD",
            gold,
            24,
            POLL,
        ),
        new_child(
            "components/polls/PollIdentifyView.tsx",
            "PLATINUM",
            platinum,
            5,
            POLL,
        ),
        new_child(
            "components/polls/KillCheckModal.tsx",
            "PLATINUM",
            platinum,
            4,
            POLL,
        ),
        new_child(
            "components/polls/ThinCoverageModal.tsx",
            "PLATINUM",
            platinum,
            3,
            POLL,
        ),
        new_child(
            "components/polls/week-grid-model.ts",
            "PLATINUM",
            platinum,
            9,
            WEEK,
        ),
        new_child(
            "components/polls/WeekGridLegend.tsx",
            "PLATINUM",
            platinum,
            6,
            WEEK,
        ),
        new_child(
            "components/polls/WeekGridDesktop.tsx",
            "GOLD",
            gold,
            9,
            WEEK,
        ),
        new_child("components/polls/WeekGridMobile.tsx", "GOLD", gold, 8, WEEK),
        new_child(
            "components/links/link-form-defaults.ts",
            "SILVER",
            silver,
            7,
            LINK,
        ),
        new_child(
            "components/links/form-controls.tsx",
            "PLATINUM",
            platinum,
            6,
            LINK,
        ),
        new_child(
            "components/links/MemberAvailabilitySection.tsx",
            "PLATINUM",
            platinum,
            8,
            LINK,
        ),
        new_child(
            "components/links/LivePreviewCard.tsx",
            "GOLD",
            gold,
            9,
            LINK,
        ),
        new_child(
            "lib/bookings/booking-error.ts",
            "PLATINUM",
            platinum,
            1,
            CREATE,
        ),
        new_child(
            "lib/bookings/booking-guards.ts",
            "PLATINUM",
            platinum,
            5,
            CREATE,
        ),
        new_child(
            "lib/bookings/booking-notify.ts",
            "PLATINUM",
            platinum,
            4,
            CREATE,
        ),
        new_child(
            "lib/bookings/booking-slots.ts",
            "PLATINUM",
            platinum,
            6,
            CREATE,
        ),
    ]);

    let clusters = vec![
        cluster_of(
            POLL,
            vec![
                child(
                    "components/polls/poll-shell-types.tsx",
                    Reach::Shared,
                    6,
                    12,
                ),
                child(
                    "components/polls/PollSubmittedView.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
                child(
                    "components/polls/PollIdentifyView.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
                child("components/polls/KillCheckModal.tsx", Reach::Private, 1, 0),
                child(
                    "components/polls/ThinCoverageModal.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
            ],
            ClusterMark::Ok,
            Vec::new(),
            (117, 85),
            (68, 71),
            (1304, 1475),
            (15, 24),
            None,
        ),
        cluster_of(
            WEEK,
            vec![
                child("components/polls/week-grid-model.ts", Reach::Shared, 5, 17),
                child("components/polls/WeekGridLegend.tsx", Reach::Private, 1, 0),
                child("components/polls/WeekGridDesktop.tsx", Reach::Private, 1, 0),
                child("components/polls/WeekGridMobile.tsx", Reach::Private, 1, 0),
            ],
            ClusterMark::Ok,
            Vec::new(),
            (134, 93),
            (83, 87),
            (1191, 1439),
            (0, 5),
            Some(ledger(vec![moved(
                WEEK,
                "components/polls/week-grid-model.ts",
                "buildModel",
                13,
                13,
            )])),
        ),
        cluster_of(
            LINK,
            vec![
                child(
                    "components/links/link-form-defaults.ts",
                    Reach::Shared,
                    4,
                    14,
                ),
                child("components/links/form-controls.tsx", Reach::Private, 1, 0),
                child(
                    "components/links/MemberAvailabilitySection.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
                child("components/links/LivePreviewCard.tsx", Reach::Private, 1, 0),
            ],
            ClusterMark::Warn,
            vec!["decisions rose 33→46 (+39%)".to_string()],
            (117, 73),
            (33, 46),
            (1138, 1305),
            (11, 19),
            None,
        ),
        cluster_of(
            CREATE,
            vec![
                child("lib/bookings/booking-error.ts", Reach::Private, 1, 0),
                child("lib/bookings/booking-guards.ts", Reach::Private, 1, 0),
                child("lib/bookings/booking-notify.ts", Reach::Private, 1, 0),
                child("lib/bookings/booking-slots.ts", Reach::Private, 1, 0),
            ],
            ClusterMark::Ok,
            Vec::new(),
            (33, 13),
            (17, 14),
            (313, 411),
            (13, 11),
            None,
        ),
    ];

    recap_of(
        Headline::Improvement,
        files,
        clusters,
        Some(rollup("BRONZE", "BRONZE", false, &[])),
        scope(23, 17, 1, true),
    )
}

/// A plain edit: two modified files, no splits at all.
pub(super) fn fixture_plain() -> PrRecap {
    let files = vec![
        build(Spec {
            path: "topos/cli/src/commands/config.rs",
            change: FileChange::Modified,
            status: Headline::ImprovementScore,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 44.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (14, 12),
            decisions: (30, 28),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/cli/src/commands/inspect.rs",
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("SILVER"),
            after: "SILVER",
            pillars: [
                (false, false, 30.0, 30.0),
                (true, true, 70.0, 70.0),
                (true, true, 100.0, 100.0),
                (false, false, 50.0, 50.0),
            ],
            worst: (20, 20),
            decisions: (40, 40),
            cluster: None,
            cosmetic: false,
        }),
    ];
    recap_of(
        Headline::ImprovementScore,
        files,
        Vec::new(),
        Some(rollup("SILVER", "SILVER", false, &[])),
        scope(2, 0, 0, false),
    )
}

/// The path in [`fixture_plain`] that fails SIMPLE on both sides.
pub(super) const FAN_IN_TARGET: &str = "topos/cli/src/commands/config.rs";

/// A file-level MDG over `imports`, one `File` node per path named.
fn file_graph(imports: &[(&str, &str)]) -> FileGraph {
    let mut graph = ModuleDependencyGraph::new("x");
    for (from, to) in imports {
        for path in [from, to] {
            graph.add_node(GraphNode {
                id: format!("File:{path}"),
                label: "File".to_string(),
                properties: HashMap::from([("filePath".to_string(), (*path).into())]),
            });
        }
        graph.add_relationship(GraphRelationship {
            id: format!("{from}->{to}"),
            source_id: format!("File:{from}"),
            target_id: format!("File:{to}"),
            rel_type: "IMPORTS".to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties: HashMap::new(),
        });
    }
    FileGraph::build(&graph)
}

/// [`fixture_plain`], measured: a new Python import cycle (warn), three
/// new dependents on a file failing SIMPLE (warn), and the change's reach
/// (info), all from two hand-built graphs.
pub(super) fn fixture_coupling() -> PrRecap {
    let mut recap = fixture_plain();
    recap.scope = scope(2, 0, 0, true);
    let base = [
        ("topos/bind/models.py", "topos/bind/views.py"),
        ("topos/cli/src/main.rs", "topos/cli/src/commands/inspect.rs"),
        (FAN_IN_TARGET, "topos/engine/src/config/mod.rs"),
    ];
    let mut head = base.to_vec();
    head.extend([
        ("topos/bind/views.py", "topos/bind/models.py"),
        ("topos/cli/src/commands/a.rs", FAN_IN_TARGET),
        ("topos/cli/src/commands/b.rs", FAN_IN_TARGET),
        ("topos/cli/src/commands/c.rs", FAN_IN_TARGET),
    ]);
    let coupling = gates::Coupling {
        base: file_graph(&base),
        head: file_graph(&head),
        changed: [
            FAN_IN_TARGET,
            "topos/cli/src/commands/inspect.rs",
            "topos/bind/views.py",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        ..gates::Coupling::default()
    };
    let cfg = PrGateConfig::default();
    let (readiness, findings) = gates::evaluate(
        &recap.files,
        &recap.clusters,
        0,
        &cfg,
        &RangeMoves::default(),
        Some(&coupling),
    );
    for file in &mut recap.files {
        file.severity = gates::worst_at(&findings, &file.path);
    }
    recap.readiness = readiness;
    recap.exit_code = readiness.exit_code(cfg.fail_on);
    recap.check = if recap.exit_code == 1 { "fail" } else { "pass" };
    recap.findings = findings;
    recap
}

/// One file that lost SIMPLE while gaining NAVIGABLE: a lateral move by
/// status, a blocking `pillar_lost` by the gates.
pub(super) const LATERAL_LOSS: &str = "topos/cli/src/commands/lattice.rs";

pub(super) fn fixture_lateral_loss() -> PrRecap {
    let files = vec![build(Spec {
        path: LATERAL_LOSS,
        change: FileChange::Modified,
        status: Headline::LateralMove,
        before: Some("SILVER"),
        after: "SILVER",
        pillars: [
            (true, false, 60.0, 40.0),
            (true, true, 80.0, 80.0),
            (true, true, 100.0, 100.0),
            (false, true, 40.0, 90.0),
        ],
        worst: (12, 12),
        decisions: (30, 30),
        cluster: None,
        cosmetic: false,
    })];
    recap_of(
        Headline::LateralMove,
        files,
        Vec::new(),
        Some(rollup("SILVER", "SILVER", false, &[])),
        scope(1, 0, 0, false),
    )
}

/// One cluster plus every unclustered row word the card can print.
pub(super) fn fixture_mixed() -> PrRecap {
    const PARENT: &str = "topos/mcp/src/evaluation/depgraph.rs";
    let mut files = vec![
        build(Spec {
            path: "topos/mcp/src/tools/depgraph.rs",
            change: FileChange::Modified,
            status: Headline::Regression,
            before: Some("GOLD"),
            after: "SILVER",
            pillars: [
                (true, false, 62.0, 38.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (10, 14),
            decisions: (50, 58),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/engine/src/graphs/mdg/object.rs",
            change: FileChange::Modified,
            status: Headline::RegressionScore,
            before: Some("SILVER"),
            after: "SILVER",
            pillars: [
                (false, false, 40.0, 38.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (false, false, 50.0, 50.0),
            ],
            worst: (18, 19),
            decisions: (60, 62),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/mcp/src/evaluation/freshness.rs",
            change: FileChange::Modified,
            status: Headline::RegressionScore,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 45.0, 30.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 77.0, 74.0),
            ],
            worst: (12, 13),
            decisions: (20, 22),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/cli/src/commands/composable.rs",
            change: FileChange::Modified,
            status: Headline::SuspiciousNoStructuralChange,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 40.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 70.0, 74.0),
            ],
            worst: (9, 9),
            decisions: (14, 14),
            cluster: None,
            cosmetic: true,
        }),
        build(Spec {
            path: "topos/engine/src/adapters/gitnexus.rs",
            change: FileChange::Modified,
            status: Headline::Improvement,
            before: Some("BRONZE"),
            after: "SILVER",
            pillars: [
                (false, false, 30.0, 30.0),
                (false, false, 40.0, 40.0),
                (true, true, 100.0, 100.0),
                (false, true, 55.0, 81.0),
            ],
            worst: (22, 15),
            decisions: (44, 40),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/mcp/src/context_budget.rs",
            change: FileChange::Added,
            status: Headline::Improvement,
            before: None,
            after: "PLATINUM",
            pillars: [
                (true, true, 0.0, 95.0),
                (true, true, 0.0, 95.0),
                (true, true, 0.0, 100.0),
                (true, true, 0.0, 92.0),
            ],
            worst: (0, 6),
            decisions: (0, 8),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/cli/src/commands/depgraph.rs",
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 40.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (10, 10),
            decisions: (18, 18),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: PARENT,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 40.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (41, 18),
            decisions: (52, 54),
            cluster: Some((PARENT, ClusterRole::Parent)),
            cosmetic: false,
        }),
    ];
    let platinum = [true, true, true, true];
    let gold = [false, true, true, true];
    files.extend([
        new_child(
            "topos/mcp/src/evaluation/gitref.rs",
            "PLATINUM",
            platinum,
            4,
            PARENT,
        ),
        new_child(
            "topos/mcp/src/evaluation/window.rs",
            "PLATINUM",
            platinum,
            5,
            PARENT,
        ),
        new_child("topos/mcp/src/evaluation/store.rs", "GOLD", gold, 7, PARENT),
    ]);

    let clusters = vec![cluster_of(
        PARENT,
        vec![
            child("topos/mcp/src/evaluation/gitref.rs", Reach::Shared, 2, 4),
            child("topos/mcp/src/evaluation/window.rs", Reach::Private, 1, 0),
            child("topos/mcp/src/evaluation/store.rs", Reach::Private, 1, 0),
        ],
        ClusterMark::Ok,
        Vec::new(),
        (41, 18),
        (52, 54),
        (520, 610),
        (6, 9),
        None,
    )];

    let mut recap = recap_of(
        Headline::Regression,
        files,
        clusters,
        Some(rollup("SILVER", "SILVER", false, &[])),
        scope(11, 3, 0, true),
    );
    recap.deleted = vec!["topos/mcp/src/legacy.rs".to_string()];
    recap.hotspots = vec![Hotspot {
        path: "topos/mcp/src/tools/depgraph.rs".to_string(),
        line: 212,
        function: Some("cap_generation_detail".to_string()),
        metric: "ast.max_function_complexity".to_string(),
        detail: "cap_generation_detail complexity 14, gate 10".to_string(),
        advice: "Extract a decision so this function clears the gate.".to_string(),
    }];
    recap.hotspots_total = 1;
    recap
}

/// A split that went wrong: the parent lost SECURE and SIMPLE, one child
/// landed SLOP, and a moved function got more complex on the way.
pub(super) fn fixture_losses() -> PrRecap {
    const PARENT: &str = "topos/engine/src/functors/probes/cpg/taint.rs";
    let mut files = vec![build(Spec {
        path: PARENT,
        change: FileChange::Modified,
        status: Headline::Regression,
        before: Some("GOLD"),
        after: "BRONZE",
        pillars: [
            (true, false, 70.0, 30.0),
            (true, true, 80.0, 80.0),
            (true, false, 90.0, 40.0),
            (true, true, 90.0, 90.0),
        ],
        worst: (30, 48),
        decisions: (40, 61),
        cluster: Some((PARENT, ClusterRole::Parent)),
        cosmetic: false,
    })];
    files.extend([
        new_child(
            "topos/engine/src/functors/probes/cpg/taint_sinks.rs",
            "SLOP",
            [false, false, false, false],
            21,
            PARENT,
        ),
        new_child(
            "topos/engine/src/functors/probes/cpg/taint_walk.rs",
            "GOLD",
            [false, true, true, true],
            19,
            PARENT,
        ),
    ]);
    let clusters = vec![cluster_of(
        PARENT,
        vec![
            child(
                "topos/engine/src/functors/probes/cpg/taint_sinks.rs",
                Reach::Private,
                1,
                0,
            ),
            child(
                "topos/engine/src/functors/probes/cpg/taint_walk.rs",
                Reach::Private,
                1,
                3,
            ),
        ],
        ClusterMark::Fail,
        vec!["parent lost SECURE".to_string()],
        (30, 48),
        (40, 61),
        (400, 520),
        (4, 9),
        Some(ledger(vec![moved(
            PARENT,
            "topos/engine/src/functors/probes/cpg/taint_walk.rs",
            "walk",
            12,
            19,
        )])),
    )];
    recap_of(
        Headline::Regression,
        files,
        clusters,
        Some(rollup("GOLD", "SILVER", true, &["simple", "secure"])),
        scope(3, 2, 0, true),
    )
}

/// `count` near-identical clusters, for the GitHub length budget.
pub(super) fn fixture_many_clusters(count: usize) -> PrRecap {
    let platinum = [true, true, true, true];
    let mut files = Vec::new();
    let mut clusters = Vec::new();
    for index in 0..count {
        let parent = format!("topos/engine/src/functors/profunctors/module_{index:02}/mod.rs");
        files.push(build(Spec {
            path: &parent,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("BRONZE"),
            after: "BRONZE",
            pillars: [
                (false, false, 20.0, 20.0),
                (false, false, 30.0, 30.0),
                (true, true, 100.0, 100.0),
                (false, false, 40.0, 40.0),
            ],
            worst: (80, 40),
            decisions: (60, 62),
            cluster: Some((&parent, ClusterRole::Parent)),
            cosmetic: false,
        }));
        let mut children = Vec::new();
        for slot in 0..8 {
            let path =
                format!("topos/engine/src/functors/profunctors/module_{index:02}/part_{slot}.rs");
            files.push(new_child(&path, "PLATINUM", platinum, 5, &parent));
            children.push(child(&path, Reach::Shared, 3, 4));
        }
        let mut cluster = cluster_of(
            &parent,
            children,
            ClusterMark::Ok,
            Vec::new(),
            (80, 40),
            (60, 62),
            (900, 1100),
            (9, 14),
            None,
        );
        cluster.symbols_moved = (0..30)
            .map(|slot| SymbolMove {
                name: format!("relocatedProfunctorHelper{slot:02}"),
                kind: "Function".to_string(),
                from: parent.clone(),
                to: format!(
                    "topos/engine/src/functors/profunctors/module_{index:02}/part_{}.rs",
                    slot % 8
                ),
            })
            .collect();
        cluster.symbols_new = (0..20)
            .map(|slot| NewSymbol {
                name: format!("freshlyIntroducedBinding{slot:02}"),
                kind: "Function".to_string(),
                file: format!(
                    "topos/engine/src/functors/profunctors/module_{index:02}/part_{}.rs",
                    slot % 8
                ),
            })
            .collect();
        clusters.push(cluster);
    }
    recap_of(
        Headline::Improvement,
        files,
        clusters,
        Some(rollup("BRONZE", "BRONZE", false, &[])),
        scope(count * 9, count * 8, 0, true),
    )
}
