//! Pass A: one file at a time, both sides parsed once and scored.

use std::collections::HashSet;
use std::path::Path;

use topos_engine::config::ToposConfig;
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::core::object::ProgramObject;
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue, Generator};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::secure::score_secure;
use topos_engine::functors::probes::ast::complexity::{
    calculate_function_complexity_entries, FunctionComplexityEntry,
};
use topos_engine::functors::probes::cpg::danger::dangerous_api_reachable;
use topos_engine::functors::probes::cpg::taint::taint_flow_paths;
use topos_engine::functors::profunctors::ast::compare::calculate_ast_distance;
use topos_engine::functors::profunctors::uast::ledger::{snapshot_functions, FunctionSnapshot};
use topos_engine::graphs::cpg::object::CodePropertyGraph;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_mcp::schemas::SecurityFinding;
use topos_mcp::security_findings::dangerous_call_findings;

use super::git::{file_change, show_file, DiffEntry};
use super::hotspots::file_hotspots;
use super::model::*;
use super::verdict::{complexity_relocated, measured_verdict, medal, pillar_deltas, raw};
use crate::commands::classify::classify_with_representations;
use crate::commands::lang::detect_language;

/// One scored file with the intermediate state the later passes need.
pub(super) struct Scored {
    pub(super) recap: FileRecap,
    pub(super) before: ClassificationResult,
    pub(super) after: ClassificationResult,
    pub(super) after_src: String,
    pub(super) before_snapshots: Option<Vec<FunctionSnapshot>>,
    pub(super) after_snapshots: Option<Vec<FunctionSnapshot>>,
    pub(super) distance: Option<f64>,
}

/// One revision of one file, parsed once. Classification caches the
/// control-flow, dependence and property graphs on the morphism, so the
/// distance, the function walk, the hotspots and the security findings
/// all read that same parse.
pub(super) struct Side {
    morphism: ProgramMorphism,
    pub(super) result: ClassificationResult,
    /// The `.topos.toml` allowlist patterns covering this path.
    allow: HashSet<String>,
}

impl Side {
    /// Parse and classify `source` as the file at `path`: its language,
    /// and the graph node it is looked up by, both come from that path.
    /// SECURE is scored with the allowlist applied, as MCP does.
    fn new(
        source: &str,
        path: &str,
        graph: Option<&ModuleDependencyGraph>,
        priority: Priority,
        allow: HashSet<String>,
    ) -> Side {
        let language = detect_language(Path::new(path));
        let mut morphism = ProgramMorphism::with_path(source, language, path);
        let mut result =
            classify_with_representations(&CharacteristicMorphism, &mut morphism, graph, priority);
        if !allow.is_empty() {
            if let Some(cpg) = morphism.build_cpg() {
                allow_secure(&mut result, cpg, &allow);
            }
        }
        Side {
            morphism,
            result,
            allow,
        }
    }

    pub(super) fn source(&self) -> &str {
        &self.morphism.source
    }

    /// The syntax tree, only when the source parsed cleanly.
    pub(super) fn ast(&self) -> Option<&ProgramObject> {
        self.morphism
            .ast
            .as_ref()
            .filter(|_| self.morphism.is_valid())
    }

    pub(super) fn dangerous_calls(&mut self) -> Vec<SecurityFinding> {
        let allow = (!self.allow.is_empty()).then_some(&self.allow);
        self.morphism
            .build_cpg()
            .map(|cpg| dangerous_call_findings(cpg, usize::MAX, allow))
            .unwrap_or_default()
    }
}

