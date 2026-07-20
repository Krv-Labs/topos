<!-- OPENWIKI:START -->

## OpenWiki

This repository uses OpenWiki for recurring code documentation. Start with `openwiki/quickstart.md`, then follow its links to architecture, workflows, domain concepts, operations, integrations, testing guidance, and source maps.

The scheduled OpenWiki GitHub Actions workflow refreshes the repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.

<!-- OPENWIKI:END -->

<!-- OPENWIKI-POLICY:START -->
## OpenWiki CI (repo-owned policy)

Canonical workflow: [`.github/workflows/openwiki.yml`](.github/workflows/openwiki.yml).

### What it does

Regenerates the engineering wiki under `openwiki/` from the codebase, then opens a docs PR (`openwiki/update`) with only:

- `openwiki/**`
- `AGENTS.md`
- `CLAUDE.md`

Workflow files are **never** committed by that PR.

### When it runs (no cron)

1. **A PR is merged / code is pushed to `main`** on non-docs paths, or
2. **Manual** — Actions → **OpenWiki Update** → **Run workflow**

`paths-ignore` covers `openwiki/**`, `AGENTS.md`, and `CLAUDE.md` so merging the auto-generated docs PR does **not** re-run OpenWiki and burn credits.

### Cost / model

> [!NOTE]
> **OpenAI usage:** This workflow calls OpenAI with `OPENWIKI_MODEL_ID=gpt-5.6-terra` via the repo secret `OPENAI_API_KEY` (already configured). Each run bills the OpenAI account for a full OpenWiki regeneration. Prefer intentional runs—after meaningful code merges or a manual **Run workflow**—and avoid repeated or speculative executions. Pure `openwiki/**` / `AGENTS.md` / `CLAUDE.md` merges are ignored so the auto-docs PR does not re-trigger itself. Prefer the generated docs PR over hand-editing OpenWiki pages.

### Hardening against OpenWiki CLI overwrites

OpenWiki CLI rewrites `.github/workflows/openwiki-update.yml` on every `openwiki code --update` (stock daily cron template). CI:

1. Uses **only** `openwiki.yml` as the committed workflow
2. **Deletes** any regenerated `openwiki-update.yml` after the CLI runs
3. **Excludes** `.github/workflows/**` from `create-pull-request` `add-paths`

### MCP docs vs OpenWiki

`topos_get_doc` / `topos://docs/*` serve six embedded topics only (`agent-contract`, `lattice`, `metrics`, `preferences`, `priority`, `workflows`). Broader engineering docs live under `openwiki/` on the filesystem — they are **not** MCP resources. Agents with workspace access should read those files directly.
<!-- OPENWIKI-POLICY:END -->
