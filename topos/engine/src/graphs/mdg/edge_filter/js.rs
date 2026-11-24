//! JavaScript/TypeScript bare package imports matched to a same-named repo
//! file.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tree_sitter::{Language, Node};

use super::jsonc::parse_jsonc;
use super::paths::{ancestors, extension, file_name, join, parent};
use super::syntax::{named_nodes, parse_tree};
use super::Repo;

const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const CONFIGS: &[&str] = &["tsconfig.json", "jsconfig.json"];
const MAX_EXTENDS_DEPTH: usize = 8;

pub(super) fn is_js_extension(extension: &str) -> bool {
    EXTENSIONS.contains(&extension)
}

/// Module specifiers a config resolves inside the repository.
#[derive(Default, Clone)]
struct AliasConfig {
    patterns: Vec<String>,
    base_dirs: Vec<String>,
}

pub(super) struct JsRule<'g> {
    workspace_packages: Vec<String>,
    path_entries: HashSet<String>,
    specifiers: HashMap<&'g str, Option<Vec<String>>>,
    configs: HashMap<String, Option<AliasConfig>>,
}

impl<'g> JsRule<'g> {
    pub(super) fn new(repo: &Repo<'g>) -> Self {
        let workspace_packages = repo
            .paths
            .iter()
            .filter(|path| file_name(path) == "package.json")
            .filter_map(|path| package_name(repo.content.get(path)?))
            .collect();
        JsRule {
            workspace_packages,
            path_entries: path_entries(repo),
            specifiers: HashMap::new(),
            configs: HashMap::new(),
        }
    }

    /// Whether `source` names `target` only through bare package specifiers.
    pub(super) fn judge(&mut self, repo: &Repo<'g>, source: &'g str, target: &str) -> bool {
        let names = module_names(target);
        let Some(text) = repo.content.get(source) else {
            return false;
        };
        let Some(config) = self.config_for(repo, source) else {
            return false;
        };
        if !quoted_literals_ending_in(text, &names)
            .any(|literal| self.is_bare(literal, &config, target))
        {
            return false;
        }
        let specifiers = self
            .specifiers
            .entry(source)
            .or_insert_with(|| parse_specifiers(text, extension(source)));
        let Some(specifiers) = specifiers.clone() else {
            return false;
        };
        let mut naming = specifiers
            .iter()
            .filter(|specifier| names.contains(&specifier_name(specifier)))
            .peekable();
        naming.peek().is_some() && naming.all(|specifier| self.is_bare(specifier, &config, target))
    }

    fn is_bare(&self, specifier: &str, config: &AliasConfig, target: &str) -> bool {
        !(is_local(specifier)
            || spells_out_target(specifier, target)
            || config.patterns.iter().any(|p| alias_matches(p, specifier))
            || self.is_workspace_package(specifier)
            || self.under_base_url(specifier, config))
    }

