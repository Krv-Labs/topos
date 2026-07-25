//! Classification helpers: morphism → lattice verdict.

use std::path::Path;

use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::functors::probes::uast::abstractness::AbstractnessRepresentation;
use topos_engine::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};
use topos_engine::graphs::base::Representation;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;

use super::depgraph::load_dep_graph;

/// Run the classifier with CFG/PDG/CPG/Abstractness plus an optional MDG.
pub fn classify_morphism(
    morphism: &mut ProgramMorphism,
    priority: Priority,
    dep_graph: Option<&ModuleDependencyGraph>,
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
    if let Some(dep_graph) = dep_graph {
        representations.push(dep_graph);
    }

    CharacteristicMorphism.classify_detailed(morphism, &representations, priority)
}

/// Classify raw source. CFG / PDG / CPG always run; the COMPOSABLE
/// generator is unreachable without a ModuleDependencyGraph.
pub fn classify_code_string(
    code: &str,
    language: &str,
    priority: Priority,
) -> Result<ClassificationResult, String> {
    if !SUPPORTED_LANGUAGES.contains(&language) {
        let mut expected: Vec<&str> = SUPPORTED_LANGUAGES.to_vec();
        expected.sort_unstable();
        return Err(format!(
            "Unsupported language '{language}'; expected one of {expected:?}"
        ));
    }
    let mut morphism = ProgramMorphism::new(code, language);
    Ok(classify_morphism(&mut morphism, priority, None))
}

/// Map a file suffix to a tree-sitter language, defaulting to `python`.
pub fn detect_language(path: &Path) -> &'static str {
    let suffix = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for lang in SUPPORTED_LANGUAGES {
        if let Some(suffixes) = language_file_suffixes(lang) {
            if suffixes.contains(&suffix.as_str()) {
                return lang;
            }
        }
    }
    "python"
}

/// Classify a file, attaching every available representation.
///
/// Returns `(result, dep_graph, load_error)` so callers can cache the dep
/// graph for subsequent proposed-code evaluations.
#[allow(clippy::type_complexity)]
pub fn classify_file(
    path: &Path,
    priority: Priority,
    gitnexus_dir: Option<&Path>,
) -> Result<
    (
        ClassificationResult,
        Option<ModuleDependencyGraph>,
        Option<String>,
    ),
    String,
> {
    let language = detect_language(path);
    let mut morphism = ProgramMorphism::from_file(path, language)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (dep_graph, load_error) = load_dep_graph(gitnexus_dir, &path.to_string_lossy());
    let result = classify_morphism(&mut morphism, priority, dep_graph.as_ref());
    Ok((result, dep_graph, load_error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detect_language_by_suffix() {
        assert_eq!(detect_language(Path::new("a.rs")), "rust");
        assert_eq!(detect_language(Path::new("a.py")), "python");
        assert_eq!(detect_language(Path::new("a.tsx")), "typescript");
        assert_eq!(detect_language(Path::new("a.unknown")), "python");
    }

    #[test]
    fn classify_code_string_rejects_unknown_language() {
        assert!(classify_code_string("x = 1", "cobol", Priority::Simple).is_err());
    }

    #[test]
    fn classify_code_string_scores_python() {
        let result = classify_code_string("def f():\n    return 1\n", "python", Priority::Simple)
            .expect("classification runs");
        assert!(result.is_parseable);
        assert!(result.scores.contains_key("simple"));
    }
}
