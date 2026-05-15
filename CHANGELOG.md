# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-05-15

### Added
- Introduced the 3-pillar code quality evaluation model (Simple, Composable, Secure).
- Added Heyting Algebra support for partial confidence code evaluation.
- Added new evaluation modules: `CharacteristicMorphism` and `ClassificationResult`.
- Added new representation models: `ControlFlowGraph`, `CodePropertyGraph`, `ModuleDependencyGraph`.

### Changed
- Major architectural overhaul transitioning from experimental 0.x releases.
- Consolidated evaluation logic to operate on the new structural code quality metrics.
- Updated documentation and README to reflect the new 3-pillar approach.
- **Breaking:** Previous experimental APIs and CLI commands from 0.x are no longer compatible and have been removed.
