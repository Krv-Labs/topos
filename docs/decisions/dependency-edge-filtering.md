# Filtering false dependency edges

Status: **ACCEPTED**.

## Problem

COMPOSABLE and the pr-recap coupling gates (`import_cycle`, `fan_in_growth`,
`blast_radius`) read the dependency graph GitNexus builds. GitNexus resolves a
call or an import by the bare name, so it links code to repository files the
code never uses:

- Rust: `std::fs::write` becomes a call to a private `fn write` in another
  crate of the same workspace.
- Python: `import json` becomes an import of `pkg/http/response/json.py`.
- TypeScript/JavaScript: `import { EditorState } from "@codemirror/state"`
  becomes an import of `packages/laser-pointer/src/state.ts`.

The graph keeps no import specifier on an edge, but every `File` node carries
its source, so the specifier can be recovered by parsing the file.

## Decision

When a graph loads (`ModuleDependencyGraph::from_lbug_path`), Topos removes an
edge only when the source proves it false. Each rule keeps the edge whenever
another reading is possible.

| Rule | Drops | Keeps |
|---|---|---|
| `rust_private_cross_crate` | `CALLS` from one crate to a free function of another crate (nearest `Cargo.toml` in the graph) marked `isExported: false` | Calls inside one crate (child modules reach private ancestors through `super`), methods (trait impl methods carry no `pub`), graphs without `Cargo.toml` nodes |
| `python_stdlib_import` | `IMPORTS` to module `M` when every import in the source that names `M` is an absolute standard-library import (`import json`, `from json import x`, `import asyncio.subprocess`) | Relative imports, repository imports naming `M`, imports of a name the repository also has as a top-level module or package, files without content |
| `js_bare_package_import` | `IMPORTS` to file `F` when every specifier in the source that names `F` is a bare package (`@codemirror/state`, `roughjs/bin/math`) | Relative and absolute specifiers; `paths` aliases and `baseUrl` entries from the nearest `tsconfig.json`/`jsconfig.json` and its relative `extends`; workspace package names from `package.json`; `#`, `~`, and `@/` aliases; specifiers whose path after the first segment spells out the target's trailing path (bundler aliases such as `@site/src/initialData`); every edge from a file whose config cannot be read in full, including one that extends a package config |

The standard-library list is the union of `sys.stdlib_module_names` for
CPython 3.11–3.14 plus the modules 3.9 and 3.10 still shipped, embedded in
`topos/engine/src/graphs/mdg/python_stdlib.rs`.

The filter lives in the loader, not in `FileGraph::build`, so every consumer
sees one graph: `mdg.fan_out` (the COMPOSABLE gate), `mdg.fan_in`,
`mdg.coupling`, `mdg.instability`, and the pr-recap file graph. A
`FileGraph`-only filter would have left the COMPOSABLE gate counting the same
false calls the pr-recap gates had stopped counting.

`topos_depgraph_status` returns `dropped_edges` (counts per rule) and
`topos depgraph status --json` lists every removed edge with its files and
rule, so each removal can be checked against the source.

## Measured effect

Graphs built with GitNexus 1.6.8 on shallow clones, comparing `topos
evaluate -r --json` before and after:

| Repository | Edges removed | File pairs no longer linked | Files whose `mdg.fan_out` changed | COMPOSABLE verdicts changed |
|---|---|---|---|---|
| topos (Rust) | 72 Rust | 32 of 956 | 27 of 264 | 1 (`mcp/src/pr_recap/store.rs`: 11 → 8, now COMPOSABLE) |
| scrapy (Python) | 93 Python | 79 of 2,706 | 0 of 557 | 0 |
| excalidraw (TypeScript) | 2 JS/TS | 2 of 3,991 | 0 | 0 |

Every removed edge in topos and scrapy, and both in excalidraw, was checked
against the source: each targets a private free function in another crate,
or a module named only by a standard-library or bare package import.

The filter adds about 0.1 s to a cold graph load on excalidraw and nothing
measurable on topos or scrapy. JS/TS sources are parsed only when a quoted
string that could be a bare specifier ends in the target's name.

Python and TypeScript removals touch only `IMPORTS`, so they change
`mdg.coupling` and the instability advisory, not the fan-out gate. In scrapy,
97 files changed coupling and the COMPOSABLE score moved by up to about 10
points either way through the instability advisory; no verdict changed.

## Known limits

- TypeScript `paths` aliases that GitNexus routes to the wrong file
  (`@excalidraw/element/types` → `packages/excalidraw/types.ts`) are kept.
  Fixing them means resolving `paths` against the file tree, which is a
  separate change.
- Dynamic imports with computed specifiers, and Python modules wired by
  string (plugin settings, `importlib.import_module`), are invisible to
  GitNexus and to this filter.
- A standard-library name that a third-party import also mentions
  (`import ssl` next to `from twisted.internet.ssl import ...`) keeps the edge.
- Private methods in inherent `impl` blocks are kept with trait methods,
  because the graph does not say which `impl` a method belongs to.
