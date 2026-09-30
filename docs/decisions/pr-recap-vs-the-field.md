# `topos pr-recap` against the field, and how it becomes a GitHub Action

Status: written 2026-09-21 alongside the pr-recap v2 prototype (`docs/decisions/pr-recap-refactor-tracing.md`).
Competitor facts come from official documentation fetched that day; "—" means the vendor's docs do not
claim the capability, not that it is absent.

## What the card does that review bots do not

| Capability | pr-recap | CodeRabbit | Copilot review | GitHub Code Quality | CodeScene | SonarQube Cloud | Codacy / Qlty / DeepSource | Greptile / Bito / Sourcery / Qodo | RefactoringMiner action |
|---|---|---|---|---|---|---|---|---|---|
| Structural before → after per file (medal, four pillars) | yes | — | — | coverage delta only | Code Health `10.0 → 9.0` | new-code scope, not delta | coverage / complexity deltas | — | — |
| Project-level regression when every touched file looks fine | yes (rollup) | — | — | — | — | — (new-code blind spot) | — | — | — |
| Split tracing: which new file came from which parent, which symbols moved | yes (graph `DEFINES` diff + UAST ledger) | — | — | — | — | — | — | — | refactoring list, no metrics |
| Complexity ledger with a balancing invariant (moved / new glue / removed) | yes | — | — | — | — | — | — | — | — |
| Coupling delta from a real dependency graph, split-aware | yes (fan-out excluding a parent's own children) | — | — | — | text warning | — | — | sequence diagrams only | — |
| Cosmetic-change detector (score moved, syntax tree did not) | yes | — | — | — | — | — | — | — | — |
| Deterministic, reproducible, no LLM | yes (`--json` reproduces the card) | no | no | rules yes | yes | yes | partly | no | yes |
| Line-level "where to look" with a fix | yes (gate hotspots) | inline comments | inline comments | inline findings | findings | inline issues | inline issues | inline comments | deep links |
| Blocks merge | exit code today; check-run planned | pre-merge checks | partial | rulesets | gates | required check | gates | status check | — |

Reading: CodeScene is the only product with a structural before → after score, and it already markets
"deterministic". The ground nobody holds is the combination of a lattice verdict, a function-level move
ledger that must balance, and a split-aware coupling delta. That combination is what the card leads with.

## Where the prototype is weak today

| Gap | Effect | Fix |
|---|---|---|
| COMPOSABLE needs GitNexus installed and two `gitnexus analyze` runs (~6 s each on 160 files) | Without it the card says "not measured" and splits fall back to import lines + ledger | Ship GitNexus in the Action image; cache `.git/topos-pr-<N>/` between runs |
| Rename detection is name + similarity ≥ 0.8 on UAST kind sequences | A renamed-and-rewritten function reads as removed + new | Add token TF-IDF (RefDiff) as a second signal |
| Anonymous callbacks dominate ledgers in React code | Hidden by default, counted as "N anonymous callbacks" | Name inference already covers `const X = () =>`; extend to `useCallback` results assigned to props |
| Cluster detection only pairs Added children with Modified/Deleted parents | Code moved between two existing files is reported, not clustered | Promote `moved_between_existing` to a "MOVE" row type |
| Cross-repository PRs are refused | Forks cannot be reviewed | Fetch the head SHA from the fork remote in the Action |
| Card is the only tested renderer against real PRs | GitHub markdown was checked by fixture only | Post to a test PR and iterate on the comment |

## Next steps

1. Merge the prototype behind the existing `pr-recap` command; nothing else changes for users.
2. Dogfood on every Topos PR for two weeks with `topos pr-recap <N>` in CI (compact card in the log, exit code
   non-blocking), and file each false SPLIT / false SUSPICIOUS as an issue.
3. Promote `moved_between_existing` to a `MOVE` row and add the TF-IDF rename signal.
4. Ship the Action.

## GitHub Action shape

The CLI already emits everything the Action needs: `--format github` prints a sticky comment with the marker
`<!-- topos-pr-recap:v2 -->` on line 1, `--json` carries the document, `--compact` fits the job log, and the
exit code encodes the headline: 0 pass, 1 regression (REGRESSION, SCORE DOWN or SUSPICIOUS), 2 error.

```yaml
# .github/workflows/topos-pr-recap.yml
name: Topos structural review
on:
  pull_request:
    types: [opened, synchronize, reopened]
permissions:
  contents: read
  pull-requests: write
  checks: write
jobs:
  recap:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - name: Install topos and GitNexus
        run: |
          curl -fsSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | sh
          npm install -g gitnexus@1.6.8
      - name: Cache coupling stores
        uses: actions/cache@v4
        with:
          path: .git/topos-pr-${{ github.event.pull_request.number }}
          key: topos-pr-${{ github.event.pull_request.number }}-${{ github.event.pull_request.head.sha }}
      - name: Review
        id: recap
        continue-on-error: true
        env:
          GH_TOKEN: ${{ github.token }}
          PR: ${{ github.event.pull_request.number }}
        # An explicit `shell: bash` runs as `bash -eo pipefail`; the default is
        # `bash -e`, where `tee` would hide the exit code of `topos`.
        shell: bash
        run: |
          # Write the comment and the document first: under `-e` a regression
          # (exit 1) would stop the script before they exist.
          topos pr-recap "$PR" --format github > recap.md || true
          topos pr-recap "$PR" --json > recap.json || true
          # Last, so its status (0 pass, 1 regression, 2 error) is the step's.
          topos pr-recap "$PR" --compact | tee -a "$GITHUB_STEP_SUMMARY"
      - name: Post or update the sticky comment
        uses: marocchino/sticky-pull-request-comment@v2
        with:
          header: topos-pr-recap
          path: recap.md
      - name: Gate
        if: steps.recap.outcome == 'failure'
        run: exit 1   # make this a required check once the false-positive rate is known
```

What the Action must respect, verified against GitHub's limits: comment bodies cap at 65,536 characters
(`--format github` trims cluster details from the smallest cluster up and says so), the job summary is
1 MiB per step with silent overflow (the compact card is ≤ 12 lines), check-run annotations are 50 per
request (hotspots are already capped at two per card), and Mermaid renders in PR comments. Edit the
comment in place via the marker; do not delete and repost, which notifies every subscriber.

Two engineering items before the Action is safe as a required check: fetch fork head SHAs so cross-repo
PRs work, and record in `--json` which children were attached by the import-line fallback rather than by
graph evidence, so a reviewer can tell how much of the card rests on the coupling stores.