/// Rescore SECURE with `allow` taken out of the counts, the way MCP's
/// `apply_allowlist` recomputes its gate. MCP's grade cap (acknowledged
/// risk never buys IDEAL) only changes the medal it displays, so it is
/// not applied here, where the pillar pass is what gates.
fn allow_secure(
    result: &mut ClassificationResult,
    cpg: &CodePropertyGraph,
    allow: &HashSet<String>,
) {
    const DANGEROUS: &str = "cpg.dangerous_calls";
    const TAINT: &str = "cpg.taint_flows";
    if !result.raw_metrics.contains_key(DANGEROUS) && !result.raw_metrics.contains_key(TAINT) {
        return;
    }
    let dangerous = dangerous_api_reachable(cpg, allow) as f64;
    let taint = taint_flow_paths(cpg, allow) as f64;
    result.raw_metrics.insert(DANGEROUS.to_string(), dangerous);
    result.raw_metrics.insert(TAINT.to_string(), taint);
    let secure = score_secure(dangerous, taint);
    let key = Generator::Secure.as_str();
    result.scores.insert(key.to_string(), secure.score);
    result.interpretation.extend(secure.interpretation);
    result.dimensions.insert(
        key.to_string(),
        if secure.achieved {
            Generator::Secure.value()
        } else {
            EvaluationValue::Slop
        },
    );
    let satisfied: Vec<Generator> = Generator::ALL
        .into_iter()
        .filter(|generator| result.dimensions.get(generator.as_str()) == Some(&generator.value()))
        .collect();
    result.lattice_element = verdict_from_generators(&satisfied);
}

/// The allowlist patterns `config` applies to `path`, relative to `repo`.
fn allow_patterns(config: &ToposConfig, repo: &Path, path: &str) -> HashSet<String> {
    config
        .entries_for(Some(&repo.join(path)))
        .into_iter()
        .map(|entry| entry.pattern.clone())
        .collect()
}

/// The function walk over one side of one file.
#[derive(Default)]
struct Functions {
    worst: Option<FunctionComplexityEntry>,
    snapshots: Option<Vec<FunctionSnapshot>>,
}

/// Snapshots are labelled `file` rather than the side's own path, so a
/// renamed file's functions are not read as moving out of it.
fn functions(side: &Side, file: &str) -> Functions {
    let Some(ast) = side.ast() else {
        return Functions::default();
    };
    Functions {
        worst: calculate_function_complexity_entries(&ast.uast_root, side.source())
            .into_iter()
            .max_by_key(|entry| entry.complexity),
        snapshots: Some(snapshot_functions(&ast.uast_root, side.source(), file)),
    }
}

fn function_ref(entry: &FunctionComplexityEntry) -> FunctionRef {
    FunctionRef {
        name: entry.qualified_name.clone(),
        line: entry.start_line,
        complexity: entry.complexity,
    }
}

/// What every file in one recap is scored against.
pub(super) struct Scoring<'a> {
    pub(super) repo: &'a Path,
    pub(super) base: &'a str,
    pub(super) head: &'a str,
    pub(super) base_graph: Option<&'a ModuleDependencyGraph>,
    pub(super) head_graph: Option<&'a ModuleDependencyGraph>,
    pub(super) priority: Priority,
    /// The project config, for its allowlist.
    pub(super) config: &'a ToposConfig,
}

