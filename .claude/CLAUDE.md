# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **Note:** The core project architecture, rules, v1.0.0 evaluation model (SIMPLE, COMPOSABLE, SECURE), and refactoring workflows have been centralized. 
> 
> **You MUST read `AGENTS.md` at the repository root (or `.agents/AGENTS.md`) for the canonical project rules.**

## Writing Style

Always use **American English spelling** — "optimize" not "optimise", "analyze" not "analyse", "modeling" not "modelling", etc.

## Claude Tools & MCP Configuration
- You are configured to use the Topos MCP tools. Use `topos_get_doc` or similar MCP endpoints for dynamic help if `AGENTS.md` lacks specific details.
- Always follow the closed-loop refactoring recipe documented in the centralized `AGENTS.md`.