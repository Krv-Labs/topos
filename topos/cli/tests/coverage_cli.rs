//! Verify the rendered coverage verdict and corpus counts through the CLI.
use std::process::{Command, Output};

fn coverage(files: &[(&str, &str)], args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        std::fs::write(dir.path().join(path), source).unwrap();
    }
    Command::new(env!("CARGO_BIN_EXE_topos"))
        .current_dir(dir.path())
        .env("TOPOS_NO_UPDATE_NOTICES", "1")
        .arg("coverage")
        .args(args)
        .output()
        .unwrap()
}

fn successful_text(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn rust_tests_count_equally_inline_and_explicit() {
    let program = "fn value() -> i32 { 1 }\n";
    let tests = "#[cfg( /* condition */ test )]\n// explanation\n#[allow(dead_code)]\nmod tests { #[test]\nfn checks() { assert_eq!(1, 1); } }";
    let mixed = format!("{program}{tests}");
    let inline = successful_text(coverage(
        &[("lib.rs", &mixed)],
        &["lib.rs", "--language", "rust"],
    ));
    let explicit = successful_text(coverage(
        &[("lib.rs", program), ("checks.rs", tests)],
        &["lib.rs", "--tests", "checks.rs", "--language", "rust"],
    ));
    for text in [&inline, &explicit] {
        assert!(text.contains("1 source · 1 test"), "{text}");
        assert!(text.contains("PASS"), "{text}");
    }
}

#[test]
fn rust_attribute_strings_do_not_supply_tests() {
    let text = successful_text(coverage(
        &[("lib.rs", "#[doc = \"cfg(test)\"]\nfn value() {}")],
        &["lib.rs", "--language", "rust"],
    ));
    assert!(text.contains("1 source · 0 test"), "{text}");
    assert!(text.contains("INCONCLUSIVE"), "{text}");
}

#[test]
fn test_only_source_is_rejected() {
    let output = coverage(
        &[("lib.rs", "#[test]\n// explanation\nfn checks() {}")],
        &["lib.rs", "--language", "rust"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("no function or method declarations found"),
        "{stderr}"
    );
}

#[test]
fn threshold_changes_pass_fail_and_missing_tests_are_inconclusive() {
    let files = [
        (
            "lib.py",
            "def value(n):\n    while n:\n        if n > 2:\n            break\n    return n\n",
        ),
        ("test_lib.py", "def test_value():\n    pass\n"),
    ];
    for (threshold, verdict) in [("0", "PASS"), ("1", "FAIL")] {
        let text = successful_text(coverage(
            &files,
            &[
                "lib.py",
                "--tests",
                "test_lib.py",
                "--language",
                "python",
                "--coverage-threshold",
                threshold,
            ],
        ));
        assert!(text.contains(verdict), "{text}");
    }
    let text = successful_text(coverage(&files, &["lib.py", "--language", "python"]));
    assert!(text.contains("INCONCLUSIVE"), "{text}");
}

#[test]
fn go_and_python_inline_counts_are_preserved() {
    for (file, language, source) in [
        (
            "lib.go",
            "go",
            "package m\nfunc Add() int { return 1 }\nfunc TestAdd() { Add() }",
        ),
        (
            "lib.py",
            "python",
            "def add():\n    return 1\ndef test_add():\n    assert add() == 1\n",
        ),
    ] {
        let text = successful_text(coverage(&[(file, source)], &[file, "--language", language]));
        assert!(text.contains("1 source · 1 test"), "{text}");
    }
}
