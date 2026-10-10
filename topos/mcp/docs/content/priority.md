# Priority Profiles

`priority` names one generator (`simple`, `composable`, `secure`, or
`navigable`). That generator is the head of the preference ranking: it
wins an ascent tie, and on the CLI a single `--priority` value is lifted
into a full ranking with `focused_ranking`. The engine default is `simple`,
the head of the default ranking.

The policy translators do not reweight metrics for priority, and priority
does not change `achieved`. The lattice walk stays the bitmask order from
`preferences.ranking`. `next_pillar` is chosen by gate score inside that
goal; priority only breaks a tie.

## When to use which

### `simple` (default)

Leaf implementation: branch count inside the file is the tie to win. Use
when few things depend on the file.

### `composable`

Orchestrator or integration boundary. Fan-out is the gate; fan-in and
instability are advisories.

### `secure`

The file handles untrusted input. SECURE is zero-tolerance, so a tie that
lands here is a blocker rather than a gradient.

### `navigable`

A file agents keep having to read and edit. Nesting divergence is the gate.
A file can pass SIMPLE and still fail NAVIGABLE.

## Example

`topos/mcp/src/server.rs` (MCP entry point, few callers, lots of internal
orchestration): use `simple` — the SIMPLE generator reflects real quality.

`topos/engine/src/core/omega.rs` (the classifier, imported by every evaluation
path): use `composable` — coupling quality is the main lever here.

`topos/engine/src/adapters/gitnexus.rs` (parses untrusted subprocess output):
use `secure` — external input crossing a trust boundary is a known footgun;
the SECURE generator is the relevant target.

## Switching mid-loop

Changing priority changes which generator wins an equal gate score, and on
the CLI which generator the ranking concedes last. It does not change the
numeric score. For the concession order, pass `preferences.ranking`.