impl Scoring<'_> {
    pub(super) fn score_file(&self, entry: &DiffEntry) -> Result<Scored, String> {
        let path = entry.path.clone();
        let change = file_change(&entry.status);
        let is_new = change == FileChange::Added;
        // A renamed file only exists under its old path at base.
        let base_path = entry.old_path.as_deref().unwrap_or(&path);
        let before_src = if is_new {
            String::new()
        } else {
            show_file(self.repo, self.base, base_path)?
        };
        let after_src = if self.head == "worktree" {
            std::fs::read_to_string(self.repo.join(&path))
                .map_err(|e| format!("reading {path}: {e}"))?
        } else {
            show_file(self.repo, self.head, &path)?
        };

        // The base graph only knows a renamed file by its old path. Aimed at
        // the new one it would find no File node and read zero coupling.
        let mut before = Side::new(
            &before_src,
            base_path,
            targeted(self.base_graph, base_path).as_ref(),
            self.priority,
            allow_patterns(self.config, self.repo, base_path),
        );
        let mut after = Side::new(
            &after_src,
            &path,
            targeted(self.head_graph, &path).as_ref(),
            self.priority,
            allow_patterns(self.config, self.repo, &path),
        );
        let distance = structural_distance(&before, &after);
        let before_functions = if is_new {
            Functions::default()
        } else {
            functions(&before, &path)
        };
        let after_functions = functions(&after, &path);
        let hotspots = file_hotspots(
            &path,
            after_functions.worst.as_ref(),
            &mut before,
            &mut after,
        );
        let (before, after) = (before.result, after.result);

        let before_verdict = measured_verdict(&before);
        let after_verdict = measured_verdict(&after);
        let (lines_added, lines_removed) = line_delta(&before_src, &after_src);
        let measured = self.base_graph.is_some() && self.head_graph.is_some();

        let recap = FileRecap {
            path: path.clone(),
            change,
            // Pass C decides this, once cluster fan-out is known.
            status: Headline::LateralMove,
            // The gates decide this, once every file is scored.
            severity: None,
            lines_before: before_src.lines().count(),
            lines_after: after_src.lines().count(),
            lines_added,
            lines_removed,
            medal_before: (!is_new).then(|| medal(before_verdict)),
            medal_after: after.is_parseable.then(|| medal(after_verdict)),
            pillars: pillar_deltas(&before, &after, is_new),
            structural_distance: distance,
            cosmetic: false,
            complexity_relocated_within_file: complexity_relocated(&before, &after),
            worst_function_before: before_functions.worst.as_ref().map(function_ref),
            worst_function_after: after_functions.worst.as_ref().map(function_ref),
            decisions_before: (!is_new).then(|| decisions(&before)).flatten(),
            decisions_after: decisions(&after),
            fan_in_before: measured.then(|| raw(&before, "mdg.fan_in")).flatten(),
            fan_in_after: measured.then(|| raw(&after, "mdg.fan_in")).flatten(),
            fan_out_before: measured.then(|| raw(&before, "mdg.fan_out")).flatten(),
            fan_out_after: measured.then(|| raw(&after, "mdg.fan_out")).flatten(),
            cluster: None,
            hotspots,
        };
        Ok(Scored {
            recap,
            before,
            after,
            after_src,
            before_snapshots: before_functions.snapshots,
            after_snapshots: after_functions.snapshots,
            distance,
        })
    }
}

/// A clone of the repository graph aimed at one file. Cloning is far
/// cheaper than re-reading the store for every path.
fn targeted(graph: Option<&ModuleDependencyGraph>, path: &str) -> Option<ModuleDependencyGraph> {
    let mut graph = graph?.clone();
    graph.target_file = path.to_string();
    Some(graph)
}

fn decisions(result: &ClassificationResult) -> Option<usize> {
    raw(result, "cfg.cyclomatic")
}

fn line_delta(before: &str, after: &str) -> (usize, usize) {
    let before_lines: std::collections::HashMap<&str, usize> = counts(before);
    let after_lines: std::collections::HashMap<&str, usize> = counts(after);
    let removed = before_lines
        .iter()
        .map(|(line, count)| count.saturating_sub(*after_lines.get(line).unwrap_or(&0)))
        .sum();
    let added = after_lines
        .iter()
        .map(|(line, count)| count.saturating_sub(*before_lines.get(line).unwrap_or(&0)))
        .sum();
    (added, removed)
}

