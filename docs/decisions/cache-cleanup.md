# Cache cleanup and safe uninstall

Status: **IMPLEMENTED** for inventory, status, discovery, targeted cleanup,
and prune. The command surface is `topos cache status`, `discover`, `clean`,
and `prune`; `topos uninstall` remains responsible only for harness
installation state.

Topos writes two very different kinds of data. The first is harness installation
state: MCP entries in agent configs, the `install.json` ledger, and the
directories install had to create. The second is shared cache data: PR stores
under a repository's Git common directory and a cross-repository inventory under
`state_dir()/cache-index`. Cache data can be large, is shared by several
worktrees and repositories, and may be in use by another process while the user
runs a command. Treating both under one "leave no trace" rule made uninstall
delete bookkeeping that outlived the installation, and made cleanup look safer
than the inventory alone can prove.

This note records the rules that keep those two lifetimes separate.

## The inventory is a hint, not authority

`cache-index` records candidate locations, ownership hints, timestamps, and
compatibility metadata. It is deliberately **not** a deletion manifest:

- Records are merged monotonically under a file lock and written atomically, so
  a stale record can hide or misdescribe a location, but it cannot make an
  arbitrary path deletable.
- A record may outlive the directory it names, may name a path that has since
  been replaced, or may be missing for a store that exists on disk.
- Discovery and status never authorize deletion. Discovery may register what
  it finds in the inventory, but neither adopts a legacy store.

Deletion revalidates the thing on disk. The expected slot
`<git-common-dir>/topos-pr-<N>` must be a real directory rather than a symlink;
the ownership marker must match that common directory and PR; any `base`/`head`
worktree that still exists must be a live, detached registration naming that
path; the contents must contain nothing outside the disposable graph; and a
concurrent lease must be free. A marked store may already be partially deleted,
so cleanup does not require a graph or a complete commit record. An unmarked
legacy store is different: `clean` may adopt it only after validating the exact
slot, allowed top-level entries, two-commit record, live detached registrations
at those commits, and both worktree contents. If the inventory and the disk
disagree, the disk wins and the store is skipped or reported. A global
inventory lookup expands the candidate set; it does not relax any check.

## Marker and layout are separate from generator compatibility

Three schemas are intentionally separate:

| Layer | Schema | What it versions |
| --- | --- | --- |
| Inventory document | `topos.cache.v1` | Status/discover/clean/prune output |
| Inventory entry | `topos.cache_entry.v1` | One record in `cache-index/entries/` |
| PR store marker | `topos.pr_store.v1` | A managed store's ownership and layout |

A PR marker also carries `layout` and an optional `generator`. `layout` is the
store's on-disk contract (what `base/`, `head/`, `commits`, and their graphs
mean). `generator` names what produced a graph when that is known. They are
recorded separately because they move independently: a GitNexus upgrade can
change the generator without changing Topos's store layout, and a layout change
must invalidate a store even if the same generator produced it.

Generator metadata is diagnostic. It does not by itself make a store unowned,
and it is not proof that a newer generator's output is compatible. A marker
with an unsupported schema or layout, a mismatched common directory or PR, or a
broken marker is not adopted or deleted. Ordinary project `.gitnexus` graphs
carry no Topos compatibility claim at all; they are GitNexus data.

## GitNexus owns ordinary graph invalidation

Topos loads GitNexus graphs for COMPOSABLE and reports their locations, but it
does not duplicate GitNexus's freshness or invalidation rules. A project's
`.gitnexus/` is owned by GitNexus, and the `base`/`head` graphs inside a PR
store are built by GitNexus as part of that store's lifecycle. Cache cleanup
does not delete or rebuild an ordinary project graph, and it does not treat a
graph as garbage merely because Topos would build it differently.

Duplicating invalidation would create two authorities with different versions,
schema tolerances, and freshness markers. Until Topos can verify a stable
GitNexus contract and prove that deleting a graph cannot race a current build,
graph invalidation stays with GitNexus.

## Lock order

Every path that reads or removes a PR store follows one lock order:

1. the in-process per-PR mutex,
2. the per-PR file lease, `<git-common-dir>/topos-pr-<N>.lease`,
3. the short repository worktree lock, `<git-common-dir>/topos-worktrees.lock`,
4. the inventory lock as a leaf.

