//! UAST Abstractness probe.
//!
//! Martin's Abstractness (A): the fraction of a module's type
//! declarations that are abstract (trait / interface / protocol /
//! abstract class) rather than concrete (struct / class / enum / union
//! / type alias).
//!
//! Paired with Instability (`mdg.instability`), this drives the
//! Distance from the Main Sequence gate (`mdg.main_sequence_distance`,
//! [`crate::evaluation::policies::composable`]) instead of gating raw
//! instability against a fixed band — see issue #124.

use std::collections::HashMap;

use crate::graphs::base::Representation;
use crate::graphs::uast::models::{AttributeValue, UASTNode};

const ABSTRACT_TYPE_KINDS: &[&str] = &["trait", "interface", "abstractClass", "protocol"];
const CONCRETE_TYPE_KINDS: &[&str] = &["class", "struct", "enum", "union", "typeAlias"];

/// Languages whose UAST mappers populate the `typeKind` attribute needed
/// to classify type declarations as abstract vs. concrete. JavaScript is
/// permanently absent -- plain JS has no `interface`/`abstract class`
/// syntax at all (that is a TypeScript-only extension to the shared
/// grammar), so there is nothing in a `.js` file's grammar to ever
/// classify as abstract; reporting `0.0` for every JS file would be
/// indistinguishable from "this file happens to declare no types."
const ABSTRACTNESS_SUPPORTED_LANGUAGES: &[&str] = &["python", "rust", "go", "typescript", "cpp"];

/// Adapts [`calculate_abstractness`] to the [`Representation`] protocol,
/// so it merges into the same `composable`-dimension metrics map as
/// [`crate::graphs::mdg::object::ModuleDependencyGraph`].
///
/// Unlike `ModuleDependencyGraph` (built from GitNexus graph data),
/// Abstractness is purely a property of a single file's UAST -- no
/// dependency graph is required, so it is always available for any file
/// in a supported language (see [`ABSTRACTNESS_SUPPORTED_LANGUAGES`]).
pub struct AbstractnessRepresentation<'a> {
    pub uast_root: &'a UASTNode,
}

impl<'a> AbstractnessRepresentation<'a> {
    pub fn new(uast_root: &'a UASTNode) -> Self {
        AbstractnessRepresentation { uast_root }
    }
}

impl Representation for AbstractnessRepresentation<'_> {
    fn name(&self) -> &str {
        "abstractness"
    }

    // Merges into the same raw-metrics dict as ModuleDependencyGraph so
    // Φ_COMPOSABLE can pair instability with abstractness (issue #124).
    fn dimension(&self) -> &str {
        "composable"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        if !ABSTRACTNESS_SUPPORTED_LANGUAGES.contains(&self.uast_root.lang.as_str()) {
            return HashMap::new();
        }
        HashMap::from([(
            "mdg.abstractness".to_string(),
            calculate_abstractness(self.uast_root),
        )])
    }
}

fn walk<'a>(root: &'a UASTNode, out: &mut Vec<&'a UASTNode>) {
    out.push(root);
    for child in &root.children {
        walk(child, out);
    }
}

/// Fraction of classifiable `TypeDecl` nodes in `root` that are abstract.
///
/// Returns `abstract_count / (abstract_count + concrete_count)`, or
/// `0.0` when the module declares zero classifiable type declarations.
/// This is a real, meaningful value — a functions-only module (e.g. a
/// typical `main.rs` orchestrator with no struct/trait/enum) is
/// legitimately 0% abstract, not "undefined." Whether Abstractness is
/// *applicable at all* for a given file is a language-support question,
/// decided by the caller, not by this function. `TypeDecl` nodes with
/// no recognized `typeKind` attribute (e.g. Rust `impl_item` blocks, Go
/// type aliases to primitives) are excluded from both the numerator and
/// the denominator.
pub fn calculate_abstractness(root: &UASTNode) -> f64 {
    let mut nodes = Vec::new();
    walk(root, &mut nodes);

    let mut abstract_count = 0usize;
    let mut concrete_count = 0usize;
    for node in nodes {
        if node.kind != "TypeDecl" {
            continue;
        }
        let Some(AttributeValue::Str(type_kind)) = node.attributes.get("typeKind") else {
            continue;
        };
        if ABSTRACT_TYPE_KINDS.contains(&type_kind.as_str()) {
            abstract_count += 1;
        } else if CONCRETE_TYPE_KINDS.contains(&type_kind.as_str()) {
            concrete_count += 1;
        }
    }

    let total = abstract_count + concrete_count;
    if total == 0 {
        return 0.0;
    }
    abstract_count as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;
    use std::collections::HashMap;

    fn uast(source: &str, language: &str) -> UASTNode {
        parse_source(source, language, None).unwrap().uast_root
    }

    #[test]
    fn no_type_declarations_is_zero() {
        let root = uast("fn main() {}\n", "rust");
        assert_eq!(calculate_abstractness(&root), 0.0);
    }

    #[test]
    fn all_concrete_is_zero() {
        let root = uast("struct A;\nstruct B;\n", "rust");
        assert_eq!(calculate_abstractness(&root), 0.0);
    }

    #[test]
    fn all_abstract_is_one() {
        let root = uast("trait A {}\ntrait B {}\n", "rust");
        assert_eq!(calculate_abstractness(&root), 1.0);
    }

    #[test]
    fn mixed_is_the_ratio() {
        let root = uast("trait A {}\nstruct B;\nstruct C;\nstruct D;\n", "rust");
        assert_eq!(calculate_abstractness(&root), 0.25);
    }

    #[test]
    fn unclassified_type_decl_excluded_from_ratio() {
        // impl_item maps to TypeDecl but is never given a typeKind
        // attribute, so it must not dilute the denominator.
        let root = uast("trait A {}\nstruct B;\nimpl B {}\n", "rust");
        assert_eq!(calculate_abstractness(&root), 0.5);
    }

    #[test]
    fn synthetic_node_with_no_attributes_is_ignored() {
        let root = UASTNode {
            kind: "TypeDecl".to_string(),
            lang: "rust".to_string(),
            native: crate::graphs::uast::models::NativeRef {
                parser: "test".to_string(),
                parser_version: "unknown".to_string(),
                node_kind: "struct_item".to_string(),
            },
            attributes: HashMap::new(),
            children: Vec::new(),
            id: "synthetic".to_string(),
            span: crate::graphs::uast::models::SourceSpan {
                file: None,
                start_byte: 0,
                end_byte: 0,
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
        };
        assert_eq!(calculate_abstractness(&root), 0.0);
    }
}