fn counts(source: &str) -> std::collections::HashMap<&str, usize> {
    let mut counts = std::collections::HashMap::new();
    for line in source.lines() {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

fn structural_distance(before: &Side, after: &Side) -> Option<f64> {
    if before.source().is_empty() {
        return None;
    }
    Some(calculate_ast_distance(before.ast()?, after.ast()?).normalized_distance)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{commit_all, write_files, write_repo};
    use super::*;
    use crate::commands::gh::git;

    /// Aimed at the new path, the base graph finds no File node and reads
    /// zero coupling; a pure `git mv` then looks like coupling changed.
    #[test]
    fn a_renamed_file_reads_base_coupling_at_its_old_path() {
        use serde_json::Value;
        use std::collections::HashMap;
        use topos_engine::graphs::mdg::models::{GraphNode, GraphRelationship};

        fn node(id: &str, label: &str, path: Option<&str>) -> GraphNode {
            let mut properties = HashMap::from([("name".to_string(), Value::from(id))]);
            if let Some(path) = path {
                properties.insert("filePath".to_string(), Value::from(path));
            }
            GraphNode {
                id: id.to_string(),
                label: label.to_string(),
                properties,
            }
        }
        fn edge(source: &str, target: &str, kind: &str) -> GraphRelationship {
            GraphRelationship {
                id: format!("{source}-{kind}->{target}"),
                source_id: source.to_string(),
                target_id: target.to_string(),
                rel_type: kind.to_string(),
                confidence: 1.0,
                reason: String::new(),
                properties: Default::default(),
            }
        }
        // `path` defines `run`, which calls two functions in another file.
        fn graph(path: &str) -> ModuleDependencyGraph {
            let mut graph = ModuleDependencyGraph::new("");
            graph.add_node(node("File:mine", "File", Some(path)));
            graph.add_node(node("File:lib", "File", Some("src/lib.py")));
            graph.add_node(node("run", "Function", None));
            graph.add_relationship(edge("File:mine", "run", "DEFINES"));
            for callee in ["one", "two"] {
                graph.add_node(node(callee, "Function", None));
                graph.add_relationship(edge("File:lib", callee, "DEFINES"));
                graph.add_relationship(edge("run", callee, "CALLS"));
            }
            graph
        }

        let body = "def run():\n    return one() + two()\n";
        let (_keep, repo) = write_repo(&[("src/a.py", body)]);
        git(&repo, &["mv", "src/a.py", "src/b.py"]).unwrap();
        commit_all(&repo, "rename");
        let entry = DiffEntry {
            status: "R100".to_string(),
            path: "src/b.py".to_string(),
            old_path: Some("src/a.py".to_string()),
        };
        let (base_graph, head_graph) = (graph("src/a.py"), graph("src/b.py"));
        let scoring = Scoring {
            repo: &repo,
            base: "HEAD~1",
            head: "HEAD",
            base_graph: Some(&base_graph),
            head_graph: Some(&head_graph),
            priority: Priority::Secure,
            config: &ToposConfig::default(),
        };
        let scored = scoring.score_file(&entry).unwrap();
        assert_eq!(scored.recap.fan_out_after, Some(2));
        assert_eq!(scored.recap.fan_out_before, Some(2));
    }

    /// A file that changes language on the way (`.py` → `.js`) is read at
    /// base as the language it was then; parsed as its new language, the
    /// old source does not parse and the distance is lost.
    #[test]
    fn a_renamed_file_is_parsed_at_base_as_its_old_language() {
        let (_keep, repo) = write_repo(&[("src/run.py", "def run(cmd):\n    return cmd\n")]);
        git(&repo, &["rm", "-q", "src/run.py"]).unwrap();
        write_files(
            &repo,
            &[("src/run.js", "function run(cmd) {\n  return cmd;\n}\n")],
        );
        commit_all(&repo, "port");
        let entry = DiffEntry {
            status: "R050".to_string(),
            path: "src/run.js".to_string(),
            old_path: Some("src/run.py".to_string()),
        };
        let scoring = Scoring {
            repo: &repo,
            base: "HEAD~1",
            head: "HEAD",
            base_graph: None,
            head_graph: None,
            priority: Priority::Secure,
            config: &ToposConfig::default(),
        };
        let scored = scoring.score_file(&entry).unwrap();
        assert!(scored.before.is_parseable, "the base side parses as Python");
        assert!(scored.recap.structural_distance.is_some());
    }
}
