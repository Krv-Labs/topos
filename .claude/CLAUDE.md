# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **Note:** The core project architecture, rules, evaluation model (SIMPLE, COMPOSABLE, SECURE, NAVIGABLE), and refactoring workflows have been centralized. 
> 
> **You MUST read `AGENTS.md` at the repository root (or `.agents/AGENTS.md`) for the canonical project rules.**

## Writing Style

Always use **American English spelling** — "optimize" not "optimise", "analyze" not "analyse", "modeling" not "modelling", etc.

## Claude Tools & MCP Configuration
- You are configured to use the Topos MCP tools. Use `topos_get_doc` or similar MCP endpoints for dynamic help if `AGENTS.md` lacks specific details.
- Always follow the closed-loop refactoring recipe documented in the centralized `AGENTS.md`.

## Context & Compaction State Preservation
When summarizing or compacting context during multi-step implementations:
- Always preserve:
  1. The checklist of completed vs pending task IDs (e.g. Task 1, 2 vs Task 13).
  2. Baseline measurements, test commands, and equivalence verification outputs.
  3. Active worktree diffs and modified file paths.
- Omit:
  1. Raw unedited file reads that are still accessible on disk.
  2. Intermediate syntax error logs that have already been resolved.