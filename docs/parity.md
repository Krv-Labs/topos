# Drop-in parity and performance

Topos went all-Rust in v0.4.0 (issue [#159](https://github.com/Krv-Labs/topos/pull/159)):
the `topos` CLI and `topos-mcp` server are now compiled binaries with no
Python runtime. Two scripts in `scripts/` guard the promise that this is a
**drop-in replacement** for the last Python release, not a behavior change:

- `scripts/parity_check.py` — does the new binary produce the same
  `ClassificationResult` the old one did, file for file?
- `scripts/benchmark_perf.py` — how much faster is it?

Both are external-process harnesses: they shell out to two `topos` binaries
and diff/time stdout. Neither imports anything from a `topos` Python
package, because there isn't one anymore — the **reference** side is
resolved from a command string, defaulting to the last Python release
pulled straight from PyPI with `uvx`:

```
uvx --from topos-mcp==0.3.11 topos
```

The **candidate** side is `target/release/topos`, built from this worktree.

## Corpus

`parity/corpus/` is a small, self-contained, six-language fixture set
(Python, Rust, JavaScript, TypeScript, Go, C++) checked into the repo so
neither script depends on an external corpus or network access beyond
resolving the reference binary. Each language has:

- `report.{ext}` — a branchy classifier (nested `if`/`elif`/`switch`,
  a loop with an early break, a boolean short-circuit) plus one
  intentionally dangerous call (`os.system` / `Command::new("sh")` /
  `eval` / `exec.Command("sh", …)` / `std::system`), so every SIMPLE
  and SECURE raw metric gets exercised.
- `clean.{ext}` — two trivial functions, to confirm the two CLIs agree
  on the "nothing wrong here" case too.

## Running parity

```bash
cargo build --release -p topos-cli

# default: bundled corpus, all six languages, reference = PyPI 0.3.11 via uvx
python3 scripts/parity_check.py

# one language, or a different corpus entirely
python3 scripts/parity_check.py --corpus crates/topos-core/src --language rust

# pin a different reference release, or a local Python build
python3 scripts/parity_check.py --reference "uvx --from topos-mcp==0.3.11 topos"
python3 scripts/parity_check.py --reference /path/to/old/topos

# self-parity smoke test: candidate vs candidate must be 100% clean
python3 scripts/parity_check.py --reference target/release/topos
```

It runs `topos inspect <file> --json` through both binaries and diffs
`raw_metrics`, `scores`, and `dimensions`, normalizing the old CLI's
0–100 display scale against the Rust CLI's raw 0.0–1.0 floats
(`_normalize_scores` auto-detects which scale each side used, so the
same comparison also works for the candidate-vs-candidate self-parity
check). Real, understood divergences are allowlisted in
`KNOWN_DIVERGENCES` with the tracking issue; anything else fails the run.

Currently allowlisted:

- **`ast.max_function_complexity` (Python source only, issue [#153](https://github.com/Krv-Labs/topos/issues/153))** —
  the old CLI's native-Python complexity counter treats `elif`, `with`,
  `assert`, comprehensions, and boolean short-circuits as separate
  decision points; the Rust port's UAST-based counter uses the same
  `If`/`For`/`While`/`Match`/`Try` + boolean-check convention
  `cfg.cyclomatic` already uses, so the two metrics are internally
  consistent with each other but diverge from the old Python-only rule.
  Non-Python languages were already made language-neutral in v0.3.11 and
  match exactly.

Last validated against the published `topos-mcp==0.3.11` PyPI release:
**12/12 corpus files pass** (11 exact matches, 1 with only the allowlisted
`#153` divergence above).

## Running the benchmark

```bash
cargo build --release -p topos-cli
python3 scripts/benchmark_perf.py
python3 scripts/benchmark_perf.py --reference /path/to/old/topos   # pinned/local install
python3 scripts/benchmark_perf.py --candidate-only                 # Rust-only, no comparison
```

It measures two different things, because they have different causes:

1. **Per-invocation cost** (subprocess-per-file) — dominated by Python
   interpreter startup on the old side. This is what an agent or editor
   pays every time it calls the CLI once per file — the common case for
   an in-loop refactor check — and it's where the rewrite wins biggest.
2. **Whole-corpus throughput** (one `evaluate -r <dir>` call per CLI) —
   startup amortized across every file, isolating parse+analyze speed
   once the interpreter is already warm.

### Results (this branch vs. published `topos-mcp==0.3.11`, M-series Mac)

Two different corpus scales, since per-invocation and whole-corpus
throughput answer different questions (see above):

| | old (Python) | new (Rust) | speedup |
| --- | ---: | ---: | ---: |
| Per-invocation (`inspect --json`, 1 file, tiny fixture corpus) | ~440ms | ~12ms | **~37x** |
| Per-invocation (`inspect --json`, 1 file, real 92-file crate) | ~436ms | ~34ms | **~13.5x** |
| Whole-corpus (`evaluate -r`, 92 real `.rs` files) | ~1929ms | ~1101ms | **~1.8x** |
| Per-file, amortized (whole-corpus / 92 files) | ~21ms | ~12ms | **~1.8x** |

The per-invocation number holds whether the reference is a properly
`pip install`ed release or resolved fresh via `uvx` each call — Python
interpreter + import-machinery startup dominates either way, not tool
resolution overhead. The gap between the two numbers is itself the
interesting result: the whole-corpus case amortizes startup across every
file, so it isolates parse+analyze speed — a real, but much smaller, win —
while per-invocation is the number that matters for an agent calling the
CLI once per edited file, which is topos's primary use case
(`topos_assess_worktree_change`, `topos_evaluate_file`, pre-commit hooks).
Absolute times are hardware- and corpus-dependent, but the per-invocation
gap is structural (compiled binary vs. interpreter startup) and should
hold across machines.
