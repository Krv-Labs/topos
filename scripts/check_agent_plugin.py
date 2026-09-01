#!/usr/bin/env python3
"""Validate the Agent Plugins 1.0 package under agent-plugin/."""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
PLUGIN_ROOT = ROOT / "agent-plugin"
PLUGIN_SCHEMA = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
MCP_SCHEMA = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json"
NAME_RE = re.compile(r"^(?!.*(?:--|\.\.))[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$")
PLUGIN_TOP_LEVEL = {
    "$schema",
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "extensions",
}
MCP_TOP_LEVEL = {"$schema", "mcpServers"}
STDIO_KEYS = {"type", "command", "args", "env", "cwd"}
HTTP_KEYS = {"type", "url", "headers"}


def cargo_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as f:
        data = tomllib.load(f)
    if "package" in data and "version" in data["package"]:
        return data["package"]["version"]
    return data["workspace"]["package"]["version"]


def load_json(path: Path, errors: list[str]) -> dict[str, Any] | None:
    if not path.is_file():
        errors.append(f"{path.relative_to(ROOT)}: missing")
        return None
    if path.is_symlink():
        errors.append(f"{path.relative_to(ROOT)}: must be a regular file, not a symlink")
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        errors.append(f"{path.relative_to(ROOT)}: invalid JSON ({exc})")
        return None
    if not isinstance(data, dict):
        errors.append(f"{path.relative_to(ROOT)}: must be a JSON object")
        return None
    return data


def validate_plugin_manifest(data: dict[str, Any], expected_version: str) -> list[str]:
    errors: list[str] = []
    unknown = sorted(set(data) - PLUGIN_TOP_LEVEL)
    # Spec: unknown top-level fields are reported but non-fatal for clients.
    # For our authored package, treat them as errors so we stay schema-closed.
    for key in unknown:
        errors.append(f"agent-plugin/plugin.json: unknown top-level field {key!r}")

    if data.get("$schema") != PLUGIN_SCHEMA:
        errors.append(
            f"agent-plugin/plugin.json: $schema must be {PLUGIN_SCHEMA!r}, "
            f"got {data.get('$schema')!r}"
        )

    name = data.get("name")
    if not isinstance(name, str) or not name:
        errors.append("agent-plugin/plugin.json: name is required")
    elif not NAME_RE.fullmatch(name) or len(name) > 64:
        errors.append(f"agent-plugin/plugin.json: invalid name {name!r}")

    version = data.get("version")
    if version is None:
        errors.append("agent-plugin/plugin.json: version is required in this repo")
    elif not isinstance(version, str):
        errors.append("agent-plugin/plugin.json: version must be a string")
    elif version != expected_version:
        errors.append(
            f"agent-plugin/plugin.json: version {version!r} must match "
            f"Cargo.toml {expected_version!r}"
        )

    for key in ("description", "homepage", "repository", "license"):
        if key in data and not isinstance(data[key], str):
            errors.append(f"agent-plugin/plugin.json: {key} must be a string")

    if "keywords" in data:
        keywords = data["keywords"]
        if not isinstance(keywords, list) or not all(
            isinstance(item, str) for item in keywords
        ):
            errors.append("agent-plugin/plugin.json: keywords must be an array of strings")

    if "author" in data:
        author = data["author"]
        if not isinstance(author, dict):
            errors.append("agent-plugin/plugin.json: author must be an object")
        else:
            for key in ("name", "email", "url"):
                if key in author and not isinstance(author[key], str):
                    errors.append(
                        f"agent-plugin/plugin.json: author.{key} must be a string"
                    )
            unknown_author = sorted(set(author) - {"name", "email", "url"})
            for key in unknown_author:
                errors.append(
                    f"agent-plugin/plugin.json: unknown author field {key!r}"
                )

    if "extensions" in data and not isinstance(data["extensions"], dict):
        errors.append("agent-plugin/plugin.json: extensions must be an object")

    return errors


