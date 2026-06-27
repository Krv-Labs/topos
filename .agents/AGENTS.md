# CLAUDE.md

## Style & Spelling
- **Writing Style**: Always use **American English spelling** ("optimize", "analyze", "modeling").

## Project Architecture
**Topos** evaluates Python code quality using category theory, mapping programs to an 8-element lattice ($\Omega$) of free Heyting algebra on 3 independent, pairwise incomparable generators:
- **`SIMPLE`** (CFG/AST): cyclomatic complexity, nesting, entropy. Passing: $\ge 0.40$.
- **`COMPOSABLE`** (MDG): coupling, instability, fan-in/out. Passing: $\ge 0.80$. Unreachable without GitNexus directory (`--gitnexus-dir`).
- **`SECURE`** (CPG): dangerous calls, taint flows. Passing: $\ge 0.70$.
- **Lattice ($\Omega$)**: `SLOP` ($\bot$) < single satisfied generators < dual combinations < `IDEAL` ($\top$). Pointwise meet ($\bigwedge$) for rollups.

### Layout & Extensibility
- **`topos/core/`**: Program category, morphism, objects, and `Omega` lattice.
- **`topos/graphs/`**: Representations implementing the `Representation` protocol (`name`, `dimension`, `metrics() -> dict`).
- **`topos/evaluation/`**: `CharacteristicMorphism` ($\chi_S : P \to \Omega$) and policy translators (score functions).
- **`topos/functors/` & `src/`**: Probes (heavy metrics delegating to Rust backend) and profunctors (comparisons).

**To Add a Representation**:
1. Create `graphs/<name>/object.py` implementing the `Representation` protocol.
2. Add raw metric probes in `topos/functors/probes/<name>/`.
3. Register a score dispatcher in `_REPRESENTATION_SCORE_DISPATCHERS` in `topos/evaluation/characteristic_morphism.py`.
4. (Optional) Add pairwise comparison in `topos/functors/profunctors/<name>/compare.py`.

## CLI & Dev Commands
```bash
uv pip install -e ".[dev]" && uv run maturin develop  # Setup
pytest                                              # Run tests
ruff check topos/ --fix && ruff format topos/       # Lint/format
topos evaluate <path> [-r] [--gitnexus-dir <dir>] [--priority <dim>]
```

## Weight Control: Priority vs. Preferences
1. **`Priority`** (Single-knob CLI): upweights primary metric of targeted generator (`simple`/`composable`/`secure`).
   - `simple` $\to$ weights: complexity 0.7, other 0.3
   - `secure` (default) $\to$ weights: secure 0.7, other 0.3
2. **`UserPreferences`** (Strict total order, e.g., `[COMPOSABLE, SECURE, SIMPLE]`):
   - Induces total order on $\Omega$ (binary weighted 4/2/1 by preference rank).
   - Enables two-stage targeting: target `IDEAL` first, fallback to meet of top 2 when progress plateaus.
   - Computes relaxation walk and `next_step` (smallest improvement).
   - Generates granular weight profile (0.7 for top, 0.5 for middle, 0.3 for bottom).

## MCP Server (`topos-mcp`)
Exposes tools, resources, and prompts for agent workflows:
- **Tools**: `topos_evaluate_code`, `topos_evaluate_file`, `topos_evaluate_project`, `topos_compare_code`, `topos_compare_files`, `topos_assess_improvement` (anti-gaming), `topos_assess_worktree_change` (edit-in-place vs a git ref), `topos_begin_refactor` + `topos_assess_snapshot` (edit-in-place vs a captured baseline), `topos_inspect_code`, `topos_preference_walk`, `topos_calculate_coverage`, `topos_get_doc`.
- **Resources**: `topos://docs/agent-contract`, `topos://docs/lattice`, `topos://docs/metrics`, `topos://docs/priority`, `topos://docs/preferences`, `topos://docs/workflows`.
- **Prompts**: `topos_refactor_until_ideal`.

## Closed-Loop Agent Workflow
Read `topos://docs/agent-contract` first. Use Topos as the structural verifier:
measure, make one focused structural change, verify with
`topos_assess_worktree_change` for in-place edits, snapshot first only when the
baseline is not in git, and use `topos_assess_improvement` only for side-by-side
variants. Run relevant behavior checks before accepting.
`IMPROVEMENT` / `IMPROVEMENT_SCORE` are Topos acceptance signals, not automatic
commit permission. `SUSPICIOUS_NO_STRUCTURAL_CHANGE` blocks acceptance.

### Escape Hatches
- **Score plateaus**: Split file. Extract high-complexity functions identified by `topos_inspect_code`.
- **SIMPLE improves, COMPOSABLE regresses**: Abstraction is just relocation. Verify whole project rollup.

## Releases

Use a **lightweight release PR** when shipping a tagged version. Feature work lands in separate PRs first (`[cli]`, `[mcp]`, etc.); the release PR only bumps versions, finalizes the changelog, and applies any small docs touch-ups tied to the version string.

### Branch and PR naming

| Item | Convention | Example |
|------|------------|---------|
| Branch | `release/v<semver>` | `release/v0.3.6` |
| PR title | `[release] v<semver>` | `[release] v0.3.6` |

This matches the scoped prefixes used elsewhere (`[cli]`, `[mcp]`). Do **not** mix feature work into a release branch unless it is a hotfix that must ship in the same tag.

When a feature PR itself carries the release (e.g. last change before tag), you may use `v<semver>: <summary>` in the title instead — but prefer a dedicated `[release]` PR for version-only diffs.

### Version source of truth

**`Cargo.toml`** (`[package].version`) is canonical. Maturin publishes that version to PyPI; `topos._version` reads it for editable installs.

Also bump (must stay in sync):

- `extensions/vscode/package.json` → `"version"`

Run before opening the PR:

```bash
python scripts/check_versions.py   # CI enforces this
```

Python `__version__` and Sphinx `release` are derived from `Cargo.toml` at runtime — no manual edit in `topos/_version.py` or `docs/source/conf.py`.

### Release PR checklist

1. **CHANGELOG.md** — move `[Unreleased]` entries into `## [X.Y.Z] - YYYY-MM-DD`; leave an empty `[Unreleased]` section at the top.
2. **Cargo.toml** — bump `[package].version` (semver: patch for fixes, minor for features while on 0.x).
3. **extensions/vscode/package.json** — same semver string.
4. **Docs** — only if needed (version called out in prose, install examples, etc.). API docs pick up version from `__version__` automatically.
5. **`python scripts/check_versions.py`** — must pass.
6. **PR body** — short summary of what ships; link merged feature PRs if helpful.

No new features, refactors, or unrelated doc edits in a release PR.

### Tag and publish

After the release PR merges to `main`:

```bash
git checkout main && git pull
git tag vX.Y.Z
git push origin vX.Y.Z
```

Tags use a **`v` prefix** (`v0.3.6`). Pushing the tag triggers `.github/workflows/release.yml` (GitHub release assets, PyPI, VS Code marketplace).

Optional manual dispatch: Actions → **Build and Release** → `workflow_dispatch` with `version: vX.Y.Z`.

### Agent workflow summary

```
feature PRs → merge to main → [release] PR (version + changelog) → merge → tag vX.Y.Z → CI release
```
