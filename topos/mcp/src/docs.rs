//! Embedded documentation content, shared by the `topos_get_doc` tool and
//! the `topos://docs/*` resources.
//!
//! Content is compiled in via `include_str!` (from `docs/content/*.md`),
//! so the server is a single self-contained binary — no runtime file reads,
//! matching the "single source of truth" the Python package achieved with
//! a shared content directory.

use crate::schemas::DocTopic;

pub const AGENT_CONTRACT: &str = include_str!("../docs/content/agent-contract.md");
pub const LATTICE: &str = include_str!("../docs/content/lattice.md");
pub const METRICS: &str = include_str!("../docs/content/metrics.md");
pub const PREFERENCES: &str = include_str!("../docs/content/preferences.md");
pub const PRIORITY: &str = include_str!("../docs/content/priority.md");
pub const WORKFLOWS: &str = include_str!("../docs/content/workflows.md");

/// Content for a documentation topic.
pub fn doc_content(topic: DocTopic) -> &'static str {
    match topic {
        DocTopic::AgentContract => AGENT_CONTRACT,
        DocTopic::Lattice => LATTICE,
        DocTopic::Metrics => METRICS,
        DocTopic::Preferences => PREFERENCES,
        DocTopic::Priority => PRIORITY,
        DocTopic::Workflows => WORKFLOWS,
    }
}

/// Content for a `topos://docs/<slug>` resource URI, or `None` for an
/// unknown slug.
pub fn doc_content_for_slug(slug: &str) -> Option<&'static str> {
    match slug {
        "agent-contract" => Some(AGENT_CONTRACT),
        "lattice" => Some(LATTICE),
        "metrics" => Some(METRICS),
        "preferences" => Some(PREFERENCES),
        "priority" => Some(PRIORITY),
        "workflows" => Some(WORKFLOWS),
        _ => None,
    }
}

/// The six resource slugs, in listing order.
pub const DOC_SLUGS: [&str; 6] = [
    "agent-contract",
    "lattice",
    "metrics",
    "priority",
    "preferences",
    "workflows",
];

/// Guards that the agent-visible prose stays consistent with the tool surface
/// actually registered on the router.
///
/// These exist because the Python→Rust rewrite flattened the tool inputs
/// (FastMCP wrapped every tool's arguments in a single `params` model; `rmcp`
/// takes them at the top level) while the docs kept teaching the old shape —
/// so every documented call example was a hard `unknown field` error.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::ToposServer;

    fn all_docs() -> Vec<(&'static str, &'static str)> {
        DOC_SLUGS
            .iter()
            .map(|slug| (*slug, doc_content_for_slug(slug).expect("slug has content")))
            .collect()
    }

    /// Every `DOC_SLUGS` entry must resolve, or `list_resources` advertises a
    /// `topos://docs/*` URI that `read_resource` then 404s on.
    #[test]
    fn every_slug_resolves() {
        for slug in DOC_SLUGS {
            assert!(
                doc_content_for_slug(slug).is_some(),
                "advertised resource slug `{slug}` has no content"
            );
        }
        assert!(doc_content_for_slug("nope").is_none());
    }

    /// Tool inputs are flat. No doc may teach the Python-era `params` wrapper.
    #[test]
    fn docs_do_not_teach_params_wrapper() {
        for (slug, body) in all_docs() {
            assert!(
                !body.contains("\"params\":"),
                "`{slug}` teaches the removed `params` wrapper; tool arguments are flat"
            );
        }
    }

    /// Same guard for the prompt — it is the artifact an LLM copies verbatim.
    #[test]
    fn refactor_prompt_does_not_teach_params_wrapper() {
        let text = crate::server::refactor_prompt_text_for_test();
        assert!(
            !text.contains("\"params\":"),
            "topos_refactor_until_ideal prompt teaches the removed `params` wrapper"
        );
        assert!(
            text.contains("\"filepath\""),
            "prompt should show a flat filepath argument"
        );
    }

    /// Every registered tool must be reachable from the embedded docs, so an
    /// agent that reads them learns the whole surface. Catches tools added to
    /// the router without a corresponding doc mention.
    #[test]
    fn every_registered_tool_is_documented() {
        let corpus: String = all_docs()
            .iter()
            .map(|(_, b)| *b)
            .collect::<Vec<_>>()
            .join("\n");
        let undocumented: Vec<String> = ToposServer::new()
            .list_tool_defs()
            .into_iter()
            .map(|t| t.name.to_string())
            .filter(|name| !corpus.contains(name.as_str()))
            .collect();
        assert!(
            undocumented.is_empty(),
            "tools registered but absent from every embedded doc: {undocumented:?}"
        );
    }

    /// The inverse: no doc may reference a `topos_*` tool that isn't
    /// registered, which is how `topos refactor`-style phantom guidance
    /// survives a rewrite.
    #[test]
    fn docs_reference_no_phantom_tools() {
        let registered: Vec<String> = ToposServer::new()
            .list_tool_defs()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        // `topos_`-prefixed identifiers that are legitimately not tools: the
        // crate/server name and the prompt. Extend this rather than loosening
        // the check.
        const NON_TOOL_IDENTS: [&str; 3] =
            ["topos_mcp", "topos_refactor_until_ideal", "topos_core"];
        for (slug, body) in all_docs() {
            for raw in body.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                if !raw.starts_with("topos_") || NON_TOOL_IDENTS.contains(&raw) {
                    continue;
                }
                if registered.iter().any(|r| r == raw) {
                    continue;
                }
                panic!(
                    "`{slug}` references `{raw}`, which is not a registered tool. \
                     If it is intentionally not a tool, add it to NON_TOOL_IDENTS."
                );
            }
        }
    }
}