def validate_stdio_server(name: str, server: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    unknown = sorted(set(server) - STDIO_KEYS)
    for key in unknown:
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: unknown field {key!r}"
        )
    command = server.get("command")
    if not isinstance(command, str) or not command.strip():
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: command is required"
        )
    elif command != command.strip() or re.search(r"\s", command):
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: command must be one token"
        )
    if "args" in server:
        args = server["args"]
        if not isinstance(args, list) or not all(isinstance(item, str) for item in args):
            errors.append(
                f"agent-plugin/mcp.json: mcpServers.{name}: args must be string array"
            )
    if "env" in server and not isinstance(server["env"], dict):
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: env must be an object"
        )
    if "cwd" in server and not isinstance(server["cwd"], str):
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: cwd must be a string"
        )
    return errors


def validate_http_server(name: str, server: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    unknown = sorted(set(server) - HTTP_KEYS)
    for key in unknown:
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: unknown field {key!r}"
        )
    url = server.get("url")
    if not isinstance(url, str) or not url:
        errors.append(f"agent-plugin/mcp.json: mcpServers.{name}: url is required")
    if "headers" in server and not isinstance(server["headers"], dict):
        errors.append(
            f"agent-plugin/mcp.json: mcpServers.{name}: headers must be an object"
        )
    return errors


def validate_mcp(data: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    unknown = sorted(set(data) - MCP_TOP_LEVEL)
    for key in unknown:
        errors.append(f"agent-plugin/mcp.json: unknown top-level field {key!r}")

    if data.get("$schema") != MCP_SCHEMA:
        errors.append(
            f"agent-plugin/mcp.json: $schema must be {MCP_SCHEMA!r}, "
            f"got {data.get('$schema')!r}"
        )

    servers = data.get("mcpServers")
    if not isinstance(servers, dict) or not servers:
        errors.append("agent-plugin/mcp.json: mcpServers must be a non-empty object")
        return errors

    for name, server in servers.items():
        if not isinstance(server, dict):
            errors.append(
                f"agent-plugin/mcp.json: mcpServers.{name} must be an object"
            )
            continue
        transport = server.get("type")
        if transport == "stdio":
            errors.extend(validate_stdio_server(name, server))
        elif transport in {"streamable-http", "sse"}:
            errors.extend(validate_http_server(name, server))
        else:
            errors.append(
                f"agent-plugin/mcp.json: mcpServers.{name}: "
                f"type must be stdio, streamable-http, or sse; got {transport!r}"
            )
    return errors


def validate_skill_sync() -> list[str]:
    canonical = ROOT / "skills" / "topos" / "SKILL.md"
    packaged = PLUGIN_ROOT / "skills" / "topos" / "SKILL.md"
    errors: list[str] = []
    if not canonical.is_file():
        errors.append("skills/topos/SKILL.md: missing canonical skill")
        return errors
    if not packaged.is_file():
        errors.append("agent-plugin/skills/topos/SKILL.md: missing packaged skill")
        return errors
    if packaged.is_symlink():
        errors.append(
            "agent-plugin/skills/topos/SKILL.md: must be a regular file "
            "(Agent Plugins forbids package-escaping symlinks)"
        )
        return errors
    if canonical.read_bytes() != packaged.read_bytes():
        errors.append(
            "agent-plugin/skills/topos/SKILL.md differs from skills/topos/SKILL.md; "
            "copy the canonical skill into the plugin package"
        )
    return errors


def main() -> int:
    if not PLUGIN_ROOT.is_dir():
        print(f"agent-plugin root not found: {PLUGIN_ROOT}", file=sys.stderr)
        return 1

    expected_version = cargo_version()
    errors: list[str] = []

    plugin = load_json(PLUGIN_ROOT / "plugin.json", errors)
    if plugin is not None:
        errors.extend(validate_plugin_manifest(plugin, expected_version))

    mcp = load_json(PLUGIN_ROOT / "mcp.json", errors)
    if mcp is not None:
        errors.extend(validate_mcp(mcp))

    for path in PLUGIN_ROOT.rglob("*"):
        if path.is_symlink():
            errors.append(f"{path.relative_to(ROOT)}: must not be a symlink")
            break

    errors.extend(validate_skill_sync())

    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        return 1

    print(
        f"agent-plugin check passed "
        f"(Agent Plugins 1.0.0, version {expected_version})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
