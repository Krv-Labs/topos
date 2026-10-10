//! Exercise coverage parsing and verdicts through the actual MCP protocol.
use std::io::Write;
use std::process::{Command, Stdio};

fn coverage(files: &[(&str, &str)], arguments: serde_json::Value) -> serde_json::Value {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    for (path, source) in files {
        std::fs::write(dir.path().join(path), source).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_topos-mcp"))
        .current_dir(dir.path())
        .env("TOPOS_MCP_FILE_ROOT", dir.path())
        .env("TOPOS_NO_UPDATE_NOTICES", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for frame in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"coverage-test","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"topos_calculate_coverage","arguments":arguments}}),
    ] {
        writeln!(stdin, "{frame}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let responses = String::from_utf8(output.stdout).unwrap();
    let response = responses
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|v| v["id"] == 2)
        .unwrap_or_else(|| {
            panic!(
                "No coverage response: {responses}; stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert!(response["error"].is_null(), "{response}");
    let result = response["result"]["structuredContent"].clone();
    assert!(result.is_object(), "{response}");
    result
}

#[test]
fn rust_tests_count_equally_inline_and_explicit() {
    let program = "fn value() -> i32 { 1 }\n";
    let tests = "#[cfg( /* condition */ test )]\n// explanation\n#[allow(dead_code)]\nmod tests { #[test]\nfn checks() { assert_eq!(1, 1); } }";
    let mixed = format!("{program}{tests}");
    let inline = coverage(
        &[("lib.rs", &mixed)],
        serde_json::json!({
        "put_files":["lib.rs"],"language":"rust"}),
    );
    let explicit = coverage(
        &[("lib.rs", program), ("checks.rs", tests)],
        serde_json::json!({
        "put_files":["lib.rs"],"test_files":["checks.rs"],"language":"rust"}),
    );
    for result in [&inline, &explicit] {
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["put_declaration_count"], 1);
        assert_eq!(result["test_declaration_count"], 1);
    }
    assert_eq!(
        inline["mean_declaration_coverage"],
        explicit["mean_declaration_coverage"]
    );
    assert_eq!(inline["verdict"], explicit["verdict"]);
}

#[test]
fn empty_source_corpora_are_errors_without_a_verdict() {
    for (file, language, source) in [
        ("lib.rs", "rust", "#[test]\nfn checks() {}"),
        (
            "lib.rs",
            "rust",
            "#[cfg(test)]\nmod tests { fn checks() {} }",
        ),
        ("lib.py", "python", "def test_checks():\n    pass\n"),
        ("test_lib.py", "python", "def helper():\n    pass\n"),
        ("lib_test.go", "go", "package m\nfunc TestChecks() {}"),
        ("lib.rs", "rust", ""),
    ] {
        let result = coverage(
            &[(file, source)],
            serde_json::json!({
            "put_files":[file],"language":language}),
        );
        assert!(result["error"].is_string(), "{result}");
        assert!(result["verdict"].is_null(), "{result}");
        assert_ne!(result["mean_declaration_coverage"], 1.0);
    }
}

#[test]
fn declaration_free_file_does_not_hide_a_valid_program() {
    let result = coverage(
        &[
            ("empty.rs", ""),
            (
                "lib.rs",
                "fn value() -> i32 { 1 }\n#[test]\nfn checks() { assert_eq!(1, 1); }",
            ),
        ],
        serde_json::json!({"put_files":["empty.rs","lib.rs"],"language":"rust"}),
    );
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(result["put_declaration_count"], 1);
    assert_eq!(result["test_declaration_count"], 1);
}

#[test]
fn threshold_changes_pass_fail_and_missing_tests_are_inconclusive() {
    let program =
        "def value(n):\n    while n:\n        if n > 2:\n            break\n    return n\n";
    let files = [
        ("lib.py", program),
        ("test_lib.py", "def test_value():\n    pass\n"),
    ];
    for (threshold, verdict) in [(0.0, "PASS"), (1.0, "FAIL")] {
        let result = coverage(
            &files,
            serde_json::json!({
            "put_files":["lib.py"],"test_files":["test_lib.py"],
            "language":"python","coverage_threshold":threshold}),
        );
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["verdict"], verdict);
    }
    let result = coverage(
        &files,
        serde_json::json!({"put_files":["lib.py"],"language":"python"}),
    );
    assert_eq!(result["verdict"], "INCONCLUSIVE");
    assert!(!result["warnings"].as_array().unwrap().is_empty());
}
