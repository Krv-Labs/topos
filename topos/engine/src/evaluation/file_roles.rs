//! File roles — predicates that classify what *role* a source file
//! plays, so the characteristic morphism can relax or skip specific
//! quality gates when a file is structurally special rather than
//! ordinary logic.
//!
//! The first role is the **import/export-only entrypoint module** —
//! `__init__.py`, `mod.rs`/`lib.rs`, `index.ts`, and friends — which are
//! trivial re-export hubs and should not be penalized for low entropy
//! or high instability. Test files ([`is_test_module`]) and literal-data
//! files ([`is_data_module`]) are the roles `pr-recap` reads to soften a
//! SIMPLE loss caused by low entropy alone, where repeated text is the
//! file's job rather than a smell.

use std::path::Path;

use crate::core::morphism::ProgramMorphism;
use crate::functors::probes::ast::complexity::calculate_file_complexity;
use crate::functors::profunctors::uast::inline_tests::parse_with_inline_tests;
use crate::graphs::uast::models::UASTNode;

/// True iff `morphism` is an import/export-only entrypoint module.
pub fn is_entrypoint_module(morphism: &ProgramMorphism) -> bool {
    let Some(filepath) = morphism.filepath.as_deref() else {
        return false;
    };
    if !entrypoint_filename_hint(filepath, &morphism.language) {
        return false;
    }
    is_entrypoint_source_only(&morphism.source, &morphism.language)
}

/// True iff `morphism` is a declarations-only module with no executable
/// control/call flow -- Martin's accepted "Zone of Pain" exception for
/// frozen, rarely-changing foundation/utility code (constants, error
/// types).
///
/// Deliberately filename-agnostic (unlike [`is_entrypoint_module`]): the
/// whole point of pairing Abstractness with Instability is to avoid
/// per-language filename lists, and that principle extends here where
/// possible.
pub fn is_stable_leaf_module(morphism: &ProgramMorphism) -> bool {
    let Some(uast_root) = morphism.ast.as_ref().map(|ast| &ast.uast_root) else {
        return false;
    };
    use crate::functors::probes::uast::signature::control_flow_profile;

    let profile = control_flow_profile(uast_root);
    const DISQUALIFYING_KINDS: &[&str] = &[
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "TryStmt",
        "CallExpr",
        "FunctionDecl",
        "MethodDecl",
    ];
    DISQUALIFYING_KINDS
        .iter()
        .all(|kind| *profile.get(*kind).unwrap_or(&0) == 0)
}

/// A test by the common layout conventions: under a `tests`, `test` or
/// `__tests__` directory, or named `*_test.*` (Go, Rust, Python),
/// `test_*.py` (pytest), `*.test.*` or `*.spec.*` (Jest, Vitest, Mocha).
/// `path` is repository-relative and `/`-separated.
pub fn is_test_path(path: &str) -> bool {
    let mut segments: Vec<&str> = path.split('/').collect();
    let name = segments.pop().unwrap_or_default();
    if segments
        .iter()
        .any(|dir| matches!(*dir, "tests" | "test" | "__tests__"))
    {
        return true;
    }
    let stem = name.split('.').next().unwrap_or_default();
    stem.ends_with("_test")
        || (name.starts_with("test_") && name.ends_with(".py"))
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// True iff `morphism` is a test file: its path follows [`is_test_path`],
/// or more than half of its non-blank text is inline tests as structural
/// coverage recognizes them (Rust `#[cfg(test)]`/`#[test]` items, Go
/// `Test*` functions, Python `test_*` functions and `Test*` classes).
pub fn is_test_module(morphism: &ProgramMorphism) -> bool {
    let Some(path) = morphism.filepath.as_deref() else {
        return false;
    };
    let path = path.to_string_lossy().replace('\\', "/");
    if is_test_path(&path) {
        return true;
    }
    let total = morphism.source.trim().len();
    let Ok((_, tests)) = parse_with_inline_tests(&morphism.source, &morphism.language, Some(&path))
    else {
        return false;
    };
    let tested: usize = tests.iter().map(span_len).sum();
    total > 0 && 2 * tested > total
}

/// Native syntax-tree kinds of a literal collection: Python dict, list,
/// tuple and set displays, JavaScript/TypeScript object and array literals,
/// Rust array, tuple and struct expressions, Go composite literals and C++
/// brace initializers.
const DATA_LITERAL_KINDS: &[&str] = &[
    "dictionary",
    "list",
    "tuple",
    "set",
    "object",
    "array",
    "array_expression",
    "tuple_expression",
    "struct_expression",
    "composite_literal",
    "initializer_list",
];

/// True iff `morphism` is a literal-data file: it parsed, it makes no
/// decision anywhere (whole-file cyclomatic complexity 1, so no branch,
/// loop, comprehension or short-circuit), and more than half of its
/// non-blank text sits inside literal collections ([`DATA_LITERAL_KINDS`]).
/// A constants table, a fixture dict or a strings/i18n object qualifies; a
/// class of trivial accessors does not, because its text is code.
pub fn is_data_module(morphism: &ProgramMorphism) -> bool {
    let Some(ast) = morphism.ast.as_ref().filter(|_| morphism.is_valid()) else {
        return false;
    };
    let total = morphism.source.trim().len();
    if total == 0 || calculate_file_complexity(&ast.uast_root) > 1 {
        return false;
    }
    2 * literal_bytes(&ast.uast_root) > total
}

fn span_len(node: &UASTNode) -> usize {
    node.span.end_byte.saturating_sub(node.span.start_byte)
}

/// Bytes covered by the outermost literal collections under `root`.
fn literal_bytes(root: &UASTNode) -> usize {
    let mut bytes = 0;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if DATA_LITERAL_KINDS.contains(&node.native.node_kind.as_str()) {
            bytes += span_len(node);
        } else {
            stack.extend(node.children.iter());
        }
    }
    bytes
}

