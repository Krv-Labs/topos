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
    // The pinned `SUPPORTED_PROTOCOL_VERSIONS`, reaching the wire intact. This
    // list also bounds `initialize` negotiation, so dropping a revision here
    // stops Topos speaking it — see `initialize_negotiates_every_supported_revision`.
    // Asserted exactly (not by presence) so that an rmcp upgrade which adds a
    // revision fails here and forces a deliberate choice.
    assert_eq!(
        discover["supportedVersions"],
        serde_json::json!([
            "2024-11-05",
            "2025-03-26",
            "2025-06-18",
            "2025-11-25",
            "2026-07-28"
        ])
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
    assert!(
        discover["capabilities"]["extensions"]
            .get("io.modelcontextprotocol/tasks")
            .is_some(),
        "io.modelcontextprotocol/tasks extension capability missing from discover: {discover:#?}"
    );

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

/// Every handshake revision in `SUPPORTED_PROTOCOL_VERSIONS` negotiates as
/// itself over `initialize`, while `2026-07-28` (`ProtocolVersion::NO_INITIALIZE`)
/// replaced `initialize` with `server/discover` + per-request `_meta` and
/// therefore answers a legacy `initialize` call with the newest handshake
/// revision (`2025-11-25`).
#[test]
fn initialize_negotiates_every_supported_revision() {
    // Hoisted: the reference surface is the same for every revision, so spawning
    // it once per iteration would double the child processes for nothing.
    let reference_tools = tool_names(&initialize_era_tools_list());
    for (requested, expected_negotiated) in [
        ("2024-11-05", "2024-11-05"),
        ("2025-03-26", "2025-03-26"),
        ("2025-06-18", "2025-06-18"),
        ("2025-11-25", "2025-11-25"),
        ("2026-07-28", "2025-11-25"),
    ] {
        let responses = exchange(&[
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{requested}","capabilities":{{}},"clientInfo":{{"name":"lifecycle-test","version":"0.0.1"}}}}}}"#
            ),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        ]);

        let init = result_for_id(&responses, 1);
        assert_eq!(
            init["protocolVersion"], expected_negotiated,
            "unexpected negotiated version for requested {requested}: {init:#?}"
        );
        // A revision is only genuinely supported if the session then works.
        assert_eq!(
            tool_names(result_for_id(&responses, 2)),
            reference_tools,
            "tool surface differs under requested {requested}"
        );
    }
}

/// SEP-2549 explicit caching hints: static discovery/doc surfaces advertise a
/// 24h public TTL, while runtime-specific `topos://build` advertises `ttlMs: 0`
/// and `cacheScope: "private"`.
#[test]
fn sep2549_caching_hints_on_static_and_dynamic_surfaces() {
    let responses = exchange(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"prompts/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"resources/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"resources/templates/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"topos://docs/lattice","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"topos://build","_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
    ]);

    for id in 1..=5 {
        let res = result_for_id(&responses, id);
        assert_eq!(res["resultType"], "complete");
        assert_eq!(res["ttlMs"], 86_400_000);
        assert_eq!(res["cacheScope"], "public");
    }

    let build_res = result_for_id(&responses, 6);
    assert_eq!(build_res["resultType"], "complete");
    assert_eq!(build_res["ttlMs"], 0);
    assert_eq!(build_res["cacheScope"], "private");
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