A PR lease is never taken while holding a repository or inventory lock, and the
repository lock never spans analysis or scoring. Cleanup probes the lease
non-blockingly; `busy` means another worker owns the store and the cleanup
leaves it alone. The lease file lives beside the PR directory, not inside it,
so removing the directory cannot remove the lock. Lock files are durable
coordination points: deleting a lock file while a process may hold it creates a
split-lock race on the next open. Uninstall therefore never enters a
repository's Git common directory, and neither cleanup nor uninstall removes
`topos-pr-*.lease` or `topos-worktrees.lock`.

## Cleanup validation and partial retry

`clean` is targeted: one PR with `--pr N`, or every known managed PR store with
`--all-prs`. It is idempotent and safe to rerun. `prune` is the same deletion
path with an explicit age threshold. Both validate every target immediately
before destructive work, regardless of how the target was discovered.

A deletion that is interrupted must be recognizable rather than guessed at.
Cleanup records deletion intent in the store (the `topos-delete.json` marker),
removes Git worktree registrations before the data they describe, and reports
`partial` when it cannot finish. Rerunning the same command revalidates the
remaining state and continues; already removed stores are simply absent. Busy,
unsafe, and legacy-ambiguous stores are skipped and named. An applying `clean`
exits non-zero if any store was skipped, partial, busy, or unsafe; `prune`
exits non-zero for partial, busy, or unsafe results, since a store younger than
the threshold is expected to remain. `--dry-run` never writes a deletion marker
or removes anything.

Legacy stores without a marker are report-only in status and discovery.
`clean` may adopt one, but only after validating the exact slot, the allowed
top-level entries, the two-commit record, the live detached worktree
registrations, and the contents of both sides; the marker is written before any
destructive work. `prune` never adopts a store: it only removes stores that
already carry a valid marker. Anything ambiguous or foreign is a diagnostic.

## The 24-hour prune grace

`prune --older-than DURATION` has a hard minimum of 24 hours. A shorter window
is raised to 24 hours even with `--yes`. The floor protects a store that was
just built or used, absorbs clock skew and timestamp granularity, and keeps a
routine `--all-prs` script from becoming an accidental immediate purge. Targeted
`clean --pr N` is not age-gated: once the lease is free and the store validates,
the user named exactly what to remove.

There is no timer, daemon, or startup sweep. Cleanup only runs when a user
invokes `clean` or `prune`.

## What cleanup does not do

- It does not delete agent snapshots. `topos_begin_refactor` baselines live in
  the snapshot store and have their own TTL; cache cleanup is not their owner.
- It does not delete ordinary project `.gitnexus` graphs, GitNexus snapshots,
  or GitNexus registry entries. GitNexus owns them.
- It does not run background garbage collection or reclaim space merely because
  `status --sizes` reported a number.
- It does not remove repository leases or locks, and it does not remove the
  shared inventory directory.
- It does not treat a size estimate as reclaimable disk. Hardlinks,
  copy-on-write storage, and Git's shared object database all make the estimate
  larger than the space a deletion would actually free.

## Uninstall preserves shared bookkeeping

`topos uninstall` still removes every harness installation artifact it created,
including the `install.json` ledger and the update-check cache. It does not
prune `state_dir()` when `state_dir()/cache-index` remains, and it reports the
retained path on the human output. With no retained cache bookkeeping, the
existing leave-no-trace behavior is unchanged. The created-directory pruning
list also filters the state directory and any retained cache path, so an older
ledger cannot take shared inventory down with harness state.

## Per-PR duplication is intentionally unoptimized

Each PR store carries its own `base` and `head` worktrees and their graphs.
Sharing objects through Git alternates, hardlinks, or a repository-level graph
could reduce disk use, but it would also complicate lease ownership, worktree
registration, partial deletion, and Git object reachability. It would also make
the `--sizes` estimate even less representative of reclaimable space.

This is a measurement-driven follow-up, not a claim that duplication is free.
`topos cache status --sizes` and observed cleanup rates should show whether
duplication is material before any sharing scheme is designed. Until then,
correctness and the ability to remove one PR without disturbing another take
precedence.
