//! Reading, classifying, writing and removing the topos MCP entry in a
//! JSON-shaped harness config.
//!
//! Covers both plain JSON (`mcpServers.topos`) and VS Code's JSONC
//! (`servers.topos`, plus `"type": "stdio"`). The two differ only in how the
//! file is parsed and whether a transport is declared, so they share one
//! implementation parameterized by [`Artifact`].

use std::path::Path;

use serde_json::{Map, Value};

use super::artifact::{
    names_topos, points_at_topos, Artifact, Inspection, State, MCP_ARGS, SERVER_KEY,
};
use super::binary::drift;
use super::fsops::{read_json_object, read_jsonc_object, write_json_object, WriteOutcome};

pub(crate) fn inspect(artifact: Artifact, path: &Path, binary: &Path) -> Inspection {
    let (map, comments) = match read(artifact, path) {
        Ok(pair) => pair,
        Err(message) => return Inspection::conflict(message),
    };
    let found = classify(artifact, &map, path, binary);
    match found.state {
        // Comments only matter when a write is needed: a correct entry in a
        // commented file is still active, it simply cannot be rewritten.
        State::Active | State::Conflict => found,
        _ if comments => Inspection::conflict(comment_advice(artifact, path, binary)),
        _ => found,
    }
}

pub(crate) fn write(
    artifact: Artifact,
    path: &Path,
    binary: &Path,
    backup: bool,
) -> Result<WriteOutcome, String> {
    let (mut map, _) = read(artifact, path)?;
    let container = map
        .entry(artifact.container_key().to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = container else {
        return Err(format!(
            "{} `{}` must be an object",
            path.display(),
            artifact.container_key()
        ));
    };
    let entry = servers
        .entry(SERVER_KEY.to_string())
        .or_insert_with(|| fresh_entry(artifact, binary));
    let Value::Object(fields) = entry else {
        return Err(format!(
            "{} `{SERVER_KEY}` must be an object",
            path.display()
        ));
    };
    // Field-wise: set only the keys topos owns and leave any the client added,
    // so client-side normalization never turns into a rewrite loop.
    set_owned_fields(fields, artifact, binary);
    write_json_object(path, &map, backup)
}

pub(crate) fn remove(artifact: Artifact, path: &Path, dry_run: bool) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let (mut map, comments) = read(artifact, path)?;
    if !owns_entry(artifact, &map) {
        return Ok(false);
    }
    if comments {
        return Err(format!(
            "{} contains comments — remove its `{SERVER_KEY}` entry by hand",
            path.display()
        ));
    }
    let Some(Value::Object(servers)) = map.get_mut(artifact.container_key()) else {
        return Ok(false);
    };
    servers.remove(SERVER_KEY);
    if servers.is_empty() {
        map.remove(artifact.container_key());
    }
    if !dry_run {
        write_json_object(path, &map, false)?;
    }
    Ok(true)
}

pub(crate) fn duplicate_keys(artifact: Artifact, path: &Path, binary: &Path) -> Vec<String> {
    let Ok((map, _)) = read(artifact, path) else {
        return Vec::new();
    };
    let Some(Value::Object(servers)) = map.get(artifact.container_key()) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter(|(key, _)| key.as_str() != SERVER_KEY)
        .filter(|(_, entry)| command_of(entry).is_some_and(|c| points_at_topos(c, binary)))
        .map(|(key, _)| key.clone())
        .collect()
}

fn read(artifact: Artifact, path: &Path) -> Result<(Map<String, Value>, bool), String> {
    if artifact.is_jsonc() {
        read_jsonc_object(path)
    } else {
        read_json_object(path).map(|map| (map, false))
    }
}

fn classify(
    artifact: Artifact,
    map: &Map<String, Value>,
    path: &Path,
    binary: &Path,
) -> Inspection {
    let Some(entry) = entry_of(artifact, map) else {
        return Inspection::plain(State::Absent);
    };
    let command = command_of(entry).unwrap_or_default();
    if !names_topos(command) || !args_are_mcp(entry.get("args")) {
        return Inspection::conflict(format!(
            "`{SERVER_KEY}` in {} is an MCP entry topos did not write — inspect it by hand",
            path.display()
        ));
    }
    if let Some(reason) = drift(command, binary) {
        return Inspection::incomplete(reason);
    }
    if artifact.wants_stdio_type() && entry.get("type").and_then(Value::as_str) != Some("stdio") {
        return Inspection::incomplete(format!(
            "`{SERVER_KEY}` in {} is missing \"type\": \"stdio\"",
            path.display()
        ));
    }
    Inspection::plain(State::Active)
}

