# PyInstaller onefile vs. onedir

Issue [#108](https://github.com/Krv-Labs/topos/issues/108) listed onedir
packaging as an out-of-scope follow-up for faster warm starts. This doc
records the measured tradeoff and why we're keeping onefile as the only
shipped release format for now.

## Measured numbers

Built locally (macOS arm64) from the same `PYINSTALLER_ARGS` used for the
slim release binary (see `.github/workflows/release.yml`), varying only
`--onefile` vs `--onedir`:

| | onefile (slim) | onedir | delta |
|---|---|---|---|
| Distribution size | 44.95 MB (1 file) | 99 MB (571 files) | onedir ~2.2x larger on disk |
| `topos --version` warm | ~610 ms | ~70 ms | onedir ~9x faster |
| `topos --help` warm | ~615 ms | ~70 ms | onedir ~9x faster |
| `topos evaluate --help` warm | ~1616 ms | ~108 ms | onedir ~15x faster |

The onefile numbers already reflect this PR's optimizations (dropped
`--collect-all topos`, excluded the ECT dependency stack — see
[`cli-startup-benchmarks.md`](cli-startup-benchmarks.md)). The gap vs. onedir
is entirely PyInstaller onefile's per-run archive extraction to a temp
directory: there's no built-in extraction caching in PyInstaller (confirmed
via the upstream "extract only once" enhancement request,
[pyinstaller/pyinstaller#4994](https://github.com/pyinstaller/pyinstaller/issues/4994),
still open), so onefile re-pays that cost on every single invocation
regardless of archive size. onedir has no extraction step — the bootloader
loads the executable and its adjacent `_internal/` shared libraries directly.

If sub-second binary cold start ever becomes a hard requirement, onedir is
the lever that gets us there — shrinking the onefile archive further can
reduce extraction time somewhat, but can't eliminate the extraction step
itself.

## Why we're not switching the default artifact

Onedir isn't a drop-in flag change — it changes the release artifact from a
single file to a directory tree, and three independently-shipped consumers
in this repo currently hard-code "the artifact is exactly one file":

**`install.sh`** — downloads one asset named `topos-${platform}`, verifies
one SHA-256 against one line in `checksums.txt`, and `mv`s it directly to
`$INSTALL_DIR/topos`. `topos/cli/installation.py`'s PATH-scanning
(`find_topos_executables_on_path()`) checks `candidate.is_file()`, and
`topos/cli/commands/system.py`'s `uninstall` command removes a single file.
Supporting onedir here means downloading and extracting an archive, hashing
the archive instead of the raw binary, installing into a dedicated directory
with a wrapper/symlink at `$INSTALL_DIR/topos`, and making the PATH-scan and
uninstall logic directory-aware.

**The VS Code extension** (`extensions/vscode/`) — `stage-binary.js` copies
exactly one file to `extension/bin/topos`; `.vscodeignore` only un-ignores
that one path; `extension.ts`/`runtime.ts`'s download-fallback manifest
(`ManifestBinary`) is one URL + one SHA-256 per platform, downloaded and
hashed as a single file. Onedir support means bundling/downloading a whole
`_internal/` tree, changing the manifest shape, and re-tuning the 200 MiB
VSIX size gate now that a directory of hundreds of loose files ships instead
of one binary.

**`.github/workflows/release.yml`** — macOS codesigning currently signs one
executable. Onedir would require signing every `.so`/`.dylib` under
`_internal/` individually (or `--deep` signing the whole tree), which is a
known fragile area for notarization — missing even one embedded library
blocks the whole notarization ticket. `checksums.txt` generation
(`sha256sum topos-*`) also assumes flat files, not a directory.

There's no incremental path that upgrades just one of these three consumers
without breaking the "single file" contract the other two still expect.
Given the size and reliability of that migration, and that the current PR
already gets meaningful onefile wins (44.95 MB vs. 72.14 MB baseline, ~20-28%
faster warm start), we're keeping onefile as the sole shipped format.

## Recommendation

Keep onefile as the only release artifact. Revisit onedir only if sub-second
binary cold start becomes a hard product requirement — at that point, budget
for coordinated changes to `install.sh`, the VS Code extension's
staging/manifest/download code, and the release workflow's codesigning step,
not just a PyInstaller flag change.

## Related

- [`cli-startup-benchmarks.md`](cli-startup-benchmarks.md) — startup
  measurement harness and the onefile optimizations shipped in this PR.
- [`ect-coverage-release-sizes.md`](ect-coverage-release-sizes.md) — binary
  size breakdown by artifact variant.
- Release build: [`.github/workflows/release.yml`](../.github/workflows/release.yml)
