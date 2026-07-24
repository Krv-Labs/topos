//! Shared classification step for `evaluate` and `inspect`: build every
//! representation the characteristic morphism needs and classify once.
//!
//! Split out of `evaluate.rs` -- `inspect.rs` already depended on this
//! function through it, which was really a sign it belongs to neither
//! command specifically.

use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::functors::probes::uast::abstractness::AbstractnessRepresentation;
use topos_engine::graphs::base::Representation;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use topos_engine::graphs::mdg::models::{GraphNode, GraphRelationship};

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
    fn classify_with_representations_survives_deeply_nested_supported_languages() {
        const DEPTH: usize = 10_000;
        let open = "(".repeat(DEPTH);
        let close = ")".repeat(DEPTH);
        let cases = [
            ("python", format!("x = {open}1{close}\n")),
            ("rust", format!("const X: i32 = {open}1{close};\n")),
            ("javascript", format!("const x = {open}1{close};\n")),
            ("typescript", format!("const x: number = {open}1{close};\n")),
            ("cpp", format!("int x = {open}1{close};\n")),
            ("go", format!("package p\nvar x = {open}1{close}\n")),
        ];

        for (language, source) in cases {
            let mut morphism = ProgramMorphism::new(source, language);
            let result =
                classify_with_representations(&CharacteristicMorphism, &mut morphism, None);

            assert!(result.is_parseable, "failed for {language}");
        }
    }
}
