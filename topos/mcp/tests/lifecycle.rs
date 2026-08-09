//! Stdio lifecycle regression tests — both MCP eras Topos claims.
//!
//! The rmcp 3 upgrade (#264) got `server/discover` for free from the SDK's
//! default `ServerHandler` impls, which is exactly why it needs a test: nothing
//! in the Topos source mentions discovery, so a future handler change could
//! silently drop the stateless path with no compile error. These pipe real
//! JSON-RPC frames into the built binary and assert on what comes back.

use std::io::Write;
use std::process::{Command, Stdio};

/// Pipe newline-delimited frames into a fresh `topos-mcp`, close stdin, and
/// parse the responses it wrote before shutting down.
fn exchange(frames: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_topos-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn topos-mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    for frame in frames {
        writeln!(stdin, "{frame}").expect("write frame");
    }
    drop(stdin); // EOF — the server shuts the stdio transport down.

    let out = child.wait_with_output().expect("wait for topos-mcp");
    String::from_utf8(out.stdout)
        .expect("utf-8 stdout")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response line"))
        .collect()
}

fn result_for_id(responses: &[serde_json::Value], id: u64) -> &serde_json::Value {
    responses
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no response with id {id} in {responses:#?}"))
        .get("result")
        .unwrap_or_else(|| panic!("response {id} carried no result: {responses:#?}"))
}

/// Legacy initialize era — what every host in the wild speaks today.
#[test]
fn initialize_lifecycle_lists_and_calls_tools() {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"lifecycle-test","version":"0.0.1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"topos_get_doc","arguments":{"topic":"agent-contract"}}}"#,
    ]);

    let init = result_for_id(&responses, 1);
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(init["serverInfo"]["name"], "topos_mcp");

    let names = tool_names(result_for_id(&responses, 2));
    assert!(
        names.iter().any(|n| n == "topos_get_doc"),
        "topos_get_doc missing from {names:?}"
    );

    let call = result_for_id(&responses, 3);
    assert_eq!(call["isError"], false);
    assert!(
        call["content"][0]["text"]
            .as_str()
            .expect("text content")
            .contains("Topos Agent Contract"),
        "unexpected doc body: {call:#?}"
    );
}

/// Stateless 2026-07-28 era — `server/discover` as the very first frame, then
/// tool traffic with no `initialize` and no session id anywhere.
#[test]
fn discover_lifecycle_needs_no_initialize() {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        // Under 2026-07-28 every request is self-describing: `_meta` carries
        // the protocol version AND client capabilities, or the server rejects
        // it with -32602. There is no handshake to hoist them out of.
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"topos_get_doc","arguments":{"topic":"agent-contract"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
    ]);

    let discover = result_for_id(&responses, 1);
    assert_eq!(discover["resultType"], "complete");
    // Exactly the revisions `SUPPORTED_PROTOCOL_VERSIONS` narrows to — not
    // rmcp's default of every version the SDK happens to know.
    assert_eq!(
        discover["supportedVersions"],
        serde_json::json!(["2025-11-25", "2026-07-28"])
    );
    assert_eq!(
        discover["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "topos_mcp"
    );
    for capability in ["tools", "resources", "prompts"] {
        assert!(
            discover["capabilities"].get(capability).is_some(),
            "{capability} capability missing from discover: {discover:#?}"
        );
    }

    // Same tool surface as the initialize path: discovery is a lifecycle
    // change, not a different server.
    assert_eq!(
        tool_names(result_for_id(&responses, 2)),
        tool_names(&initialize_era_tools_list())
    );

    let call = result_for_id(&responses, 3);
    assert_eq!(call["isError"], false);
    assert!(call["content"][0]["text"]
        .as_str()
        .expect("text content")
        .contains("Topos Agent Contract"));
}

fn tool_names(list_result: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = list_result["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list carried no array: {list_result:#?}"))
        .iter()
        .map(|t| t["name"].as_str().expect("tool name").to_string())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "tools/list returned nothing");
    names
}

fn initialize_era_tools_list() -> serde_json::Value {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"lifecycle-test","version":"0.0.1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    ]);
    result_for_id(&responses, 2).clone()
}
