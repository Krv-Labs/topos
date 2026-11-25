# Changelog fragments

A PR with a notable user-visible change adds one file here instead of editing
`CHANGELOG.md`:

```
changes/<slug>.<kind>.md
```

- `<slug>`: short, lowercase, unique, e.g. `badge-command` or `pr-recap-forks`.
- `<kind>`: `added`, `changed`, `deprecated`, `removed`, `fixed` or `security`
  (the Keep a Changelog headings).
- Contents: one or more bullets, each starting with `- `, continuation lines
  indented two spaces, exactly as they should read in the changelog.

Example, `changes/pr-recap-forks.added.md`:

```markdown
- `topos pr-recap <N>` accepts pull requests from forks: the head commit is
  fetched from `origin`'s `refs/pull/<N>/head`.
```

No two PRs touch the same file, so they never conflict. The release PR runs
`python3 scripts/changelog.py release X.Y.Z`, which folds every fragment into
`## [X.Y.Z] - <date>` in `CHANGELOG.md` (in the order they landed) and deletes
them. CI runs `python3 scripts/changelog.py check`, which rejects a malformed
fragment and any entry written under `## [Unreleased]` directly.
