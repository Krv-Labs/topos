//! `topos evaluate` — run the characteristic morphism over one or more
//! files and print each verdict plus a directory-wide rollup.
//!
//! Ported from `topos/cli/commands/quality.py::evaluate` and
//! `topos/cli/evaluation.py`, scoped down per issue #147: this prints
//! scores + raw metrics only. The Python original's colored gauges,
//! "Lowest-hanging fruit"/"Needs attention" ranking tables, and the
//! per-file "Suggestions"/"Security Findings" sections
//! (`topos/cli/diagnostics.py`) all read from
//! `evaluation::{suggestions,suppression,security_guidance}`, which are
//! not yet ported to `topos-core` — follow-up work once those land.
//!
//! # COMPOSABLE / GitNexus
//!
//! Unless `--no-composable` is passed, this command also attempts to
//! attach a [`ModuleDependencyGraph`] (COMPOSABLE generator): it checks
//! whether `<cwd>/.gitnexus` (or `--gitnexus-dir`) is present and fresh
//! via the same staleness classifier the MCP server uses
//! (`topos_mcp::evaluation::depgraph_status`), and if it's missing or
//! stale, generates it by shelling out to `gitnexus analyze
//! --skip-agents-md` (streaming its output live, since generation can
//! take a while). Any failure here — GitNexus not installed, generation
//! failing, a schema mismatch — degrades gracefully to SIMPLE/SECURE
//! only with a one-line `stderr` notice; it never fails the whole
//! evaluate run, matching how the MCP tools already treat COMPOSABLE as
//! "not measured" rather than "failed" when coupling data is
//! unavailable.
//!
//! [`ModuleDependencyGraph`]: topos_core::graphs::mdg::object::ModuleDependencyGraph
//! [`ProgramDependenceGraph`]: topos_core::graphs::pdg::object::ProgramDependenceGraph

use std::path::{Path, PathBuf};

use clap::Args;
use topos_core::adapters::discovery::collect_source_files;
use topos_core::adapters::gitnexus::{
    current_git_branch, generate_depgraph, gitnexus_available, resolve_lbug_store,
};
use topos_core::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_core::core::morphism::ProgramMorphism;
use topos_core::evaluation::policies::base::Priority;
use topos_core::functors::probes::uast::abstractness::AbstractnessRepresentation;
use topos_core::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};
use topos_core::graphs::base::Representation;
use topos_core::graphs::mdg::object::ModuleDependencyGraph;
use topos_mcp::evaluation::{depgraph_status, resolve_gitnexus_dir};

#[derive(Args)]
pub struct EvaluateArgs {
    /// Files or directories to evaluate.
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,
    /// Recursively evaluate directories.
    #[arg(short = 'r', long)]
    pub recursive: bool,
    /// Source language for parsing and file discovery when paths are directories.
    #[arg(long, default_value = "python")]
    pub language: String,
    /// Skip GitNexus detection/generation; score SIMPLE/SECURE only.
    #[arg(long)]
    pub no_composable: bool,
    /// Override the `.gitnexus` directory (default: `<cwd>/.gitnexus`).
    #[arg(long)]
    pub gitnexus_dir: Option<String>,
}

