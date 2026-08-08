//! Cross-language CFG edge contracts captured from the recursive builder at
//! `118767b`, before its stack-safe traversal rewrite.

use std::collections::BTreeSet;

use super::{models::EdgeKind, object::ControlFlowGraph};
use crate::graphs::ast::dispatch::parse_source;
use crate::graphs::ast::languages::SUPPORTED_LANGUAGES;

struct Case {
    language: &'static str,
    name: &'static str,
    source: &'static str,
    expected: &'static str,
}

const BRANCH_LOOP: &str = r#"blocks=9 edges=10
edge call_FunctionDecl[] -unconditional-> loop_header[ForStmt]
edge entry[] -unconditional-> call_FunctionDecl[]
edge if_else[] -continue-> loop_header[ForStmt]
edge if_join[] -loop_back-> loop_header[ForStmt]
edge if_then[] -break-> loop_after[ReturnStmt]
edge loop_after[ReturnStmt] -return-> exit[]
edge loop_body[IfStmt] -false-> if_else[]
edge loop_body[IfStmt] -true-> if_then[]
edge loop_header[ForStmt] -false-> loop_after[ReturnStmt]
edge loop_header[ForStmt] -true-> loop_body[IfStmt]"#;

const RUST_BRANCH_LOOP: &str = r#"blocks=14 edges=18
edge call_FunctionDecl[] -unconditional-> nested[]
edge entry[] -unconditional-> call_FunctionDecl[]
edge if_else[] -unconditional-> if_join[]
edge if_else[] -unconditional-> nested[]
edge if_join[] -loop_back-> loop_header[ForStmt]
edge if_then[] -unconditional-> if_join[]
edge if_then[] -unconditional-> nested[]
edge loop_after[] -unconditional-> exit[]
edge loop_after[] -unconditional-> nested[ReturnStmt]
edge loop_body[] -unconditional-> nested[IfStmt]
edge loop_header[ForStmt] -false-> loop_after[]
edge loop_header[ForStmt] -true-> loop_body[]
edge nested[IfStmt] -false-> if_else[]
edge nested[IfStmt] -true-> if_then[]
edge nested[ReturnStmt] -return-> exit[]
edge nested[] -break-> loop_after[]
edge nested[] -continue-> loop_header[ForStmt]
edge nested[] -unconditional-> loop_header[ForStmt]"#;

const MATCH_RETURN: &str = r#"blocks=8 edges=10
edge call_FunctionDecl[MatchStmt] -switch_case-> match_arm[]
edge call_FunctionDecl[MatchStmt] -switch_case-> match_arm[]
edge entry[] -unconditional-> call_FunctionDecl[MatchStmt]
edge match_arm[] -unconditional-> match_join[]
edge match_arm[] -unconditional-> match_join[]
edge match_arm[] -unconditional-> nested[ReturnStmt]
edge match_arm[] -unconditional-> nested[ReturnStmt]
edge match_join[] -unconditional-> exit[]
edge nested[ReturnStmt] -return-> exit[]
edge nested[ReturnStmt] -return-> exit[]"#;

const RUST_MATCH_RETURN: &str = r#"blocks=9 edges=11
edge call_FunctionDecl[] -unconditional-> nested[MatchStmt]
edge entry[] -unconditional-> call_FunctionDecl[]
edge match_arm[] -unconditional-> match_join[]
edge match_arm[] -unconditional-> match_join[]
edge match_arm[] -unconditional-> nested[ReturnStmt]
edge match_arm[] -unconditional-> nested[ReturnStmt]
edge match_join[] -unconditional-> exit[]
edge nested[MatchStmt] -switch_case-> match_arm[]
edge nested[MatchStmt] -switch_case-> match_arm[]
edge nested[ReturnStmt] -return-> exit[]
edge nested[ReturnStmt] -return-> exit[]"#;

