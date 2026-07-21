# Priority Profiles

The `priority` parameter is a single scorer knob. It upweights one quality
generator (`simple`, `composable`, or `secure`) inside the policy translators
`Φᵢ`.

Priority does **not** define the target-relaxation walk and does **not**
linearize the lattice. Use `preferences.ranking` when an agent needs a strict
total order over `SIMPLE`, `COMPOSABLE`, and `SECURE`.

## When to use which

### `secure` (default)

Conservative default: upweights SECURE metrics (`w_taint` highest within
each `Φᵢ`).  Use when you want a single knob without tuning — especially
mixed or unfamiliar codebases.

### `simple`

Upweights the SIMPLE generator's metrics (CFG cyclomatic complexity).  Use
when the file is a **leaf implementation** — concrete logic that few things
depend on.  Minimizing internal branching matters more than how it composes
or how cautiously it handles inputs.

### `composable`

Upweights the COMPOSABLE generator's metrics (Martin coupling / instability).
Use when the file is a **library surface** — imported by many consumers.
Clean fan-in/out and instability in the healthy band dominate.

## Example

`topos/server.py` (MCP entry point, few callers, lots of internal
orchestration): use `simple` — the SIMPLE generator reflects real quality.

`topos/evaluation/omega.py` (the classifier, imported by every evaluation
path): use `composable` — coupling quality is the main lever here.

`topos/utils/yaml_loader.py` (parses untrusted user config): use
`secure` — `yaml.load` is a known footgun; the SECURE generator is the
relevant target.

## Switching mid-loop

Agents can change priority across evaluation calls. It is a hint to the scorer,
not a contract for what tradeoff to accept. Running the same file at e.g.
`secure` then `composable` can expose which generator is the current scoring
bottleneck.

For target tradeoffs, use `preferences.ranking`: it tells the agent which
silver or bronze outcome to prefer if `IDEAL` stalls.
