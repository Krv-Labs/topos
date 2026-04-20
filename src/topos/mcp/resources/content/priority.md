# Priority Profiles

The `priority` parameter on every Topos evaluation tool shifts metric weights
within each dimension. It does **not** change the lattice structure or
thresholds — COMPOSABLE and SELF_CONTAINED remain independent targets.

## When to use which

### `balanced` (default)

Equal weight on all metrics. Use when no specific axis matters more than
the other — e.g., exploratory evaluation, initial baseline, or when the
codebase will be consumed by many kinds of callers.

### `composable`

Upweights coupling metrics. Use when the file will be a **library surface**
— imported by many consumers. Optimizing for composability means clean fan,
balanced instability, and minimal internal complexity is secondary.

### `self_contained`

Upweights structural metrics. Use when the file is a **leaf** — a concrete
implementation that few things depend on. Minimizing internal complexity
matters more than how it composes.

## Example

Suppose `src/topos/server.py` (an MCP entry point): few callers, lots of
internal orchestration. Use `self_contained` — the structural score
reflects real quality for this kind of file.

Suppose `src/topos/logic/omega.py` (the classifier, imported by every
evaluation path): many callers. Use `composable` — coupling quality is
what makes this file sound in context.

## Switching mid-loop

Agents can change priority across evaluation calls. It's a hint to the
scorer, not a contract — the same raw metrics will produce different
verdicts under different priorities. Reporting both `balanced` + a
priority-specific run can expose which dimension is the current bottleneck.
