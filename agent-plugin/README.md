# Topos Agent Plugin

Portable [Agent Plugins](https://agent-plugins.org/) 1.0 package for Topos.

Compatible clients discover this directory, load `plugin.json`, then optionally enable:

- **Skill** — `skills/topos/SKILL.md` (Agent Skills format)
- **MCP** — `mcp.json` stdio server (`topos mcp`)

## Prerequisites

1. Install the Topos CLI so `topos` is on `PATH`:

   ```bash
   curl -fsSL https://docs.krv.ai/topos/install.sh | bash
   ```

2. Point an Agent Plugins–compatible client at this package directory
   (`agent-plugin/` in a clone, or a released copy of the same tree).

Installation, enablement, and permissions remain client-managed
([spec](https://agent-plugins.org/plugin-authors#package-boundaries)).

## Layout

```text
agent-plugin/
├── plugin.json
├── mcp.json
└── skills/
    └── topos/
        └── SKILL.md
```

`skills/topos/SKILL.md` must stay byte-identical to the ClawHub skill at
`../skills/topos/SKILL.md` (in this repository). Validate from the repo root with:

    python scripts/check_agent_plugin.py

## References

- [Agent Plugins](https://agent-plugins.org/)
- [Topos docs](https://docs.krv.ai/topos/)
- [Agent contract](https://docs.krv.ai/topos/agents.html)