pub fn run(args: EvaluateArgs) -> Result<(), String> {
    if !SUPPORTED_LANGUAGES.contains(&args.language.as_str()) {
        return Err(format!(
            "unsupported language '{}' (expected one of: {})",
            args.language,
            SUPPORTED_LANGUAGES.join(", ")
        ));
    }
    let suffixes =
        language_file_suffixes(&args.language).expect("checked against SUPPORTED_LANGUAGES above");
    let files = collect_source_files(&args.paths, suffixes, args.recursive);
    if files.is_empty() {
        return Err(format!(
            "no {} source files found (expected suffixes: {})",
            args.language,
            suffixes.join(", ")
        ));
    }

    let mut mdg = if args.no_composable {
        None
    } else {
        match std::env::current_dir() {
            Ok(project_root) => resolve_composable_mdg(&project_root, args.gitnexus_dir.as_deref()),
            Err(e) => {
                eprintln!("gitnexus: could not resolve current directory ({e}); evaluating SIMPLE/SECURE only.");
                None
            }
        }
    };

    let classifier = CharacteristicMorphism;
    let mut results = Vec::with_capacity(files.len());
    for file in &files {
        let mut morphism = ProgramMorphism::from_file(file, args.language.clone())
            .map_err(|e| format!("reading {}: {e}", file.display()))?;
        if let Some(g) = mdg.as_mut() {
            g.target_file = file.to_string_lossy().into_owned();
        }
        let result = classify_with_representations(&classifier, &mut morphism, mdg.as_ref());
        println!("{}", file.display());
        print_classification(&result);
        println!();
        results.push(result);
    }

    if results.len() > 1 {
        println!("Directory rollup ({} files)", results.len());
        println!("{}", "-".repeat(40));
        let overall = classifier.combine_dimensions(&results);
        for dim in ["simple", "composable", "secure"] {
            if let Some(val) = overall.get(dim) {
                println!("  {dim}: {val}");
            }
        }
    }
    Ok(())
}

/// Ensure a fresh `.gitnexus` build exists for `project_root` and load its
/// `ModuleDependencyGraph`, or return `None` (with an explanatory `stderr`
/// notice) if that isn't possible. Never returns an `Err` — COMPOSABLE is
/// optional and its absence must not fail the whole evaluate run.
fn resolve_composable_mdg(
    project_root: &Path,
    gitnexus_dir_override: Option<&str>,
) -> Option<ModuleDependencyGraph> {
    let status = depgraph_status(
        gitnexus_dir_override,
        project_root,
        &project_root.to_string_lossy(),
    );

    let gitnexus_dir = match status.state {
        "present" => resolve_gitnexus_dir(gitnexus_dir_override, project_root)?,
        "missing" | "stale" => {
            if !gitnexus_available() {
                eprintln!(
                    "gitnexus not found on PATH — evaluating SIMPLE/SECURE only. \
                     Install with `npm install -g gitnexus` to enable COMPOSABLE, \
                     or pass --no-composable to silence this."
                );
                return None;
            }
            eprintln!("Generating dependency graph (gitnexus)...");
            let result = generate_depgraph(project_root, /* capture = */ false, None);
            if !result.ok {
                eprintln!(
                    "gitnexus: {} — evaluating SIMPLE/SECURE only.",
                    result.message
                );
                return None;
            }
            resolve_gitnexus_dir(gitnexus_dir_override, project_root)?
        }
        _ => {
            // schema_mismatch | invalid_dir | branch_not_indexed | load_error
            eprintln!(
                "gitnexus: {} — evaluating SIMPLE/SECURE only.",
                status
                    .detail
                    .unwrap_or_else(|| format!("COMPOSABLE unavailable ({})", status.state))
            );
            return None;
        }
    };

    let branch = current_git_branch(project_root);
    let resolved = resolve_lbug_store(&gitnexus_dir, branch.as_deref());
    let lbug_path = match resolved.path {
        Some(path) if path.is_dir() => path,
        _ => {
            eprintln!(
                "gitnexus: no indexed store found at {} — evaluating SIMPLE/SECURE only.",
                gitnexus_dir.display()
            );
            return None;
        }
    };

    match ModuleDependencyGraph::from_json_dir(&lbug_path, project_root.to_string_lossy()) {
        Ok(graph) => Some(graph),
        Err(e) => {
            eprintln!(
                "gitnexus: failed to load dependency graph ({e}) — evaluating SIMPLE/SECURE only."
            );
            None
        }
    }
}

