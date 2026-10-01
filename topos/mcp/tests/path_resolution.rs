//! Protocol-level regression tests for file-path resolution.
//!
//! The bug this exists for: `resolve_project_path` refused every relative path
//! unless `TOPOS_MCP_FILE_ROOT` was set, and `topos install` never sets it. So
//! an agent that wrote `src/lib.rs` instead of `/Users/you/repo/src/lib.rs` got
//! "an absolute path is required" — and `topos_begin_refactor`,
//! `topos_assess_changeset` and the snapshot loop were unusable on any host that
//! had not hand-configured the variable.
//!
//! The unit tests in `security.rs` cover the resolution rules, but they cannot
//! catch a tool that passes its raw parameter somewhere that bypasses the
//! guard: there are 46 call sites across six tool modules. These tests drive
//! the **built binary over real JSON-RPC**, with a controlled cwd, so the only
//! thing that can make them pass is the whole path from wire to resolver.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Exchange frames with a server started in `cwd`, and return its responses.
fn exchange_in(cwd: &Path, frames: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_topos-mcp"))
        .current_dir(cwd)
        // A hermetic env. Without these the test inherits the developer's own
        // boundary, their update-check cache, and a live network call — the
        // first version of this file did, and a stale `update-check.json` on
        // the machine made an unrelated release banner appear in the output.
        .env_remove("TOPOS_MCP_FILE_ROOT")
        .env("TOPOS_NO_UPDATE_NOTICES", "1")
        .env("XDG_STATE_HOME", cwd.join(".state"))
        .env("HOME", cwd.join(".state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn topos-mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    for frame in frames {
        writeln!(stdin, "{frame}").expect("write frame");
    }
    drop(stdin);

    let out = child.wait_with_output().expect("wait for topos-mcp");
    String::from_utf8(out.stdout)
        .expect("utf-8 stdout")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response line"))
        .collect()
}

/// A scratch project: a git repo with one Rust file at a known relative path.
struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("topos-mcp-path-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
        Self { dir }
    }

    fn file(&self) -> PathBuf {
        self.dir.join("src/lib.rs")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const READ_BUILD_RESOURCE: &str =
    r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"topos://build"}}"#;

fn initialize(id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"initialize","params":{{"protocolVersion":"2024-11-05","capabilities":{{}},"clientInfo":{{"name":"path-test","version":"1"}}}}}}"#
    )
}

fn call_inspect(id: u64, filepath: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"topos_inspect_code","arguments":{{"filepath":{filepath},"no_composable":true}}}}}}"#
    )
}

/// The `warnings` list from a tool result's structured channel.
///
/// `topos_inspect_code` nests the evaluation inside its result, so this looks in
/// both places rather than hard-coding one tool's shape.
fn warnings_of(response: &serde_json::Value) -> String {
    let structured = response
        .get("structuredContent")
        .unwrap_or_else(|| panic!("no structuredContent in {response:#?}"));
    for candidate in [
        structured.get("warnings"),
        structured["evaluation"].get("warnings"),
    ] {
        if let Some(entries) = candidate.and_then(|w| w.as_array()) {
            return entries
                .iter()
                .filter_map(|w| w.as_str())
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    panic!("no warnings array in {structured:#?}");
}

fn text_of(response: &serde_json::Value) -> String {
    response["content"][0]["text"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| panic!("RAW: {response:#?}"))
}

/// The bug: a relative path over the wire was rejected outright.
#[test]
fn a_relative_path_resolves_over_the_wire() {
    let project = Project::new("relative");
    let responses = exchange_in(
        &project.dir,
        &[&initialize(1), &call_inspect(2, "\"src/lib.rs\"")],
    );

    let text = text_of(result(&responses, 2));
    assert!(
        !text.contains("absolute file or directory path is required"),
        "the reported bug must not reproduce:\n{text}"
    );
    assert!(
        text.contains("Total functions") || text.contains("Lattice"),
        "expected a real inspection, got:\n{text}"
    );
}

/// And the resolution is disclosed, because a relative path carries no project
/// identity and the base is chosen by the host, not the caller.
#[test]
fn a_relative_path_reports_which_file_was_read() {
    let project = Project::new("note");
    let responses = exchange_in(
        &project.dir,
        &[&initialize(1), &call_inspect(2, "\"src/lib.rs\"")],
    );
    let joined = warnings_of(result(&responses, 2));
    assert!(
        joined.contains("src/lib.rs") && joined.contains(&project.file().display().to_string()),
        "the caller must be able to see which absolute path was read:\n{joined}"
    );
}

/// An absolute path must stay silent — the note would be noise on every call.
#[test]
fn an_absolute_path_adds_no_resolution_note() {
    let project = Project::new("absolute");
    let absolute = project.file().display().to_string();
    let responses = exchange_in(
        &project.dir,
        &[&initialize(1), &call_inspect(2, &format!("{absolute:?}"))],
    );
    let text = text_of(result(&responses, 2));
    assert!(
        !text.contains("Resolved the relative path"),
        "an absolute path is unambiguous and must not be annotated:\n{text}"
    );
}

/// The failure that must stay loud: a relative path that does not exist under
/// the base, naming the base so the caller knows what was tried.
#[test]
fn a_missing_relative_path_names_the_base_it_tried() {
    let project = Project::new("missing");
    let responses = exchange_in(
        &project.dir,
        &[&initialize(1), &call_inspect(2, "\"src/absent.rs\"")],
    );
    let text = text_of(result(&responses, 2));
    assert!(
        text.contains("not readable"),
        "expected a read failure, got:\n{text}"
    );
    assert!(
        text.contains(&project.dir.display().to_string()),
        "the error must name the directory the relative path was resolved against:\n{text}"
    );
}

/// `topos://build` has to be able to say where its boundary came from, or a
/// server pinned to the wrong repository is invisible.
#[test]
fn build_resource_reports_where_the_file_root_came_from() {
    let project = Project::new("build");
    let responses = exchange_in(&project.dir, &[&initialize(1), READ_BUILD_RESOURCE]);
    let text = result(&responses, 2)["contents"][0]["text"]
        .as_str()
        .expect("build resource text")
        .to_string();
    assert!(text.contains("**file root**"), "{text}");
    assert!(
        text.contains("**file root from**"),
        "the provenance of the root must be reported:\n{text}"
    );
    assert!(
        text.contains("startup working directory"),
        "with no configured boundary the source is the startup directory:\n{text}"
    );
    assert!(
        text.contains(&project.dir.display().to_string()),
        "the reported root should be this project:\n{text}"
    );
}

fn result(responses: &[serde_json::Value], id: u64) -> &serde_json::Value {
    responses
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no response with id {id} in {responses:#?}"))
        .get("result")
        .unwrap_or_else(|| panic!("response {id} carried no result: {responses:#?}"))
}