const TRY_RETURN: &str = r#"blocks=6 edges=5
edge call_FunctionDecl[TryStmt] -unconditional-> try_body[ReturnStmt]
edge catch_handler[ReturnStmt] -return-> exit[]
edge entry[] -unconditional-> call_FunctionDecl[TryStmt]
edge try_body[ReturnStmt] -exception-> catch_handler[ReturnStmt]
edge try_body[ReturnStmt] -return-> exit[]"#;
const SWITCH_BREAK: &str = r#"blocks=11 edges=14
edge call_FunctionDecl[] -unconditional-> loop_header[ForStmt]
edge entry[] -unconditional-> call_FunctionDecl[]
edge loop_after[ReturnStmt] -return-> exit[]
edge loop_body[MatchStmt] -switch_case-> match_arm[]
edge loop_body[MatchStmt] -switch_case-> match_arm[]
edge loop_header[ForStmt] -false-> loop_after[ReturnStmt]
edge loop_header[ForStmt] -true-> loop_body[MatchStmt]
edge match_arm[] -unconditional-> match_join[]
edge match_arm[] -unconditional-> match_join[]
edge match_arm[] -unconditional-> nested[]
edge match_arm[] -unconditional-> nested[]
edge match_join[] -loop_back-> loop_header[ForStmt]
edge nested[] -break-> match_join[]
edge nested[] -continue-> loop_header[ForStmt]"#;

const TRY_FINALLY: &str = r#"blocks=7 edges=6
edge call_FunctionDecl[TryStmt] -unconditional-> try_body[ReturnStmt]
edge catch_handler[ReturnStmt] -unconditional-> try_finally[]
edge entry[] -unconditional-> call_FunctionDecl[TryStmt]
edge try_body[ReturnStmt] -exception-> catch_handler[ReturnStmt]
edge try_body[ReturnStmt] -unconditional-> try_finally[]
edge try_finally[] -return-> exit[]"#;

const CONDITIONAL_RETURN_FINALLY: &str = r#"blocks=8 edges=10
edge call_FunctionDecl[TryStmt] -unconditional-> try_body[IfStmt]
edge entry[] -unconditional-> call_FunctionDecl[TryStmt]
edge if_join[ExprStmt] -unconditional-> try_finally[ExprStmt]
edge if_then[ReturnStmt] -unconditional-> try_finally[ExprStmt]
edge try_body[IfStmt] -exception-> try_join[ReturnStmt]
edge try_body[IfStmt] -false-> if_join[ExprStmt]
edge try_body[IfStmt] -true-> if_then[ReturnStmt]
edge try_finally[ExprStmt] -return-> exit[]
edge try_finally[ExprStmt] -unconditional-> try_join[ReturnStmt]
edge try_join[ReturnStmt] -return-> exit[]"#;
const RETURN_INSIDE_FINALLY: &str = r#"blocks=6 edges=5
edge call_FunctionDecl[TryStmt] -unconditional-> try_body[ExprStmt]
edge entry[] -unconditional-> call_FunctionDecl[TryStmt]
edge try_body[ExprStmt] -exception-> try_join[]
edge try_body[ExprStmt] -unconditional-> try_finally[ReturnStmt]
edge try_finally[ReturnStmt] -return-> exit[]"#;
const NESTED_FINALLY: &str = r#"blocks=9 edges=8
edge call_FunctionDecl[TryStmt] -unconditional-> try_body[TryStmt]
edge entry[] -unconditional-> call_FunctionDecl[TryStmt]
edge try_body[ReturnStmt] -exception-> try_join[]
edge try_body[ReturnStmt] -unconditional-> try_finally[ExprStmt]
edge try_body[TryStmt] -exception-> try_join[]
edge try_body[TryStmt] -unconditional-> try_body[ReturnStmt]
edge try_finally[ExprStmt] -return-> exit[]
edge try_finally[ExprStmt] -unconditional-> try_finally[ExprStmt]"#;

