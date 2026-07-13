//! File roles — predicates that classify what *role* a source file
//! plays, so the characteristic morphism can relax or skip specific
//! quality gates when a file is structurally special rather than
//! ordinary logic.
//!
//! The first role is the **import/export-only entrypoint module** —
//! `__init__.py`, `mod.rs`/`lib.rs`, `index.ts`, and friends — which are
//! trivial re-export hubs and should not be penalized for low entropy
//! or high instability. Additional roles (generated code, vendored
//! code, test files, …) can be added here as further predicates over
//! the same [`crate::core::morphism::ProgramMorphism`].

use std::path::Path;

use crate::core::morphism::ProgramMorphism;

/// True iff `morphism` is an import/export-only entrypoint module.
pub fn is_entrypoint_module(morphism: &ProgramMorphism) -> bool {
    let Some(filepath) = morphism.filepath.as_deref() else {
        return false;
    };
    if !entrypoint_filename_hint(filepath, &morphism.language) {
        return false;
    }
    is_entrypoint_source_only(&morphism.source, &morphism.language)
}

fn entrypoint_filename_hint(path: &Path, language: &str) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower_name = filename.to_lowercase();
    match language {
        "python" => filename == "__init__.py",
        "rust" => matches!(filename, "mod.rs" | "lib.rs"),
        "typescript" => matches!(lower_name.as_str(), "index.ts" | "index.tsx"),
        "javascript" => matches!(lower_name.as_str(), "index.js" | "index.mjs" | "index.cjs"),
        "cpp" => {
            let suffix = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            matches!(suffix.as_str(), "hpp" | "hh" | "hxx")
        }
        _ => false,
    }
}

fn is_entrypoint_source_only(source: &str, language: &str) -> bool {
    let lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }

    match language {
        "python" => python_entrypoint_only(&lines),
        "typescript" | "javascript" => lines.iter().all(|line| {
            line.starts_with("import ")
                || line.starts_with("export *")
                || line.starts_with("export {")
                || line.starts_with("export type ")
                || line.starts_with("export interface ")
                || line.starts_with("//")
                || line.starts_with("#!")
                || line.starts_with("/*")
                || line.starts_with('*')
                || line.ends_with("*/")
        }),
        "rust" => lines.iter().all(|line| {
            line.starts_with("use ")
                || line.starts_with("pub use ")
                || line.starts_with("pub mod ")
                || line.starts_with("mod ")
                || line.starts_with("extern crate ")
                || line.starts_with("#!")
                || line.starts_with("#[")
                || line.starts_with("//")
                || line.starts_with("/*")
                || line.starts_with('*')
                || line.ends_with("*/")
        }),
        "cpp" => lines.iter().all(|line| {
            line.starts_with("#include")
                || line.starts_with("#pragma once")
                || line.starts_with("//")
                || line.starts_with("/*")
                || line.starts_with('*')
                || line.ends_with("*/")
        }),
        _ => false,
    }
}

/// Tracks open brackets so multiline `from x import (...)` and
/// `__all__ = [...]` continuation lines (e.g. `assess,`) are accepted.
fn python_entrypoint_only(lines: &[&str]) -> bool {
    let mut depth: i32 = 0;
    for &line in lines {
        if depth > 0 {
            depth += bracket_delta(line);
            continue;
        }
        let is_allowed = line.starts_with('#')
            || line.starts_with("import ")
            || line.starts_with("from ")
            || line.starts_with("__all__")
            || matches!(line, "[" | "]" | "(" | ")")
            || line.starts_with('\'')
            || line.starts_with('"');
        if !is_allowed {
            return false;
        }
        if line.starts_with("import ") || line.starts_with("from ") || line.starts_with("__all__") {
            depth += bracket_delta(line);
        }
    }
    true
}

fn bracket_delta(line: &str) -> i32 {
    let opens = line.matches('(').count() + line.matches('[').count();
    let closes = line.matches(')').count() + line.matches(']').count();
    opens as i32 - closes as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_init_with_only_imports_is_entrypoint() {
        let mut morphism =
            ProgramMorphism::new("from foo import bar\n__all__ = [\"bar\"]\n", "python");
        morphism.filepath = Some("/pkg/__init__.py".into());
        assert!(is_entrypoint_module(&morphism));
    }

    #[test]
    fn python_init_with_real_logic_is_not_entrypoint() {
        let mut morphism = ProgramMorphism::new("def f():\n    return 1\n", "python");
        morphism.filepath = Some("/pkg/__init__.py".into());
        assert!(!is_entrypoint_module(&morphism));
    }

    #[test]
    fn non_entrypoint_filename_is_never_entrypoint() {
        let mut morphism = ProgramMorphism::new("import foo\n", "python");
        morphism.filepath = Some("/pkg/util.py".into());
        assert!(!is_entrypoint_module(&morphism));
    }

    #[test]
    fn rust_lib_rs_with_only_mod_declarations_is_entrypoint() {
        let mut morphism = ProgramMorphism::new("pub mod core;\npub mod graphs;\n", "rust");
        morphism.filepath = Some("/crate/src/lib.rs".into());
        assert!(is_entrypoint_module(&morphism));
    }
}
