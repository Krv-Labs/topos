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
- **Tools**: `topos_evaluate_code`, `topos_evaluate_file`, `topos_evaluate_project`, `topos_compare_code`, `topos_compare_files`, `topos_assess_improvement` (anti-gaming), `topos_inspect_code`, `topos_preference_walk`.
- **Resources**: `topos://docs/lattice`, `topos://docs/metrics`, `topos://docs/priority`, `topos://docs/preferences`, `topos://docs/workflows`.
- **Prompts**: `topos_refactor_until_ideal`.

## Closed-Loop Agent Workflow
1. **Measure**: Run `topos_evaluate_file` (pass `gitnexus_dir` to enable COMPOSABLE).
2. **Plan**: Inspect weak spots using `topos_inspect_code` and evaluation `guidance`.
3. **Refactor**: Apply structural changes.
4. **Verify**: Call `topos_assess_improvement(filepath=..., proposed_code=...)`.
   - `IMPROVEMENT`: Lattice level moved up $\to$ **Commit**.
   - `IMPROVEMENT_SCORE`: Metrics improved, same lattice level $\to$ Keep going.
   - `LATERAL_MOVE` / `REGRESSION`: Revert and re-plan.
   - `SUSPICIOUS_NO_STRUCTURAL_CHANGE`: Only cosmetic changes (whitespace/comments/renames). Do NOT commit; require structural edits.
5. **Decide**: Stop on `IDEAL`, priority-satisfied, or iteration limit.

### Escape Hatches
- **Score plateaus**: Split file. Extract high-complexity functions identified by `topos_inspect_code`.
- **SIMPLE improves, COMPOSABLE regresses**: Abstraction is just relocation. Verify whole project rollup.
