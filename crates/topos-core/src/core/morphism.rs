//! `Morphism` — source code viewed as an arrow between objects.
//!
//! In the category of programs, we treat source code not as static text,
//! but as a "morphism" (`f: A → B`) between computational states.
//!
//! # Mathematical inspiration
//!
//! A program is a map that transforms an input domain (object `A`) into
//! an output codomain (object `B`). As code becomes a "commodity" via
//! LLMs, the identity of the morphism matters less than its structural
//! invariants.
//!
//! Two morphisms `f, g: A → B` are considered equivalent if they produce
//! the same observable behavior — but in our topos, we also care about
//! their internal structure. A morphism that is "commodity code" may
//! compute correctly but lack the structural integrity of a "verified"
//! morphism.
//!
//! The characteristic map `χ: Morphism → Ω` assigns each morphism an
//! evaluation value in our Heyting algebra, capturing this nuanced view
//! of correctness — landing with [`crate::core::omega`]'s consumer,
//! `evaluation::characteristic_morphism` (issue #144).

use std::path::{Path, PathBuf};

use crate::graphs::ast::dispatch::parse_source;
use crate::graphs::base::Representation;

use super::object::ProgramObject;

/// A program viewed as a transformation between computational states.
///
/// `ProgramMorphism` is the central abstraction of topos. It encapsulates
/// source code along with its parsed AST representation, providing the
/// foundation for evaluation by the subobject classifier.
///
/// # Categorical interpretation
///
/// In category theory, a morphism `f: A → B` is an arrow between objects.
/// Here, the source code IS the morphism — it defines how to transform
/// inputs (domain) into outputs (codomain). The AST captures the
/// "internal structure" of this transformation; additional
/// representations capture inter-module structure.
pub struct ProgramMorphism {
    /// The raw source code.
    pub source: String,
    /// The programming language (default: `"python"`).
    pub language: String,
    /// Optional path to the source file.
    pub filepath: Option<PathBuf>,
    /// The parsed AST representation. Only `None` for a language
    /// [`crate::graphs::ast::dispatch`] doesn't support.
    pub ast: Option<ProgramObject>,
    /// Additional representations (dep graph, etc.) attached to this
    /// morphism for multi-axis evaluation.
    pub representations: Vec<Box<dyn Representation>>,
    // `_cfg` / `_pdg` / `_cpg` caches land with issue #143, once
    // `ControlFlowGraph` / `ProgramDependenceGraph` / `CodePropertyGraph`
    // exist to cache. `build_cfg` / `build_pdg` / `build_cpg` follow them.
    // `classify` lands with issue #144
    // (`evaluation::characteristic_morphism::CharacteristicMorphism`).
}

impl ProgramMorphism {
    /// Construct a morphism from source, parsing it immediately — "the
    /// source code IS the morphism" is not just narrative, the AST is
    /// always attempted before this returns (mirrors the Python
    /// `__post_init__` auto-parse).
    ///
    /// `ast` is `None` only if `language` isn't one of the six
    /// [`crate::graphs::ast::languages::SUPPORTED_LANGUAGES`] — callers
    /// can still hold a `ProgramMorphism` for an unsupported language;
    /// [`Self::is_valid`] correctly reports `false` in that case.
    pub fn new(source: impl Into<String>, language: impl Into<String>) -> Self {
        let source = source.into();
        let language = language.into();
        let ast = Self::parse(&source, &language, None);
        ProgramMorphism {
            source,
            language,
            filepath: None,
            ast,
            representations: Vec::new(),
        }
    }

    fn parse(source: &str, language: &str, file: Option<&str>) -> Option<ProgramObject> {
        let result = parse_source(source, language, file).ok()?;
        Some(ProgramObject::new(
            result.tree,
            result.source,
            result.language,
        ))
    }

    /// Create a morphism from a source file.
    pub fn from_file(
        filepath: impl AsRef<Path>,
        language: impl Into<String>,
    ) -> std::io::Result<Self> {
        let filepath = filepath.as_ref();
        let source = std::fs::read_to_string(filepath)?;
        let language = language.into();
        let file = filepath.to_string_lossy().into_owned();
        let ast = Self::parse(&source, &language, Some(&file));
        Ok(ProgramMorphism {
            source,
            language,
            filepath: Some(filepath.to_path_buf()),
            ast,
            representations: Vec::new(),
        })
    }

    /// Whether the morphism represents syntactically valid code.
    pub fn is_valid(&self) -> bool {
        self.ast.as_ref().is_some_and(|ast| ast.is_valid())
    }

    /// A human-readable identifier for this morphism.
    pub fn name(&self) -> String {
        match &self.filepath {
            Some(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<morphism>".to_string()),
            None => {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                self.source.hash(&mut hasher);
                format!("<morphism:{:04}>", hasher.finish() % 10000)
            }
        }
    }
}

impl PartialEq for ProgramMorphism {
    /// Equality based on source and language, matching the Python original.
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.language == other.language
    }
}

impl Eq for ProgramMorphism {}

impl std::hash::Hash for ProgramMorphism {
    /// Hashes on `(source, language)`, matching the Python original.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.hash(state);
        self.language.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_morphism_basic() {
        let morphism = ProgramMorphism::new("def add(a, b): return a + b", "python");
        assert_eq!(morphism.source, "def add(a, b): return a + b");
        assert!(morphism.is_valid());
        assert!(morphism.ast.is_some());
    }

    #[test]
    fn program_morphism_invalid_syntax() {
        let morphism = ProgramMorphism::new("def incomplete_func(", "python");
        assert!(morphism.ast.is_some());
        assert!(!morphism.is_valid());
    }

    #[test]
    fn program_morphism_unsupported_language_is_none_not_panic() {
        let morphism = ProgramMorphism::new("PROGRAM. HELLO.", "cobol");
        assert!(morphism.ast.is_none());
        assert!(!morphism.is_valid());
    }

    #[test]
    fn program_morphism_supports_all_six_dispatch_languages() {
        // "rust" was the one language the temporary bootstrap parser
        // (issue #141) didn't support; graphs::ast::dispatch (#142) now
        // covers all six.
        for language in ["python", "rust", "javascript", "typescript", "cpp", "go"] {
            let morphism = ProgramMorphism::new("", language);
            assert!(morphism.ast.is_some(), "{language} should parse");
        }
    }

    #[test]
    fn program_morphism_equality() {
        let m1 = ProgramMorphism::new("x = 1", "python");
        let m2 = ProgramMorphism::new("x = 1", "python");
        let m3 = ProgramMorphism::new("y = 2", "python");
        assert!(m1 == m2);
        assert!(m1 != m3);

        use std::hash::{Hash, Hasher};
        let hash_of = |m: &ProgramMorphism| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            m.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash_of(&m1), hash_of(&m2));
    }

    #[test]
    fn program_morphism_from_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("topos_core_test_{}.py", std::process::id()));
        std::fs::write(&path, "print('hello world')").unwrap();

        let morphism = ProgramMorphism::from_file(&path, "python").unwrap();
        assert_eq!(morphism.filepath.as_deref(), Some(path.as_path()));
        assert_eq!(morphism.name(), path.file_name().unwrap().to_string_lossy());

        std::fs::remove_file(&path).ok();
    }
}