fn entry_of(artifact: Artifact, map: &Map<String, Value>) -> Option<&Value> {
    map.get(artifact.container_key())?.get(SERVER_KEY)
}

fn command_of(entry: &Value) -> Option<&str> {
    entry.get("command").and_then(Value::as_str)
}

fn owns_entry(artifact: Artifact, map: &Map<String, Value>) -> bool {
    let Some(entry) = entry_of(artifact, map) else {
        return false;
    };
    names_topos(command_of(entry).unwrap_or_default()) && args_are_mcp(entry.get("args"))
}

fn args_are_mcp(args: Option<&Value>) -> bool {
    args.and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>() == MCP_ARGS)
        .unwrap_or(false)
}

fn args_value() -> Value {
    Value::Array(
        MCP_ARGS
            .iter()
            .map(|arg| Value::String((*arg).to_string()))
            .collect(),
    )
}

fn set_owned_fields(fields: &mut Map<String, Value>, artifact: Artifact, binary: &Path) {
    if artifact.wants_stdio_type() {
        fields.insert("type".to_string(), Value::String("stdio".to_string()));
    }
    fields.insert(
        "command".to_string(),
        Value::String(binary.display().to_string()),
    );
    fields.insert("args".to_string(), args_value());
}

fn fresh_entry(artifact: Artifact, binary: &Path) -> Value {
    let mut fields = Map::new();
    set_owned_fields(&mut fields, artifact, binary);
    Value::Object(fields)
}

