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
