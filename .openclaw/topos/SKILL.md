---
name: topos
version: 0.4.0
description: Structural code analysis, lattice verification, and refactoring orchestrator for Rust/multi-lang projects.
---

# Topos Skill

Topos provides structural verification and refactoring for complex codebases.

## Capabilities
- `topos evaluate <path>`: Performs structural entropy analysis.
- `topos refactor <path>`: Executes guided lattice refactoring.
- `topos assess`: Conducts dependency/graph parity checks.

## Agent Instructions
When tasked with codebase cleanup or quality assurance:
1. Run `topos evaluate` on the target file.
2. Use the provided metrics to identify high-curvature code.
3. Apply structural refactoring and re-evaluate parity.