/// A `Conflict` message the user can act on without opening an editor twice:
/// it carries the exact entry to paste.
fn comment_advice(artifact: Artifact, path: &Path, binary: &Path) -> String {
    let entry =
        serde_json::to_string(&fresh_entry(artifact, binary)).unwrap_or_else(|_| "{}".to_string());
    format!(
        "{} contains comments, which topos will not rewrite — add \"{SERVER_KEY}\": {entry} to its \"{}\" object by hand",
        path.display(),
        artifact.container_key()
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::commands::install::fsops::backup_path;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("topos-json-entry-{label}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real, executable stand-in for the topos binary, so `drift` has
    /// something to compare identity against.
    fn fake_binary(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("topos");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn entry_round_trips_and_leaves_foreign_content_alone() {
        let dir = scratch("round-trip");
        let binary = fake_binary(&dir);
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{"numStartups": 42, "mcpServers": {"other": {"command": "foo"}}}"#,
        )
        .unwrap();

        let art = Artifact::McpJson;
        assert_eq!(art.inspect(&path, &binary).state, State::Absent);
        assert!(art.apply(&path, &binary).unwrap().is_some());
        assert_eq!(art.inspect(&path, &binary).state, State::Active);
        // Idempotent: a second apply writes nothing at all.
        assert!(art.apply(&path, &binary).unwrap().is_none());

        assert!(art.remove(&path, false).unwrap());
        assert_eq!(art.inspect(&path, &binary).state, State::Absent);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"other\""), "foreign entry was dropped");
        assert!(text.contains("42"), "unrelated top-level key was dropped");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_recorded_command_is_absolute_and_args_are_exactly_mcp() {
        let dir = scratch("absolute");
        let binary = fake_binary(&dir);
        let path = dir.join("settings.json");
        Artifact::McpJson.apply(&path, &binary).unwrap();

        let map = read_json_object(&path).unwrap();
        let entry = &map["mcpServers"]["topos"];
        assert_eq!(entry["command"], json_str(&binary));
        assert_eq!(entry["args"], serde_json::json!(["mcp"]));
        // A bare `topos` prints usage and exits, so `args` may never be empty.
        assert!(entry["args"].as_array().is_some_and(|a| !a.is_empty()));
        // No `type`: no literal value is portable across these clients.
        assert!(entry.get("type").is_none());
        fs::remove_dir_all(dir).ok();
    }

    fn json_str(path: &Path) -> Value {
        Value::String(path.display().to_string())
    }

    #[test]
    fn client_added_keys_survive_and_do_not_cause_a_rewrite_loop() {
        let dir = scratch("field-wise");
        let binary = fake_binary(&dir);
        let path = dir.join("settings.json");
        Artifact::McpJson.apply(&path, &binary).unwrap();

        // Simulate a client normalizing the entry: reorder the keys topos owns
        // and add one of its own. Whole-value equality would report this as
        // stale forever; field-wise comparison sees it as active.
        let mut map = read_json_object(&path).unwrap();
        let mut fields = Map::new();
        fields.insert("args".to_string(), serde_json::json!(["mcp"]));
        fields.insert("type".to_string(), Value::String("local".to_string()));
        fields.insert("command".to_string(), json_str(&binary));
        map["mcpServers"]["topos"] = Value::Object(fields);
        fs::write(&path, serde_json::to_string_pretty(&map).unwrap()).unwrap();

        assert_eq!(
            Artifact::McpJson.inspect(&path, &binary).state,
            State::Active
        );
        assert!(Artifact::McpJson.apply(&path, &binary).unwrap().is_none());
        let after = read_json_object(&path).unwrap();
        assert_eq!(after["mcpServers"]["topos"]["type"], "local");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn path_drift_is_incomplete_and_install_repairs_it_without_clobbering_the_backup() {
        let dir = scratch("drift");
        let binary = fake_binary(&dir);
        let path = dir.join("settings.json");
        fs::write(&path, "{\"userKey\": 1}\n").unwrap();

        Artifact::McpJson.apply(&path, &binary).unwrap();
        let pristine = fs::read_to_string(backup_path(&path)).unwrap();
        assert!(pristine.contains("userKey"));
        assert!(!pristine.contains("topos"), "backup captured our own write");

        // The draft's bare `topos` is a relative path, so it drifts.
        let mut map = read_json_object(&path).unwrap();
        map["mcpServers"]["topos"]["command"] = Value::String("topos".to_string());
        fs::write(&path, serde_json::to_string_pretty(&map).unwrap()).unwrap();
        let inspection = Artifact::McpJson.inspect(&path, &binary);
        assert_eq!(inspection.state, State::Incomplete);
        assert!(inspection.detail.is_some(), "drift must be explained");

        Artifact::McpJson.apply(&path, &binary).unwrap();
        assert_eq!(
            Artifact::McpJson.inspect(&path, &binary).state,
            State::Active
        );
        // The repair must not have replaced the pristine snapshot.
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), pristine);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_hand_made_entry_under_our_key_is_a_conflict_and_is_never_removed() {
        let dir = scratch("foreign");
        let binary = fake_binary(&dir);
        let path = dir.join("settings.json");
        let seed = r#"{"mcpServers": {"topos": {"command": "uvx", "args": ["topos-mcp"]}}}"#;
        fs::write(&path, seed).unwrap();

        let art = Artifact::McpJson;
        assert_eq!(art.inspect(&path, &binary).state, State::Conflict);
        assert!(art.apply(&path, &binary).is_err());
        assert!(!art.remove(&path, false).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), seed);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_unparseable_file_is_a_conflict_and_is_left_untouched() {
        let dir = scratch("unparseable");
        let binary = fake_binary(&dir);
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();

        assert_eq!(
            Artifact::McpJson.inspect(&path, &binary).state,
            State::Conflict
        );
        assert!(Artifact::McpJson.apply(&path, &binary).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn vscode_uses_the_servers_key_and_declares_a_stdio_transport() {
        let dir = scratch("vscode");
        let binary = fake_binary(&dir);
        let path = dir.join("mcp.json");

        Artifact::VsCodeJsonc.apply(&path, &binary).unwrap();
        let map = read_json_object(&path).unwrap();
        assert!(map.get("mcpServers").is_none());
        assert_eq!(map["servers"]["topos"]["type"], "stdio");
        assert_eq!(
            Artifact::VsCodeJsonc.inspect(&path, &binary).state,
            State::Active
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_commented_vscode_config_is_a_conflict_carrying_the_entry_to_paste() {
        let dir = scratch("jsonc");
        let binary = fake_binary(&dir);
        let path = dir.join("mcp.json");
        let seed = "{\n  // my servers\n  \"servers\": {}\n}\n";
        fs::write(&path, seed).unwrap();

        let inspection = Artifact::VsCodeJsonc.inspect(&path, &binary);
        assert_eq!(inspection.state, State::Conflict);
        let detail = inspection.detail.unwrap();
        assert!(detail.contains("\"servers\""), "{detail}");
        assert!(detail.contains("stdio"), "{detail}");
        assert!(Artifact::VsCodeJsonc.apply(&path, &binary).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), seed, "comments lost");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn foreign_keys_pointing_at_topos_are_reported_as_duplicates() {
        let dir = scratch("duplicates");
        let binary = fake_binary(&dir);
        let path = dir.join("mcp_config.json");
        fs::write(
            &path,
            r#"{"mcpServers": {
                 "topos-mcp": {"command": "topos", "args": ["mcp"]},
                 "unrelated": {"command": "uvx", "args": ["other"]}
               }}"#,
        )
        .unwrap();

        let keys = Artifact::McpJson.duplicate_keys(&path, &binary);
        assert_eq!(keys, vec!["topos-mcp".to_string()]);
        fs::remove_dir_all(dir).ok();
    }
}
