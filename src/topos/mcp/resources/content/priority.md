# Priority Profiles

The `priority` parameter on every Topos evaluation tool encodes the
manager's **strict total order on the quality generators** `G_qual =
{ SIMPLE, COMPOSABLE, SECURE }`.

It shifts metric weights inside each policy translator `Φᵢ` and, when
agents must trade off, defines the target-relaxation walk (highest
bit = highest priority).  It does **not** change the lattice — the three
generators remain pairwise incomparable in `H(G_qual)`.

## When to use which

### `balanced` (default)

Equal weight across SIMPLE / COMPOSABLE / SECURE.  Use for exploratory
evaluation, initial baselines, or whenever no axis dominates.

### `simple`

Upweights the SIMPLE generator's metrics (CFG cyclomatic complexity).  Use
when the file is a **leaf implementation** — concrete logic that few things
depend on.  Minimizing internal branching matters more than how it composes
or how cautiously it handles inputs.

### `composable`

Upweights the COMPOSABLE generator's metrics (Martin coupling / instability).
Use when the file is a **library surface** — imported by many consumers.
Clean fan-in/out and balanced instability dominate.

### `secure`

Upweights the SECURE generator's metrics (CPG dangerous-API reachability,
taint flows).  Use when the file handles **untrusted input** — request
handlers, deserialization sinks, shell-out wrappers.

## Example

`src/topos/server.py` (MCP entry point, few callers, lots of internal
orchestration): use `simple` — the SIMPLE generator reflects real quality.

`src/topos/evaluation/omega.py` (the classifier, imported by every evaluation
path): use `composable` — coupling quality is the main lever here.

`src/topos/utils/yaml_loader.py` (parses untrusted user config): use
`secure` — `yaml.load` is a known footgun; the SECURE generator is the
relevant target.

## Switching mid-loop

Agents can change priority across evaluation calls.  It is a hint to the
scorer, not a contract — the same raw metrics produce different verdicts
under different priorities.  Reporting `balanced` plus a priority-specific
run typically exposes which generator is the current bottleneck.
