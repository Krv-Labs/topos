//! Context-budget ratchet for the MCP tool-definition surface.
//!
//! Every agent session pays for the JSON wire definition of *all* registered
//! tools (name + description + inputSchema + annotations). These tests are
//! upper-bound *ratchets*, not equality checks: ceilings match the former
//! Python FastMCP suite (`tests/mcp/test_context_budget.py`) so the Rust
//! rewrite does not silently regrow the discovery surface.
//!
//! Token cost is approximated as `chars / 4` — the same crude heuristic used
//! for the Python baseline.

#[cfg(test)]
mod tests {
    use crate::server::ToposServer;

    /// Baseline from Python (2026-07-06 measured 33_736; ceiling = +~750).
    const TOTAL_CEILING_CHARS: usize = 34_500;
    const PER_TOOL_CEILING_CHARS: usize = 4_500;

    fn approx_tokens(chars: usize) -> usize {
        chars / 4
    }

    fn wire_sizes() -> Vec<(String, usize, String)> {
        ToposServer::new()
            .list_tool_defs()
            .into_iter()
            .map(|tool| {
                let name = tool.name.to_string();
                let wire = serde_json::to_string(&tool).expect("serialize tool wire def");
                let chars = wire.len();
                (name, chars, wire)
            })
            .collect()
    }

    fn report(sizes: &[(String, usize, String)]) -> String {
        let mut lines: Vec<String> = sizes
            .iter()
            .map(|(name, chars, _)| {
                format!("{chars:8} chars (~{:5} tok)  {name}", approx_tokens(*chars))
            })
            .collect();
        let total: usize = sizes.iter().map(|(_, c, _)| *c).sum();
        lines.push(format!(
            "{total:8} chars (~{:5} tok)  TOTAL",
            approx_tokens(total)
        ));
        lines.join("\n")
    }

    #[test]
    fn total_tool_surface_under_ceiling() {
        let mut sizes = wire_sizes();
        sizes.sort_by(|a, b| b.1.cmp(&a.1));
        let total: usize = sizes.iter().map(|(_, c, _)| *c).sum();
        assert!(
            total <= TOTAL_CEILING_CHARS,
            "MCP tool surface grew to {total} chars (~{} tok), ceiling {TOTAL_CEILING_CHARS}.\n{}",
            approx_tokens(total),
            report(&sizes)
        );
    }

    #[test]
    fn per_tool_surface_under_ceiling() {
        let sizes = wire_sizes();
        let over: Vec<(String, usize)> = sizes
            .iter()
            .filter(|(_, c, _)| *c > PER_TOOL_CEILING_CHARS)
            .map(|(n, c, _)| (n.clone(), *c))
            .collect();
        assert!(
            over.is_empty(),
            "Tool(s) exceed per-tool ceiling {PER_TOOL_CEILING_CHARS} chars: {over:?}.\n{}",
            report(&sizes)
        );
    }

    #[test]
    fn tool_surface_has_current_refactor_routing() {
        let blob: String = wire_sizes()
            .into_iter()
            .map(|(_, _, wire)| wire)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !blob.contains("STRONGLY PREFERRED for real refactor loops"),
            "stale side-by-side guidance leaked into tool metadata"
        );
        assert!(
            !blob.contains("Read this first on every new refactor session"),
            "stale session-start guidance leaked into tool metadata"
        );
        assert!(
            !blob.contains("topos_assess_improvement validates each accepted refactor"),
            "stale assess_improvement routing leaked into tool metadata"
        );
        assert!(
            blob.contains("topos_assess_worktree_change"),
            "expected worktree-change routing in tool metadata"
        );
    }

    #[test]
    #[ignore = "convenience: cargo test -p topos-mcp dump_tool_surface -- --ignored --nocapture"]
    fn dump_tool_surface() {
        let mut sizes = wire_sizes();
        sizes.sort_by_key(|b| std::cmp::Reverse(b.1));
        println!("{}", report(&sizes));
    }
}
