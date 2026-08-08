# Priority Profiles

The `priority` parameter is a single scorer knob. It names one quality
generator (`simple`, `composable`, `secure`, or `navigable`) to emphasize,
and steers the guidance the policy translators `Φᵢ` return.

Priority does **not** define the target-relaxation walk and does **not**
linearize the lattice. Use `preferences.ranking` when an agent needs a strict
total order over all four generators.

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

Upweights the COMPOSABLE generator's dependency metrics. Use when the file is
an **orchestrator or integration boundary** whose outward interactions deserve
special attention. Fan-out drives the file-level verdict; fan-in and Martin
stability readings enrich the diagnosis without hard-failing the file.

### `navigable`

Upweights the NAVIGABLE generator (worst-function nesting divergence). Use
when a file is one agents will keep having to read and edit — a hot path
for automated maintenance. Note this is orthogonal to `simple`: a file can
have low branch counts everywhere and still be deeply nested.

## Example

`topos/mcp/src/server.rs` (MCP entry point, few callers, lots of internal
orchestration): use `simple` — the SIMPLE generator reflects real quality.

`topos/engine/src/core/omega.rs` (the classifier, imported by every evaluation
path): use `composable` — coupling quality is the main lever here.

`topos/engine/src/adapters/gitnexus.rs` (parses untrusted subprocess output):
use `secure` — external input crossing a trust boundary is a known footgun;
the SECURE generator is the relevant target.

## Switching mid-loop

Agents can change priority across evaluation calls. It is a hint to the scorer,
not a contract for what tradeoff to accept. Running the same file at e.g.
`secure` then `composable` can expose which generator is the current scoring
bottleneck.

For target tradeoffs, use `preferences.ranking`: it tells the agent which
silver or bronze outcome to prefer if `IDEAL` stalls.
