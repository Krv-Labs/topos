//! Dangerous-API reachability probe (CPG → ℝ).
//!
//! Counts call and `new` sites whose callee matches the per-language
//! registry of dangerous APIs. The callee is the call's first syntax
//! child, unwrapped through parentheses, casts and generics, and its
//! source text is normalized to a dotted name before matching.

use std::collections::HashSet;

use crate::graphs::cpg::models::CPGNode;
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
            "document.write",
            "child_process.exec",
        ],
        "typescript" => &["eval", "Function", "document.write", "child_process.exec"],
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

/// Count call sites whose callee matches the dangerous-API registry for
/// `cpg.language`. Matches both bare names (`eval`) and dotted/qualified
/// names (`pickle.loads`, `std::mem::transmute`).
pub fn dangerous_api_reachable(cpg: &CodePropertyGraph, allow: &HashSet<String>) -> usize {
    let registry = effective_registry(&cpg.language, allow);
    if registry.is_empty() {
        return 0;
    }

    cpg.nodes
        .values()
        .filter_map(|node| call_callee(cpg, node))
        .filter(|callee| matches_registry(callee, registry.iter().copied()))
        .count()
}

/// The dotted callee of a call or `new` expression, read from the
/// callee's syntax node, or `None` when `node` is not a call or its
/// callee has no name (`wrap(eval)(x)`, `obj["eval"](x)`).
///
/// Parentheses, non-null assertions, casts and Rust turbofish around the
/// callee are seen through. Whitespace is dropped and `?.`, `::` and
/// `->` read as `.`. A name reached through a computed receiver
/// (`a.b().eval`) is prefixed with `?`, so it still ends in `.eval` but
/// never equals a bare key.
pub fn call_callee(cpg: &CodePropertyGraph, node: &CPGNode) -> Option<String> {
    if node.kind() != "CallExpr" && node.uast.native.node_kind != "new_expression" {
        return None;
    }
    let mut head = node.uast.children.first()?;
    loop {
        let full = cpg.nodes.get(&head.id).map_or(head, |found| &found.uast);
        let kind = full.native.node_kind.as_str();
        head = if kind == "type_assertion" {
            full.children.last()?
        } else if CALLEE_WRAPPERS.contains(&kind) {
            full.children.first()?
        } else {
            break;
        };
    }
    if ARGUMENT_KINDS.contains(&head.native.node_kind.as_str()) {
        return None;
    }
    let name = dotted_name(cpg.source.get(head.span.start_byte..head.span.end_byte)?);
    (!name.is_empty()).then_some(name)
}

const CALLEE_WRAPPERS: [&str; 5] = [
    "parenthesized_expression",
    "non_null_expression",
    "generic_function",
    "as_expression",
    "satisfies_expression",
];
const ARGUMENT_KINDS: [&str; 2] = ["arguments", "argument_list"];

fn dotted_name(text: &str) -> String {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let joined = compact
        .replace("?.", ".")
        .replace("::", ".")
        .replace("->", ".")
        .replace('!', "");
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '_' | '$' | '.');
    let start = joined
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(joined.len(), |(at, _)| at);
    let name = &joined[start..];
    if start > 0 && name.starts_with('.') {
        return format!("?{}", name.trim_end_matches('.'));
    }
    name.trim_matches('.').to_string()
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

    fn count(source: &str, language: &str) -> usize {
        let result = parse_source(source, language, None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        dangerous_api_reachable(&cpg, &HashSet::new())
    }

    #[test]
    fn javascript_and_typescript_spellings_of_a_dangerous_call_are_seen() {
        for spelling in [
            "eval(a);\n",
            "(eval)(x);\n",
            "eval?.(x);\n",
            "obj?.eval(x);\n",
            "x!.eval();\n",
            "window\n    .eval(c);\n",
            "window . eval(c);\n",
            "a.b().eval(j);\n",
            "eval<string>(b);\n",
            "new Function('h');\n",
            "(eval as any)(x);\n",
        ] {
            assert_eq!(count(spelling, "typescript"), 1, "typescript: {spelling}");
            if !spelling.contains(" as ") && !spelling.contains('!') && !spelling.contains('<') {
                assert_eq!(count(spelling, "javascript"), 1, "javascript: {spelling}");
            }
        }
    }

    #[test]
    fn python_spellings_of_a_dangerous_call_are_seen() {
        for spelling in [
            "os.system(c)\n",
            "os .system(c)\n",
            "(os.system)(c)\n",
            "(eval)(c)\n",
        ] {
            assert_eq!(count(spelling, "python"), 1, "{spelling}");
        }
    }

    #[test]
    fn rust_path_calls_are_seen() {
        for spelling in [
            "std::mem::transmute(p)",
            "transmute::<A, B>(x)",
            "mem::transmute::<u32, f32>(x)",
            "Box::from_raw(p)",
            "K :: transmute(p)",
        ] {
            let source = format!("fn f() {{\n    let _a = {spelling};\n}}\n");
            assert_eq!(count(&source, "rust"), 1, "{spelling}");
        }
    }

    #[test]
    fn mentions_wrappers_and_short_keys_on_computed_receivers_are_not_calls() {
        assert_eq!(count("wrap(eval)(x);\n", "typescript"), 0);
        assert_eq!(count("const s = \"eval(x)\";\n", "typescript"), 0);
        assert_eq!(count("s = 'eval(x)'\n", "python"), 0);
        assert_eq!(count("re.compile(p)\n(re.compile)(p)\n", "python"), 0);
        let allow = HashSet::new();
        let keys = ["run"];
        let source = "getJob().run();\njobs[0].run();\n";
        let result = parse_source(source, "typescript", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        let callees: Vec<String> = cpg
            .nodes
            .values()
            .filter_map(|node| call_callee(&cpg, node))
            .collect();
        assert!(!callees.is_empty());
        for callee in &callees {
            assert_eq!(
                match_registry_key(callee, keys.into_iter()),
                None,
                "{callee}"
            );
        }
        assert_eq!(dangerous_api_reachable(&cpg, &allow), 0);
    }

    #[test]
    fn a_computed_receiver_matches_like_a_dotted_name() {
        assert_eq!(count("model.eval();\n", "typescript"), 1);
        assert_eq!(count("getModel().eval();\n", "typescript"), 1);
    }
}
