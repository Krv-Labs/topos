//! Inline test detection for structural coverage (#336).
//!
//! Splits a source file into its program-under-test UAST and the test
//! code that lives inside it, so coverage counts tests that sit next to
//! the code they exercise:
//!
//! - Rust: items annotated `#[cfg(test)]` (typically `mod tests`) and
//!   `#[test]` / `#[<path>::test]` functions.
//! - Go: `*_test.go` files and `Test*` / `Benchmark*` / `Example*` /
//!   `Fuzz*` functions.
//! - Python: `test_*.py` / `*_test.py` files, `test_*` functions, and
//!   `Test*` classes.
//!
//! Other languages keep the whole file on the program-under-test side.

use std::path::Path;

use crate::graphs::ast::dispatch::{parse_source, DispatchError};
use crate::graphs::uast::mapper_rust::map_rust_tree_to_uast_with_tests;
use crate::graphs::uast::models::UASTNode;

const GO_TEST_PREFIXES: &[&str] = &["Test", "Benchmark", "Example", "Fuzz"];

/// Parse `source` and split it into `(program_under_test, inline_tests)`.
///
/// The program side is `None` when the whole file is a test file
/// (`*_test.go`, `test_*.py`, `*_test.py`). Each inline test is a
/// detached UAST subtree, ready to pass as a test root to
/// [`super::structural_test_coverage::declaration_coverage`].
pub fn parse_with_inline_tests(
    source: &str,
    language: &str,
    file: Option<&str>,
) -> Result<(Option<UASTNode>, Vec<UASTNode>), DispatchError> {
    let root = parse_coverage_root(source, language, file)?;
    if file.is_some_and(|f| is_test_file(f, language)) {
        return Ok((None, vec![root]));
    }
    let (program, tests) = split_tests(root, source, language);
    Ok((Some(program), tests))
}

/// Parse a coverage input without discarding test-only Rust items.
/// Explicit test inputs keep the entire returned root on the test side.
pub fn parse_coverage_root(
    source: &str,
    language: &str,
    file: Option<&str>,
) -> Result<UASTNode, DispatchError> {
    let parsed = parse_source(source, language, file)?;
    Ok(if language == "rust" {
        // The default Rust mapper discards `#[cfg(test)]` items; keep them
        // here so they land on the test side instead of nowhere.
        map_rust_tree_to_uast_with_tests(parsed.tree.root_node(), source.as_bytes(), file)
    } else {
        parsed.uast_root
    })
}

fn is_test_file(file: &str, language: &str) -> bool {
    let name = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    match language {
        "go" => name.ends_with("_test.go"),
        "python" => {
            (name.starts_with("test_") && name.ends_with(".py")) || name.ends_with("_test.py")
        }
        _ => false,
    }
}

fn node_text<'s>(node: &UASTNode, source: &'s str) -> &'s str {
    source
        .get(node.span.start_byte..node.span.end_byte)
        .unwrap_or("")
}

/// Go/Python: a test is recognized by its declaration name alone.
fn is_named_test(node: &UASTNode, source: &str, language: &str) -> bool {
    let prefixes: &[&str] = match (language, node.native.node_kind.as_str()) {
        ("go", "function_declaration") => GO_TEST_PREFIXES,
        ("python", "function_definition") => &["test_"],
        ("python", "class_definition") => &["Test"],
        _ => return false,
    };
    let name = node
        .children
        .iter()
        .find(|c| c.native.node_kind == "identifier")
        .map_or("", |c| node_text(c, source));
    prefixes.iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            // Go's rule: `TestFoo` / `Test` is a test, `Testify` is not.
            language != "go" || !rest.starts_with(|c: char| c.is_lowercase())
        })
    })
}