const CASES: &[Case] = &[
    Case {
        language: "python",
        name: "branch_loop",
        source: "def f(xs):\n    for x in xs:\n        if x > 0:\n            break\n        else:\n            continue\n    return 0\n",
        expected: BRANCH_LOOP,
    },
    Case {
        language: "rust",
        name: "branch_loop",
        source: "fn f(xs: &[i32]) -> i32 {\n    for x in xs {\n        if *x > 0 { break; } else { continue; }\n    }\n    return 0;\n}\n",
        expected: RUST_BRANCH_LOOP,
    },
    Case {
        language: "javascript",
        name: "branch_loop",
        source: "function f(xs) {\n  for (let i = 0; i < xs.length; i++) {\n    if (xs[i] > 0) { break; } else { continue; }\n  }\n  return 0;\n}\n",
        expected: BRANCH_LOOP,
    },
    Case {
        language: "typescript",
        name: "branch_loop",
        source: "function f(xs: number[]): number {\n  for (let i = 0; i < xs.length; i++) {\n    if (xs[i] > 0) { break; } else { continue; }\n  }\n  return 0;\n}\n",
        expected: BRANCH_LOOP,
    },
    Case {
        language: "cpp",
        name: "branch_loop",
        source: "int f(const int* xs, int n) {\n  for (int i = 0; i < n; ++i) {\n    if (xs[i] > 0) { break; } else { continue; }\n  }\n  return 0;\n}\n",
        expected: BRANCH_LOOP,
    },
    Case {
        language: "go",
        name: "branch_loop",
        source: "package p\nfunc f(xs []int) int {\n\tfor _, x := range xs {\n\t\tif x > 0 { break } else { continue }\n\t}\n\treturn 0\n}\n",
        expected: BRANCH_LOOP,
    },
    Case {
        language: "python",
        name: "match_return",
        source: "def f(x):\n    match x:\n        case 0:\n            return 0\n        case _:\n            return 1\n",
        expected: MATCH_RETURN,
    },
    Case {
        language: "rust",
        name: "match_return",
        source: "fn f(x: i32) -> i32 {\n    match x {\n        0 => return 0,\n        _ => return 1,\n    }\n}\n",
        expected: RUST_MATCH_RETURN,
    },
    Case {
        language: "javascript",
        name: "match_return",
        source: "function f(x) {\n  switch (x) {\n    case 0: return 0;\n    default: return 1;\n  }\n}\n",
        expected: MATCH_RETURN,
    },
    Case {
        language: "typescript",
        name: "match_return",
        source: "function f(x: number): number {\n  switch (x) {\n    case 0: return 0;\n    default: return 1;\n  }\n}\n",
        expected: MATCH_RETURN,
    },
    Case {
        language: "cpp",
        name: "match_return",
        source: "int f(int x) {\n  switch (x) {\n    case 0: return 0;\n    default: return 1;\n  }\n}\n",
        expected: MATCH_RETURN,
    },
    Case {
        language: "go",
        name: "match_return",
        source: "package p\nfunc f(x int) int {\n\tswitch x {\n\tcase 0:\n\t\treturn 0\n\tdefault:\n\t\treturn 1\n\t}\n}\n",
        expected: MATCH_RETURN,
    },
    Case {
        language: "python",
        name: "try_return",
        source: "def f():\n    try:\n        return 1\n    except Exception:\n        return 0\n",
        expected: TRY_RETURN,
    },
    Case {
        language: "javascript",
        name: "try_return",
        source: "function f() {\n  try { return 1; } catch (error) { return 0; }\n}\n",
        expected: TRY_RETURN,
    },
    Case {
        language: "typescript",
        name: "try_return",
        source: "function f(): number {\n  try { return 1; } catch (error) { return 0; }\n}\n",
        expected: TRY_RETURN,
    },
    Case {
        language: "cpp",
        name: "try_return",
        source: "int f() {\n  try { return 1; } catch (...) { return 0; }\n}\n",
        expected: TRY_RETURN,
    },

    Case {
        language: "javascript",
        name: "switch_break",
        source: "function f(xs) {
  for (let i = 0; i < xs.length; i++) {
    switch (xs[i]) {
      case 0: break;
      default: continue;
    }
  }
  return 0;
}
",
        expected: SWITCH_BREAK,
    },
    Case {
        language: "typescript",
        name: "switch_break",
        source: "function f(xs: number[]): number {
  for (let i = 0; i < xs.length; i++) {
    switch (xs[i]) {
      case 0: break;
      default: continue;
    }
  }
  return 0;
}
",
        expected: SWITCH_BREAK,
    },
    Case {
        language: "cpp",
        name: "switch_break",
        source: "int f(const int* xs, int n) {
  for (int i = 0; i < n; ++i) {
    switch (xs[i]) {
      case 0: break;
      default: continue;
    }
  }
  return 0;
}
",
        expected: SWITCH_BREAK,
    },
    Case {
        language: "go",
        name: "switch_break",
        source: "package p\nfunc f(xs []int) int {\n\tfor _, x := range xs {\n\t\tswitch x {\n\t\tcase 0:\n\t\t\tbreak\n\t\tdefault:\n\t\t\tcontinue\n\t\t}\n\t}\n\treturn 0\n}\n",
        expected: SWITCH_BREAK,
    },
    Case {
        language: "python",
        name: "try_finally",
        source: "def f():\n    try:\n        return 1\n    except Exception:\n        return 0\n    finally:\n        pass\n",
        expected: TRY_FINALLY,
    },
    Case {
        language: "python",
        name: "conditional_return_finally",
        source: "def f(flag):\n    try:\n        if flag:\n            return 1\n        work()\n    finally:\n        cleanup()\n    return 0\n",
        expected: CONDITIONAL_RETURN_FINALLY,
    },
    Case {
        language: "python",
        name: "return_inside_finally",
        source: "def f():\n    try:\n        work()\n    finally:\n        return 1\n",
        expected: RETURN_INSIDE_FINALLY,
    },
    Case {
        language: "python",
        name: "nested_finally",
        source: "def f():\n    try:\n        try:\n            return 1\n        finally:\n            inner_cleanup()\n    finally:\n        outer_cleanup()\n",
        expected: NESTED_FINALLY,
    },
    Case {
        language: "javascript",
        name: "try_finally",
        source: "function f() { try { return 1; } catch (e) { return 0; } finally {} }\n",
        expected: TRY_FINALLY,
    },
    Case {
        language: "typescript",
        name: "try_finally",
        source: "function f(): number { try { return 1; } catch (e) { return 0; } finally {} }\n",
        expected: TRY_FINALLY,
    },
];

