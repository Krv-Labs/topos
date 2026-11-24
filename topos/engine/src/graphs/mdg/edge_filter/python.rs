//! Python standard-library imports matched to a same-named repo module.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::paths::{extension, file_name, join, parent};
use super::python_stdlib::is_stdlib_module;
use super::syntax::{named_nodes, parse_tree, text};
use super::Repo;

pub(super) struct PythonRule<'g> {
    top_level: HashSet<String>,
    imports: HashMap<&'g str, Option<PythonImports>>,
}

impl<'g> PythonRule<'g> {
    pub(super) fn new(repo: &Repo<'g>) -> Self {
        PythonRule {
            top_level: top_level_names(repo),
            imports: HashMap::new(),
        }
    }

    /// Whether `source` names `target` only through standard-library
    /// imports.
    pub(super) fn judge(&mut self, repo: &Repo<'g>, source: &'g str, target: &str) -> bool {
        if !is_python(target) {
            return false;
        }
        let top_level = &self.top_level;
        let imports = self.imports.entry(source).or_insert_with(|| {
            let text = repo.content.get(source)?;
            parse_imports(text, top_level)
        });
        let name = module_name(target);
        imports
            .as_ref()
            .is_some_and(|imports| imports.stdlib.contains(name) && !imports.other.contains(name))
    }
}

/// Every name segment a file's imports mention, split by whether the import
/// is an absolute standard-library one.
#[derive(Default)]
struct PythonImports {
    stdlib: HashSet<String>,
    other: HashSet<String>,
}

struct ImportStatement {
    module: Option<String>,
    relative: bool,
    names: Vec<String>,
}

impl PythonImports {
    fn add(&mut self, statement: ImportStatement, top_level: &HashSet<String>) {
        match &statement.module {
            Some(module) => self.record(module, &statement.names, statement.relative, top_level),
            None => {
                for name in &statement.names {
                    self.record(name, &[], false, top_level);
                }
            }
        }
    }

    fn record(
        &mut self,
        dotted: &str,
        names: &[String],
        relative: bool,
        top_level: &HashSet<String>,
    ) {
        let top = dotted.split('.').next().unwrap_or("");
        let stdlib = !relative && is_stdlib_module(top) && !top_level.contains(top);
        let set = if stdlib {
            &mut self.stdlib
        } else {
            &mut self.other
        };
        let segments = std::iter::once(dotted)
            .chain(names.iter().map(String::as_str))
            .flat_map(|name| name.split('.'))
            .filter(|segment| !segment.is_empty());
        set.extend(segments.map(str::to_string));
    }
}

fn is_python(path: &str) -> bool {
    matches!(extension(path), "py" | "pyi")
}

fn module_name(path: &str) -> &str {
    let name = file_name(path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    if stem == "__init__" {
        file_name(parent(path))
    } else {
        stem
    }
}

/// Names importable without a parent package: modules and packages whose
/// directory has no `__init__.py`. An absolute import of one of these may
/// mean the repository's own module, so it never counts as standard library.
fn top_level_names(repo: &Repo<'_>) -> HashSet<String> {
    let is_package =
        |dir: &str| repo.has(&join(dir, "__init__.py")) || repo.has(&join(dir, "__init__.pyi"));
    repo.paths
        .iter()
        .filter(|path| is_python(path))
        .filter_map(|path| {
            let home = if file_name(path).starts_with("__init__.") {
                let package = parent(path);
                (!package.is_empty()).then(|| parent(package))?
            } else {
                parent(path)
            };
            (!is_package(home)).then(|| module_name(path).to_string())
        })
        .collect()
}

fn parse_imports(text: &str, top_level: &HashSet<String>) -> Option<PythonImports> {
    let tree = parse_tree(text, tree_sitter_python::LANGUAGE.into())?;
    let source = text.as_bytes();
    let mut imports = PythonImports::default();
    for statement in named_nodes(tree.root_node())
        .into_iter()
        .filter_map(|node| import_statement(node, source))
    {
        imports.add(statement, top_level);
    }
    Some(imports)
}

fn import_statement(node: Node<'_>, source: &[u8]) -> Option<ImportStatement> {
    let (module, relative) = match node.kind() {
        "import_statement" => (None, false),
        "future_import_statement" => (Some("__future__".to_string()), false),
        "import_from_statement" => {
            let module = node.child_by_field_name("module_name")?;
            (
                Some(text(module, source).trim_start_matches('.').to_string()),
                module.kind() == "relative_import",
            )
        }
        _ => return None,
    };
    let mut cursor = node.walk();
    let names = node
        .children_by_field_name("name", &mut cursor)
        .filter_map(|name| imported_name(name, source))
        .collect();
    Some(ImportStatement {
        module,
        relative,
        names,
    })
}

fn imported_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "aliased_import" => node
            .child_by_field_name("name")
            .map(|name| text(name, source)),
        _ => Some(text(node, source)),
    }
}
