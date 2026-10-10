# Contributing

## Git history and releases

- `main` is a linear series of squash-merged PRs. Don't use merge commits, and don't push directly to `main`.
- Each PR delivers one coherent outcome. Its Conventional Commit title becomes the permanent commit subject.
- Every notable user-visible PR adds its entry under `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog headings). Internal-only churn needs no entry.
- Decide release scope after the work lands. The release PR comes last: it moves `[Unreleased]` into `## [X.Y.Z] - YYYY-MM-DD`, bumps versions, and is tagged. Base it on current `main`, and never keep long-lived release branches.
- Stack PRs only when a child structurally depends on an unmerged parent, and prefer pushing to the parent instead. After the parent squash-merges, replant the child with `git rebase --onto origin/main <old-parent-tip> <child>`.
