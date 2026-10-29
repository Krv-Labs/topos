//! Dangerous-API reachability probe (CPG → ℝ).
//!
//! Counts call-site nodes whose callee text matches the per-language
//! registry of dangerous APIs. The match is intentionally textual: the
//! UAST mappers don't carry token text, so this slices the original
//! source by the CPG node's byte span and pattern-matches the result.
//!
//! No `regex` dependency: the one pattern needed
//! (`^[A-Za-z_][A-Za-z0-9_.]*\s*\(`) is a simple enough prefix scan that
//! pulling in a regex engine for it would be reaching for a bigger tool
//! than the job needs.

use std::collections::HashSet;

use crate::graphs::cpg::object::CodePropertyGraph;

/// Per-language registry of forbidden symbol names. Conservative — meant
/// to flag the obvious footguns, not to compete with a full SAST.
pub(crate) fn dangerous_apis(language: &str) -> &'static [&'static str] {
    match language {
        "python" => &[
            "eval",
            "exec",
            "compile",
            "pickle.loads",
            "yaml.load",
            "marshal.loads",
            "subprocess.call",
            "subprocess.Popen",
            "subprocess.run",
            "os.system",
            "os.popen",
            "__import__",
        ],
        "javascript" => &[
            "eval",
            "Function",
            "setTimeout",
            "setInterval",
            "innerHTML",
            "document.write",
            "child_process.exec",
        ],
        "typescript" => &[
            "eval",
            "Function",
            "innerHTML",
            "document.write",
            "child_process.exec",
        ],
        "rust" => &["unsafe", "transmute", "from_raw"],
        "cpp" => &["gets", "strcpy", "strcat", "sprintf", "scanf", "system"],
        "go" => &[
            "exec.Command",
            "exec.CommandContext",
            "os.StartProcess",
            "syscall.Exec",
            "syscall.ForkExec",
        ],
        _ => &[],
    }
}

/// Dangerous-API registry for `language` minus any allowlisted patterns.
///
/// A registry entry is dropped when it matches an allowlist pattern
/// under the same suffix-aware rules used for callee matching. An empty
/// `allow` returns the full registry unchanged — the canonical default.
pub fn effective_registry(language: &str, allow: &HashSet<String>) -> HashSet<&'static str> {
    let registry: HashSet<&'static str> = dangerous_apis(language).iter().copied().collect();
    if allow.is_empty() {
        return registry;
    }
    registry
        .into_iter()
        .filter(|api| match_registry_key(api, allow.iter().map(String::as_str)).is_none())
        .collect()
}

/// Count `CallExpr` nodes whose callee text matches the dangerous-API
/// registry for `cpg.language`. Matches both bare names (`eval`) and
/// dotted/qualified names (`pickle.loads`).
pub fn dangerous_api_reachable(cpg: &CodePropertyGraph, allow: &HashSet<String>) -> usize {
    let registry = effective_registry(&cpg.language, allow);
    if registry.is_empty() {
        return 0;
    }

    cpg.nodes
        .values()
        .filter(|node| node.kind() == "CallExpr")
        .filter(|node| {
            let text = cpg.node_text(node);
            let callee = callee_from_text(&text);
            !callee.is_empty() && matches_registry(&callee, registry.iter().copied())
        })
        .count()
}

/// Extract the dotted callee prefix from a call expression's text.
pub fn callee_from_text(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return String::new(),
    }
    let mut end = 0;
    for (i, c) in text.char_indices() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if text[end..].trim_start().starts_with('(') {
        text[..end].to_string()
    } else {
        String::new()
    }
}