fn entrypoint_filename_hint(path: &Path, language: &str) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower_name = filename.to_lowercase();
    match language {
        "python" => filename == "__init__.py",
        "rust" => matches!(filename, "mod.rs" | "lib.rs"),
        "typescript" => matches!(lower_name.as_str(), "index.ts" | "index.tsx"),
        "javascript" => matches!(lower_name.as_str(), "index.js" | "index.mjs" | "index.cjs"),
        "cpp" => {
            let suffix = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            matches!(suffix.as_str(), "hpp" | "hh" | "hxx")
        }
        _ => false,
    }
}

const COMMENT_PREFIXES: [&str; 3] = ["//", "/*", "*"];

fn is_comment_line(line: &str) -> bool {
    COMMENT_PREFIXES.iter().any(|p| line.starts_with(p)) || line.ends_with("*/")
}

const TS_JS_DECL_PREFIXES: [&str; 6] = [
    "import ",
    "export *",
    "export {",
    "export type ",
    "export interface ",
    "#!",
];
const RUST_DECL_PREFIXES: [&str; 7] = [
    "use ",
    "pub use ",
    "pub mod ",
    "mod ",
    "extern crate ",
    "#!",
    "#[",
];
const CPP_DECL_PREFIXES: [&str; 2] = ["#include", "#pragma once"];

fn is_entrypoint_source_only(source: &str, language: &str) -> bool {
    let lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }

    let decl_prefixes: &[&str] = match language {
        "python" => return python_entrypoint_only(&lines),
        "typescript" | "javascript" => &TS_JS_DECL_PREFIXES,
        "rust" => &RUST_DECL_PREFIXES,
        "cpp" => &CPP_DECL_PREFIXES,
        _ => return false,
    };

    lines
        .iter()
        .all(|line| is_comment_line(line) || decl_prefixes.iter().any(|p| line.starts_with(p)))
}

/// Tracks open brackets so multiline `from x import (...)` and
/// `__all__ = [...]` continuation lines (e.g. `assess,`) are accepted.
fn python_entrypoint_only(lines: &[&str]) -> bool {
    let mut depth: i32 = 0;
    for &line in lines {
        if depth > 0 {
            depth += bracket_delta(line);
            continue;
        }
        let is_allowed = line.starts_with('#')
            || line.starts_with("import ")
            || line.starts_with("from ")
            || line.starts_with("__all__")
            || matches!(line, "[" | "]" | "(" | ")")
            || line.starts_with('\'')
            || line.starts_with('"');
        if !is_allowed {
            return false;
        }
        if line.starts_with("import ") || line.starts_with("from ") || line.starts_with("__all__") {
            depth += bracket_delta(line);
        }
    }
    true
}