    fn is_workspace_package(&self, specifier: &str) -> bool {
        self.workspace_packages.iter().any(|name| {
            specifier
                .strip_prefix(name.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
    }

    fn under_base_url(&self, specifier: &str, config: &AliasConfig) -> bool {
        let first = specifier.split('/').next().unwrap_or(specifier);
        config
            .base_dirs
            .iter()
            .any(|base| self.path_entries.contains(&join(base, first)))
    }

    /// The aliases in force for `source`, or `None` when its nearest config
    /// cannot be read in full.
    fn config_for(&mut self, repo: &Repo<'g>, source: &str) -> Option<AliasConfig> {
        let nearest = ancestors(parent(source))
            .flat_map(|dir| CONFIGS.iter().map(move |name| join(dir, name)))
            .find(|candidate| repo.has(candidate));
        match nearest {
            Some(path) => self.load_config(repo, &path, 0),
            None => Some(AliasConfig::default()),
        }
    }

    fn load_config(&mut self, repo: &Repo<'g>, path: &str, depth: usize) -> Option<AliasConfig> {
        if let Some(cached) = self.configs.get(path) {
            return cached.clone();
        }
        let loaded = if depth > MAX_EXTENDS_DEPTH {
            None
        } else {
            self.read_config(repo, path, depth)
        };
        self.configs.insert(path.to_string(), loaded.clone());
        loaded
    }

    fn read_config(&mut self, repo: &Repo<'g>, path: &str, depth: usize) -> Option<AliasConfig> {
        let json = parse_jsonc(repo.content.get(path)?)?;
        let dir = parent(path);
        let mut config = own_aliases(&json, dir);
        for extended in extends(&json) {
            if !extended.starts_with('.') {
                return None;
            }
            let mut target = join(dir, extended);
            if !target.ends_with(".json") {
                target.push_str(".json");
            }
            let inherited = self.load_config(repo, &target, depth + 1)?;
            config.patterns.extend(inherited.patterns);
            config.base_dirs.extend(inherited.base_dirs);
        }
        Some(config)
    }
}

fn package_name(text: &str) -> Option<String> {
    let json: Value = serde_json::from_str(text).ok()?;
    Some(json.get("name")?.as_str()?.to_string())
}

/// Every repo path without its extension, and every directory.
fn path_entries(repo: &Repo<'_>) -> HashSet<String> {
    let mut entries = HashSet::new();
    for path in &repo.paths {
        entries.insert(strip_extension(path).to_string());
        entries.extend(
            ancestors(parent(path))
                .filter(|dir| !dir.is_empty())
                .map(str::to_string),
        );
    }
    entries
}

fn own_aliases(json: &Value, dir: &str) -> AliasConfig {
    let options = json.get("compilerOptions");
    let patterns = options
        .and_then(|o| o.get("paths"))
        .and_then(Value::as_object)
        .map(|paths| paths.keys().cloned().collect())
        .unwrap_or_default();
    let base_dirs = options
        .and_then(|o| o.get("baseUrl"))
        .and_then(Value::as_str)
        .map(|base| vec![join(dir, base)])
        .unwrap_or_default();
    AliasConfig {
        patterns,
        base_dirs,
    }
}

fn extends(json: &Value) -> Vec<&str> {
    match json.get("extends") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn parse_specifiers(text: &str, extension: &str) -> Option<Vec<String>> {
    let language: Language = match extension {
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "ts" | "mts" | "cts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        _ => tree_sitter_javascript::LANGUAGE.into(),
    };
    let tree = parse_tree(text, language)?;
    let source = text.as_bytes();
    Some(
        named_nodes(tree.root_node())
            .into_iter()
            .filter_map(|node| specifier(node, source))
            .collect(),
    )
}

/// The module specifier of an `import`/`export ... from`, `require(...)`, or
/// `import(...)` node.
fn specifier(node: Node<'_>, source: &[u8]) -> Option<String> {
    if let Some(literal) = node
        .child_by_field_name("source")
        .filter(|s| s.kind() == "string")
    {
        return Some(unquote(literal, source));
    }
    if node.kind() != "call_expression" {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    let is_loader = function.kind() == "import"
        || (function.kind() == "identifier" && function.utf8_text(source) == Ok("require"));
    let first = node.child_by_field_name("arguments")?.named_child(0)?;
    (is_loader && first.kind() == "string").then(|| unquote(first, source))
}

fn unquote(node: Node<'_>, source: &[u8]) -> String {
    node.utf8_text(source)
        .unwrap_or("")
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_string()
}

fn strip_extension(path: &str) -> &str {
    let name_start = path.len() - file_name(path).len();
    EXTENSIONS
        .iter()
        .chain(&["json"])
        .filter_map(|ext| path.strip_suffix(ext)?.strip_suffix('.'))
        .find(|stem| stem.len() > name_start)
        .map_or(path, |stem| stem.strip_suffix(".d").unwrap_or(stem))
}

fn module_names(path: &str) -> Vec<&str> {
    let stem = file_name(strip_extension(path));
    let mut names = vec![stem];
    if stem == "index" {
        names.push(file_name(parent(path)));
    }
    names
}

/// Quoted strings in `text` whose last path segment is one of `names`, found
/// without parsing. Parsing is skipped unless one of them is bare, since only
/// a bare specifier can make the edge droppable.
fn quoted_literals_ending_in<'t>(
    text: &'t str,
    names: &'t [&'t str],
) -> impl Iterator<Item = &'t str> {
    names.iter().flat_map(move |name| {
        text.match_indices(name)
            .filter_map(move |(at, _)| literal_ending_at(text, at, name))
    })
}

fn literal_ending_at<'t>(text: &'t str, at: usize, name: &str) -> Option<&'t str> {
    let after = &text[at + name.len()..];
    let after = EXTENSIONS
        .iter()
        .find_map(|ext| after.strip_prefix('.')?.strip_prefix(ext))
        .unwrap_or(after);
    let quote = after
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let before = &text[..at];
    if !(before.ends_with('/') || before.ends_with(quote)) {
        return None;
    }
    let open = before.rfind(quote)? + 1;
    let close = text.len() - after.len();
    let literal = &text[open..close];
    (!literal.contains('\n')).then_some(literal)
}

fn specifier_name(specifier: &str) -> &str {
    file_name(strip_extension(specifier))
}

/// Relative and absolute paths, Node subpath imports (`#x`), and the `~/`
/// and `@/` alias conventions never name an npm package.
fn is_local(specifier: &str) -> bool {
    specifier.starts_with(['.', '/', '#', '~']) || specifier.starts_with("@/")
}

/// Whether the specifier, past its first segment, names two or more
/// trailing path segments of the target, as a bundler alias like
/// `@site/src/initialData` does. A package import that GitNexus matched by
/// file name alone does not.
fn spells_out_target(specifier: &str, target: &str) -> bool {
    let Some((_, rest)) = specifier.split_once('/') else {
        return false;
    };
    if !rest.contains('/') {
        return false;
    }
    let rest = strip_extension(rest);
    let target = strip_extension(target);
    [target, target.strip_suffix("/index").unwrap_or(target)]
        .iter()
        .any(|path| path.ends_with(&format!("/{rest}")) || *path == rest)
}

fn alias_matches(pattern: &str, specifier: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            specifier.len() >= prefix.len() + suffix.len()
                && specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
        }
        None => pattern == specifier,
    }
}