/// The registry key `callee` matches, or `None`.
///
/// Exact membership wins; otherwise a `.`-qualified suffix match
/// (`foo.eval` against `eval`, `mypkg.pickle.loads` against
/// `pickle.loads`). A bare string suffix never matches, so
/// `page.waitForFunction` does not hit `Function`. `compile` skips
/// `re.compile` and `regex.compile`. Undotted keys of at most 3 chars
/// (in practice short allow patterns such as `run`) are exact-only.
/// Prefers the longest matching key so `pickle.loads` beats a
/// hypothetical bare `loads`.
pub fn match_registry_key<'a>(
    callee: &str,
    keys: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut best: Option<&'a str> = None;
    for key in keys {
        if key == callee {
            return Some(key);
        }
        if !key.contains('.') && key.len() <= 3 {
            continue;
        }
        let matches = if key == "compile" {
            (callee == "compile" || callee.ends_with(".compile"))
                && callee != "re.compile"
                && !callee.ends_with(".re.compile")
                && callee != "regex.compile"
                && !callee.ends_with(".regex.compile")
        } else {
            callee.ends_with(&format!(".{key}"))
        };
        if matches && best.is_none_or(|b| key.len() > b.len()) {
            best = Some(key);
        }
    }
    best
}

pub fn matches_registry<'a>(callee: &str, registry: impl Iterator<Item = &'a str>) -> bool {
    match_registry_key(callee, registry).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    #[test]
    fn detects_eval_call() {
        let source = "eval(user_input)\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert_eq!(dangerous_api_reachable(&cpg, &HashSet::new()), 1);
    }

    #[test]
    fn ignores_safe_calls() {
        let source = "print(user_input)\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert_eq!(dangerous_api_reachable(&cpg, &HashSet::new()), 0);
    }

    #[test]
    fn allowlist_suppresses_a_specific_api() {
        let source = "eval(user_input)\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        let allow = HashSet::from(["eval".to_string()]);
        assert_eq!(dangerous_api_reachable(&cpg, &allow), 0);
    }

    #[test]
    fn registry_key_requires_exact_or_dotted_suffix() {
        let keys = ["eval", "Function", "pickle.loads"];
        let m = |c: &str| match_registry_key(c, keys.iter().copied());
        for callee in [
            "page.waitForFunction",
            "isFunction",
            "retrieval",
            "evaluate",
        ] {
            assert_eq!(m(callee), None, "{callee}");
        }
        assert_eq!(m("eval"), Some("eval"));
        assert_eq!(m("x.eval"), Some("eval"));
        assert_eq!(m("Function"), Some("Function"));
        assert_eq!(m("pickle.loads"), Some("pickle.loads"));
        assert_eq!(m("mypkg.pickle.loads"), Some("pickle.loads"));
    }

    /// The `eval` line is a control: the lookalike calls are parsed and
    /// skipped, not missing from the graph.
    #[test]
    fn ignores_lookalike_method_names_in_javascript_and_typescript() {
        let source = "await page.waitForFunction(() => true);\n\
                      isFunction(x);\nretrieval(x);\neval(x);\n";
        for language in ["javascript", "typescript"] {
            let result = parse_source(source, language, None).unwrap();
            let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
            assert_eq!(
                dangerous_api_reachable(&cpg, &HashSet::new()),
                1,
                "{language}"
            );
        }
    }

    #[test]
    fn allow_patterns_drop_dotted_keys_but_not_lookalikes() {
        let registry = effective_registry("python", &HashSet::from(["loads".to_string()]));
        assert!(!registry.contains("pickle.loads"));
        assert!(!registry.contains("marshal.loads"));
        assert!(registry.contains("eval"));
        assert_eq!(match_registry_key("retrieval", ["eval"].into_iter()), None);
    }

    #[test]
    fn ignores_re_compile() {
        for source in ["re.compile(pattern)", "regex.compile(pattern)"] {
            let result = parse_source(source, "python", None).unwrap();
            let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
            assert_eq!(dangerous_api_reachable(&cpg, &HashSet::new()), 0);
        }
        let result = parse_source("compile(source, 'input', 'exec')", "python", None).unwrap();
        let cpg =
            CodePropertyGraph::from_uast(&result.uast_root, "compile(source, 'input', 'exec')");
        assert_eq!(dangerous_api_reachable(&cpg, &HashSet::new()), 1);
    }
}