fn block_shape(cfg: &ControlFlowGraph, id: usize) -> String {
    let block = &cfg.blocks[&id];
    let kinds = block
        .statements
        .iter()
        .map(|statement| statement.kind.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!("{}[{kinds}]", block.label)
}

fn normalized_edge_contract(cfg: &ControlFlowGraph) -> String {
    let mut edges = cfg
        .edges
        .iter()
        .map(|edge| {
            format!(
                "edge {} -{}-> {}",
                block_shape(cfg, edge.source),
                edge.kind.label(),
                block_shape(cfg, edge.target)
            )
        })
        .collect::<Vec<_>>();
    edges.sort();
    format!(
        "blocks={} edges={}\n{}",
        cfg.blocks.len(),
        cfg.edges.len(),
        edges.join("\n")
    )
}

fn cfg_for_python_case(name: &str) -> ControlFlowGraph {
    let case = CASES
        .iter()
        .find(|case| case.language == "python" && case.name == name)
        .expect("named Python CFG fixture exists");
    let parsed = parse_source(case.source, case.language, None).expect("fixture parses");
    assert!(!parsed.has_errors, "fixture must parse cleanly");
    ControlFlowGraph::from_uast(&parsed.uast_root)
}

#[test]
fn finally_paths_keep_distinct_completions_and_frames() {
    let conditional = cfg_for_python_case("conditional_return_finally");
    let conditional_finally = conditional
        .blocks
        .values()
        .find(|block| block.label == "try_finally")
        .expect("finally block exists")
        .id;
    assert!(conditional
        .edges
        .iter()
        .any(|edge| { edge.source == conditional_finally && edge.kind == EdgeKind::Return }));
    assert!(conditional.edges.iter().any(|edge| {
        edge.source == conditional_finally && edge.kind == EdgeKind::Unconditional
    }));

    let returning = cfg_for_python_case("return_inside_finally");
    assert!(returning
        .edges
        .iter()
        .all(|edge| edge.source != edge.target));

    let nested = cfg_for_python_case("nested_finally");
    let finally_ids = nested
        .blocks
        .values()
        .filter(|block| block.label == "try_finally")
        .map(|block| block.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(finally_ids.len(), 2);
    let inner_to_outer = nested
        .edges
        .iter()
        .find(|edge| {
            finally_ids.contains(&edge.source)
                && finally_ids.contains(&edge.target)
                && edge.source != edge.target
                && edge.kind == EdgeKind::Unconditional
        })
        .expect("inner finally flows through distinct outer finally");
    assert!(nested.edges.iter().any(|edge| {
        edge.source == inner_to_outer.target
            && edge.target == nested.exit_id
            && edge.kind == EdgeKind::Return
    }));
}

#[test]
fn preserves_pre_refactor_edge_contracts_across_supported_languages() {
    for case in CASES {
        let parsed = parse_source(case.source, case.language, None)
            .unwrap_or_else(|error| panic!("[{}/{}] {error}", case.language, case.name));
        assert!(
            !parsed.has_errors,
            "[{}/{}] fixture must parse cleanly",
            case.language, case.name
        );

        let cfg = ControlFlowGraph::from_uast(&parsed.uast_root);
        assert_eq!(
            normalized_edge_contract(&cfg),
            case.expected,
            "[{}/{}] CFG edge contract changed",
            case.language,
            case.name
        );
    }
}

#[test]
fn edge_contracts_cover_every_supported_language() {
    let covered = CASES
        .iter()
        .map(|case| case.language)
        .collect::<BTreeSet<_>>();
    let supported = SUPPORTED_LANGUAGES.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(covered, supported);
}