/// Detach test subtrees from `root`, returning `(program, tests)`.
///
/// Iterative so deep trees can't overflow the stack; stops descending at
/// function/method declarations (tests are not declared inside bodies).
fn split_tests(mut root: UASTNode, source: &str, language: &str) -> (UASTNode, Vec<UASTNode>) {
    let mut tests = Vec::new();
    let mut stack: Vec<&mut UASTNode> = vec![&mut root];
    while let Some(node) = stack.pop() {
        let mut pending_rust_test = false;
        let mut kept = Vec::with_capacity(node.children.len());
        for child in std::mem::take(&mut node.children) {
            let is_test = if language != "rust" {
                is_named_test(&child, source, language)
            } else if child.native.node_kind == "attribute_item" {
                // An attribute is a preceding sibling of the item it marks.
                if child.attributes.contains_key("rustTestAttribute") {
                    pending_rust_test = true;
                    continue;
                }
                false
            } else if matches!(
                child.native.node_kind.as_str(),
                "line_comment" | "block_comment"
            ) {
                false
            } else {
                std::mem::take(&mut pending_rust_test)
            };
            if is_test {
                tests.push(child);
            } else {
                kept.push(child);
            }
        }
        node.children = kept;
        stack.extend(
            node.children
                .iter_mut()
                .filter(|c| !matches!(c.kind.as_str(), "FunctionDecl" | "MethodDecl")),
        );
    }
    (root, tests)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functors::profunctors::uast::structural_test_coverage::extract_declarations;

    fn decl_counts(source: &str, language: &str, file: &str) -> (usize, usize) {
        let (program, tests) = parse_with_inline_tests(source, language, Some(file)).unwrap();
        let program = program.map_or(0, |p| extract_declarations(&p).len());
        let tests = tests.iter().map(|t| extract_declarations(t).len()).sum();
        (program, tests)
    }

    #[test]
    fn rust_cfg_test_module_and_test_fns_are_tests() {
        let source = "fn add(a: i32, b: i32) -> i32 { a + b }\n\
            #[test]\nfn bare() { assert_eq!(add(1, 1), 2); }\n\
            #[tokio::test(flavor = \"current_thread\")]\nasync fn async_case() {}\n\
            #[cfg(test)]\nmod tests {\n    use super::*;\n    fn helper() -> i32 { 1 }\n    \
            #[test]\n    fn it_adds() { assert_eq!(add(helper(), 1), 2); }\n}\n";
        assert_eq!(decl_counts(source, "rust", "lib.rs"), (1, 4));
    }

    #[test]
    fn rust_non_test_attributes_stay_on_the_program_side() {
        let source = "#[inline]\nfn fast() -> i32 { 1 }\n#[cfg(not(test))]\nfn real() {}\n";
        assert_eq!(decl_counts(source, "rust", "lib.rs"), (2, 0));
    }

    #[test]
    fn rust_attributes_allow_comments_spacing_and_other_attributes() {
        let source = r#"
            fn prod() {}
            #[cfg( /* condition */ test )]
            // module documentation
            #[allow(dead_code)]
            mod tests { fn helper() {} }
            #[test]
            /* explanation */
            #[ignore]
            fn checks() {}
            #[tokio :: test(flavor = "current_thread")]
            // async documentation
            async fn async_checks() {}
        "#;
        assert_eq!(decl_counts(source, "rust", "lib.rs"), (1, 3));
    }

    #[test]
    fn rust_attribute_strings_and_other_cfg_conditions_are_not_tests() {
        let source = r#"
            #[doc = "cfg(test)"]
            fn documented() {}
            #[cfg(not(test))]
            fn production() {}
            #[cfg(feature = "cfg(test)")]
            fn feature() {}
            #[cfg(any(test, feature = "dev"))]
            fn shared() {}
        "#;
        assert_eq!(decl_counts(source, "rust", "lib.rs"), (4, 0));
    }

    #[test]
    fn explicit_rust_tests_preserve_cfg_modules_and_helpers() {
        let source = "#[cfg( test )]\nmod tests { #[test]\nfn checks() {} }\nfn helper() {}";
        let root = parse_coverage_root(source, "rust", Some("tests/checks.rs")).unwrap();
        assert_eq!(extract_declarations(&root).len(), 2);
    }

    #[test]
    fn go_test_files_and_test_functions_are_tests() {
        let lib = "package m\nfunc Add(a, b int) int { return a + b }\n\
            func Testify() {}\n\
            func TestAdd(t *testing.T) { Add(1, 2) }\n\
            func BenchmarkAdd(b *testing.B) {}\nfunc ExampleAdd() {}\nfunc FuzzAdd(f *testing.F) {}\n";
        assert_eq!(decl_counts(lib, "go", "m.go"), (2, 4));
        let test_file = "package m\nfunc helper() int { return 1 }\n";
        assert_eq!(decl_counts(test_file, "go", "m_test.go"), (0, 1));
    }

    #[test]
    fn python_test_functions_classes_and_files_are_tests() {
        let lib = "def add(a, b):\n    return a + b\n\n\
            def test_add():\n    assert add(1, 1) == 2\n\n\
            class TestAdd:\n    def test_zero(self):\n        assert add(0, 0) == 0\n\n\
            class Adder:\n    def run(self):\n        return add(1, 2)\n";
        assert_eq!(decl_counts(lib, "python", "lib.py"), (2, 2));
        let helper = "def helper():\n    return 1\n";
        assert_eq!(decl_counts(helper, "python", "test_lib.py"), (0, 1));
        assert_eq!(decl_counts(helper, "python", "lib_test.py"), (0, 1));
    }

    #[test]
    fn other_languages_keep_everything_on_the_program_side() {
        let source = "function testValue() { return 1; }\n";
        assert_eq!(decl_counts(source, "javascript", "lib.test.js"), (1, 0));
    }
}
