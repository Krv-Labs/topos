# SIMPLE corpus

Pinned single-file references for the SIMPLE pillar sensitivity sweep.

Regenerate with:

```bash
uv run python demos/sensitivity/curate.py
```

This downloads PyPI sdists into `demos/sensitivity/.cache/`, copies the selected
files here, and writes `manifest.json` with baseline lattice verdicts.