fn bracket_delta(line: &str) -> i32 {
    let opens = line.matches('(').count() + line.matches('[').count();
    let closes = line.matches(')').count() + line.matches(']').count();
    opens as i32 - closes as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_init_with_only_imports_is_entrypoint() {
        let mut morphism =
            ProgramMorphism::new("from foo import bar\n__all__ = [\"bar\"]\n", "python");
        morphism.filepath = Some("/pkg/__init__.py".into());
        assert!(is_entrypoint_module(&morphism));
    }

    #[test]
    fn python_init_with_real_logic_is_not_entrypoint() {
        let mut morphism = ProgramMorphism::new("def f():\n    return 1\n", "python");
        morphism.filepath = Some("/pkg/__init__.py".into());
        assert!(!is_entrypoint_module(&morphism));
    }

    #[test]
    fn non_entrypoint_filename_is_never_entrypoint() {
        let mut morphism = ProgramMorphism::new("import foo\n", "python");
        morphism.filepath = Some("/pkg/util.py".into());
        assert!(!is_entrypoint_module(&morphism));
    }

    fn at(source: &str, language: &str, path: &str) -> ProgramMorphism {
        ProgramMorphism::with_path(source, language, path)
    }

    #[test]
    fn test_paths_follow_the_common_conventions() {
        for path in [
            "tests/cli.rs",
            "src/test/Main.go",
            "web/__tests__/app.tsx",
            "pkg/run_test.go",
            "tools/test_split.py",
            "tools/split_test.py",
            "web/app.test.ts",
            "web/app.spec.js",
        ] {
            assert!(is_test_path(path), "{path}");
        }
        for path in [
            "src/testing.rs",
            "src/contest.py",
            "test_data.rs",
            "src/latest/app.ts",
        ] {
            assert!(!is_test_path(path), "{path}");
        }
    }

    #[test]
    fn a_source_file_that_is_mostly_inline_tests_is_a_test_module() {
        let cases: String = (0..20)
            .map(|i| {
                format!("    #[test]\n    fn case_{i}() {{ assert_eq!(add({i}, 1), {i} + 1); }}\n")
            })
            .collect();
        let mostly_tests = format!(
            "pub fn add(a: i32, b: i32) -> i32 {{ a + b }}\n#[cfg(test)]\nmod tests {{\n    use super::*;\n{cases}}}\n"
        );
        assert!(is_test_module(&at(&mostly_tests, "rust", "src/add.rs")));

        let mostly_code: String = (0..20)
            .map(|i| format!("pub fn f{i}(x: i32) -> i32 {{ x * {i} + {i} }}\n"))
            .collect::<String>()
            + "#[cfg(test)]\nmod tests {\n    #[test]\n    fn one() {}\n}\n";
        assert!(!is_test_module(&at(&mostly_code, "rust", "src/add.rs")));

        let python = (0..20)
            .map(|i| format!("def test_case_{i}():\n    assert {i} + 1 == {}\n\n", i + 1))
            .collect::<String>();
        assert!(is_test_module(&at(&python, "python", "app/checks.py")));
        assert!(is_test_module(&at(
            "x = 1\n",
            "python",
            "tests/conftest.py"
        )));
        assert!(!is_test_module(&at("x = 1\n", "python", "app/conf.py")));
    }

    #[test]
    fn literal_tables_are_data_modules() {
        let entries: String = (0..50)
            .map(|i| format!("    \"key{i}\": \"value number {i}\",\n"))
            .collect();
        let python = format!("CODES = {{\n{entries}}}\n");
        assert!(is_data_module(&at(&python, "python", "app/codes.py")));
        let ts = format!("export const strings = {{\n{entries}}};\n").replace('"', "'");
        assert!(is_data_module(&at(&ts, "typescript", "src/en.ts")));
        let pairs: String = (0..50)
            .map(|i| format!("    (\"key{i}\", \"value {i}\"),\n"))
            .collect();
        let rust = format!("pub const PAIRS: &[(&str, &str)] = &[\n{pairs}];\n");
        assert!(is_data_module(&at(&rust, "rust", "src/pairs.rs")));
        let go = format!(
            "package data\n\nvar Codes = map[string]string{{\n{}}}\n",
            entries
        );
        assert!(is_data_module(&at(&go, "go", "data/codes.go")));
    }

    #[test]
    fn code_is_not_a_data_module() {
        let entries: String = (0..50)
            .map(|i| format!("    \"key{i}\": \"value number {i}\",\n"))
            .collect();
        let branching = format!(
            "CODES = {{\n{entries}}}\n\ndef lookup(key):\n    if key in CODES:\n        return CODES[key]\n    return None\n"
        );
        assert!(!is_data_module(&at(&branching, "python", "app/codes.py")));

        let accessors: String = (0..50)
            .map(|i| {
                format!(
                    "    def get_field_{i}(self):\n        return self._fields[\"field_{i}\"]\n\n"
                )
            })
            .collect();
        let class = format!("class Record:\n{accessors}");
        assert!(!is_data_module(&at(&class, "python", "app/model.py")));
        assert!(!is_data_module(&at("", "python", "app/empty.py")));
        assert!(!is_data_module(&at(
            "x = [1, 2\n",
            "python",
            "app/broken.py"
        )));
    }

    #[test]
    fn rust_lib_rs_with_only_mod_declarations_is_entrypoint() {
        let mut morphism = ProgramMorphism::new("pub mod core;\npub mod graphs;\n", "rust");
        morphism.filepath = Some("/crate/src/lib.rs".into());
        assert!(is_entrypoint_module(&morphism));
    }
}