/// Build the CFG (SIMPLE), PDG (diagnostic), CPG (SECURE), and — when
/// `mdg` is provided — MDG (COMPOSABLE) representations for `morphism`
/// and classify it. `mdg` is expected to already have its `target_file`
/// set to the file currently being classified.
pub(crate) fn classify_with_representations(
    classifier: &CharacteristicMorphism,
    morphism: &mut ProgramMorphism,
    mdg: Option<&ModuleDependencyGraph>,
) -> ClassificationResult {
    let cfg = morphism.build_cfg().cloned();
    let pdg = morphism.build_pdg().cloned();
    let cpg = morphism.build_cpg().cloned();
    let abstractness = morphism
        .ast
        .as_ref()
        .map(|ast| AbstractnessRepresentation::new(&ast.uast_root));
    let mut representations: Vec<&dyn Representation> = Vec::new();
    if let Some(cfg) = &cfg {
        representations.push(cfg);
    }
    if let Some(pdg) = &pdg {
        representations.push(pdg);
    }
    if let Some(cpg) = &cpg {
        representations.push(cpg);
    }
    if let Some(abstractness) = &abstractness {
        representations.push(abstractness);
    }
    if let Some(mdg) = mdg {
        representations.push(mdg);
    }
    classifier.classify_detailed(morphism, &representations, Priority::default())
}

/// Print a verdict, per-generator scores, and raw metrics for one result.
pub(crate) fn print_classification(result: &ClassificationResult) {
    if !result.is_parseable {
        println!("  {}", result.summary());
        return;
    }
    println!("  Verdict: {}", result.summary());
    for dim in ["simple", "composable", "secure"] {
        let Some(val) = result.dimensions.get(dim) else {
            continue;
        };
        let score = result.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
        println!("    {dim}: {val} [{score:.0}%]");
    }
    if !result.raw_metrics.is_empty() {
        println!("  Raw metrics:");
        let mut keys: Vec<&String> = result.raw_metrics.keys().collect();
        keys.sort();
        for key in keys {
            let value = result.raw_metrics[key];
            println!("    {key}: {value:.3}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use topos_core::graphs::mdg::models::{GraphNode, GraphRelationship};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_cli_evaluate_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn node(id: &str, label: &str, file_path: &str) -> GraphNode {
        let mut properties = HashMap::new();
        properties.insert(
            "filePath".to_string(),
            serde_json::Value::String(file_path.to_string()),
        );
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            properties,
        }
    }

    fn rel(id: &str, source: &str, target: &str, rel_type: &str) -> GraphRelationship {
        GraphRelationship {
            id: id.to_string(),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: rel_type.to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties: HashMap::new(),
        }
    }

    #[test]
    fn classify_with_representations_scores_composable_when_mdg_present() {
        let classifier = CharacteristicMorphism;
        let mut morphism = ProgramMorphism::new("def f():\n    return 1\n", "python");

        let mut mdg = ModuleDependencyGraph::new("a.py");
        mdg.add_node(node("File:a.py", "File", "a.py"));
        mdg.add_node(node("File:b.py", "File", "b.py"));
        mdg.add_relationship(rel("i1", "File:a.py", "File:b.py", "IMPORTS"));

        let without = classify_with_representations(&classifier, &mut morphism, None);
        assert!(
            !without.dimensions.contains_key("composable"),
            "composable must not appear without an MDG representation"
        );

        let with = classify_with_representations(&classifier, &mut morphism, Some(&mdg));
        assert!(
            with.dimensions.contains_key("composable"),
            "composable must appear once an MDG representation is attached"
        );
    }

    #[test]
    fn resolve_composable_mdg_returns_none_for_override_outside_project_root_without_shelling_out()
    {
        // An override outside `project_root` is rejected by
        // `resolve_gitnexus_dir`/`depgraph_status` before any
        // availability check or subprocess call, so this stays
        // deterministic regardless of whether gitnexus happens to be
        // installed on the machine running the test.
        let project_root = temp_dir("root");
        let outside = temp_dir("outside");

        let result = resolve_composable_mdg(&project_root, Some(&outside.to_string_lossy()));
        assert!(result.is_none());

        std::fs::remove_dir_all(&project_root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }
}
