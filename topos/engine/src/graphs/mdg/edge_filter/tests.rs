use super::*;
use crate::graphs::mdg::models::{GraphNode, GraphRelationship};
use serde_json::Value;

struct Fixture {
    graph: ModuleDependencyGraph,
    next: usize,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            graph: ModuleDependencyGraph::new(""),
            next: 0,
        }
    }

    fn file(&mut self, path: &str, content: &str) -> String {
        let id = format!("File:{path}");
        let mut properties = HashMap::new();
        properties.insert("filePath".to_string(), Value::String(path.to_string()));
        properties.insert("content".to_string(), Value::String(content.to_string()));
        self.graph.add_node(GraphNode {
            id: id.clone(),
            label: "File".to_string(),
            properties,
        });
        id
    }

    fn function(&mut self, file_id: &str, name: &str, exported: bool) -> String {
        let id = format!("Function:{file_id}:{name}");
        let mut properties = HashMap::new();
        properties.insert("name".to_string(), Value::String(name.to_string()));
        properties.insert("isExported".to_string(), Value::Bool(exported));
        self.graph.add_node(GraphNode {
            id: id.clone(),
            label: "Function".to_string(),
            properties,
        });
        self.edge(file_id, &id, "DEFINES", "");
        id
    }

    fn edge(&mut self, source: &str, target: &str, rel_type: &str, reason: &str) -> String {
        self.next += 1;
        let id = format!("r{}", self.next);
        self.graph.add_relationship(GraphRelationship {
            id: id.clone(),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: rel_type.to_string(),
            confidence: 0.85,
            reason: reason.to_string(),
            properties: HashMap::new(),
        });
        id
    }

    fn import(&mut self, source: &str, target: &str) -> String {
        self.edge(source, target, "IMPORTS", "python-scope: import")
    }

    fn dropped(&self) -> Vec<(String, DropReason)> {
        find_false_edges(&self.graph)
            .into_iter()
            .map(|edge| (edge.relationship_id, edge.reason))
            .collect()
    }
}

fn two_crates() -> (Fixture, String, String) {
    let mut fx = Fixture::new();
    fx.file("Cargo.toml", "[workspace]");
    fx.file("app/Cargo.toml", "[package]");
    fx.file("lib/Cargo.toml", "[package]");
    let app = fx.file("app/src/main.rs", "fn main() { std::fs::write(p, b) }");
    let lib = fx.file("lib/src/io.rs", "fn write() {}");
    (fx, app, lib)
}

#[test]
fn rust_call_to_a_private_item_in_another_crate_is_dropped() {
    let (mut fx, app, lib) = two_crates();
    let caller = fx.function(&app, "main", false);
    let callee = fx.function(&lib, "write", false);
    let call = fx.edge(&caller, &callee, "CALLS", "import-resolved");
    assert_eq!(
        fx.dropped(),
        vec![(call, DropReason::RustPrivateCrossCrate)]
    );
}

#[test]
fn rust_call_to_an_exported_item_in_another_crate_is_kept() {
    let (mut fx, app, lib) = two_crates();
    let caller = fx.function(&app, "main", false);
    let callee = fx.function(&lib, "write", true);
    fx.edge(&caller, &callee, "CALLS", "import-resolved");
    assert!(fx.dropped().is_empty());
}

#[test]
fn rust_call_to_a_method_in_another_crate_is_kept() {
    let (mut fx, app, lib) = two_crates();
    let caller = fx.function(&app, "main", false);
    let node = fx.function(&lib, "Node", true);
    let clone = fx.function(&lib, "Node.clone", false);
    fx.edge(&node, &clone, "HAS_METHOD", "");
    fx.edge(&caller, &clone, "CALLS", "import-resolved");
    assert!(fx.dropped().is_empty());
}

#[test]
fn rust_private_call_inside_one_crate_is_kept() {
    let mut fx = Fixture::new();
    fx.file("lib/Cargo.toml", "[package]");
    let parent = fx.file("lib/src/store.rs", "mod child; fn helper() {}");
    let child = fx.file("lib/src/store/child.rs", "fn f() { super::helper() }");
    let other = fx.file("lib/src/other.rs", "fn g() {}");
    let helper = fx.function(&parent, "helper", false);
    let f = fx.function(&child, "f", false);
    let g = fx.function(&other, "g", false);
    fx.edge(&f, &helper, "CALLS", "import-resolved");
    fx.edge(&g, &helper, "CALLS", "import-resolved");
    assert!(fx.dropped().is_empty());
}

#[test]
fn rust_call_without_any_manifest_in_the_graph_is_kept() {
    let mut fx = Fixture::new();
    let a = fx.file("a/src/lib.rs", "");
    let b = fx.file("b/src/lib.rs", "");
    let caller = fx.function(&a, "f", false);
    let callee = fx.function(&b, "g", false);
    fx.edge(&caller, &callee, "CALLS", "import-resolved");
    assert!(fx.dropped().is_empty());
}

fn python_package() -> Fixture {
    let mut fx = Fixture::new();
    fx.file("pkg/__init__.py", "");
    fx.file("pkg/http/__init__.py", "");
    fx.file("pkg/utils/__init__.py", "");
    fx
}

#[test]
fn python_stdlib_import_matched_to_a_nested_repo_module_is_dropped() {
    let mut fx = python_package();
    let json = fx.file("pkg/http/json.py", "");
    let asyncio = fx.file("pkg/utils/asyncio.py", "");
    let pprint = fx.file("pkg/utils/pprint.py", "");
    let source = fx.file(
        "pkg/spider.py",
        "import json\nimport asyncio.subprocess\nfrom pprint import pformat\n",
    );
    let a = fx.import(&source, &json);
    let b = fx.import(&source, &asyncio);
    let c = fx.import(&source, &pprint);
    let mut dropped = fx.dropped();
    dropped.sort();
    assert_eq!(
        dropped,
        vec![
            (a, DropReason::PythonStdlibImport),
            (b, DropReason::PythonStdlibImport),
            (c, DropReason::PythonStdlibImport),
        ]
    );
}

#[test]
fn python_relative_import_of_the_same_name_is_kept() {
    let mut fx = python_package();
    let json = fx.file("pkg/http/json.py", "");
    let source = fx.file("pkg/http/response.py", "import json\nfrom . import json\n");
    fx.import(&source, &json);
    let other = fx.file("pkg/http/text.py", "import json\nfrom .json import X\n");
    fx.import(&other, &json);
    assert!(fx.dropped().is_empty());
}

#[test]
fn python_absolute_repo_import_of_the_same_name_is_kept() {
    let mut fx = python_package();
    let json = fx.file("pkg/http/json.py", "");
    let source = fx.file(
        "pkg/spider.py",
        "import json\nfrom pkg.http.json import JsonResponse\n",
    );
    fx.import(&source, &json);
    assert!(fx.dropped().is_empty());
}

#[test]
fn python_repo_top_level_package_named_like_the_stdlib_is_kept() {
    let mut fx = python_package();
    let top = fx.file("signal/__init__.py", "");
    let nested = fx.file("pkg/utils/signal.py", "");
    let json = fx.file("pkg/http/json.py", "");
    let source = fx.file("pkg/spider.py", "import signal\nimport json\n");
    fx.import(&source, &top);
    fx.import(&source, &nested);
    let control = fx.import(&source, &json);
    assert_eq!(
        fx.dropped(),
        vec![(control, DropReason::PythonStdlibImport)]
    );
}

#[test]
fn python_import_from_a_file_without_content_is_kept() {
    let mut fx = python_package();
    let json = fx.file("pkg/http/json.py", "");
    let source = fx.file("pkg/spider.py", "");
    fx.import(&source, &json);
    assert!(fx.dropped().is_empty());
}

#[test]
fn markdown_links_are_never_judged() {
    let mut fx = python_package();
    let json = fx.file("pkg/http/json.py", "");
    let source = fx.file("pkg/spider.py", "import json\n");
    fx.edge(&source, &json, "IMPORTS", "markdown-link");
    assert!(fx.dropped().is_empty());
}

#[test]
fn ts_bare_package_specifier_matched_to_a_repo_file_is_dropped() {
    let mut fx = Fixture::new();
    let state = fx.file("packages/pointer/src/state.ts", "");
    let math = fx.file("packages/pointer/src/math.ts", "");
    let source = fx.file(
        "packages/app/src/editor.tsx",
        "import { EditorState } from \"@codemirror/state\";\n\
         import { line } from 'roughjs/bin/math';\n\
         export const A = () => <div />;\n",
    );
    let a = fx.import(&source, &state);
    let b = fx.import(&source, &math);
    let mut dropped = fx.dropped();
    dropped.sort();
    assert_eq!(
        dropped,
        vec![
            (a, DropReason::JsBarePackageImport),
            (b, DropReason::JsBarePackageImport),
        ]
    );
}

#[test]
fn ts_relative_import_of_the_same_name_is_kept() {
    let mut fx = Fixture::new();
    let state = fx.file("src/state.ts", "");
    let source = fx.file(
        "src/editor.ts",
        "import { EditorState } from \"@codemirror/state\";\nimport { s } from \"./state\";\n",
    );
    fx.import(&source, &state);
    let index = fx.file("src/math/index.ts", "");
    let other = fx.file(
        "src/view.js",
        "const m = require('roughjs/bin/math');\nconst n = require('./math');\n",
    );
    fx.import(&other, &index);
    assert!(fx.dropped().is_empty());
}

#[test]
fn ts_aliased_import_is_kept() {
    let mut fx = Fixture::new();
    fx.file(
        "tsconfig.json",
        "{\n  // comment\n  \"compilerOptions\": {\n    \"paths\": {\n      \
         \"@app/*\": [\"./src/*\"], /* trailing */\n    },\n  },\n}\n",
    );
    let state = fx.file("src/state.ts", "");
    let view = fx.file("src/view.ts", "");
    let source = fx.file(
        "src/editor.ts",
        "import { s } from \"@app/state\";\nimport { v } from \"@codemirror/view\";\n",
    );
    fx.import(&source, &state);
    let control = fx.import(&source, &view);
    assert_eq!(
        fx.dropped(),
        vec![(control, DropReason::JsBarePackageImport)]
    );
}

#[test]
fn ts_alias_inherited_through_extends_is_kept() {
    let mut fx = Fixture::new();
    fx.file(
        "packages/tsconfig.base.json",
        "{\"compilerOptions\": {\"paths\": {\"@ex/element/*\": [\"./element/src/*\"]}}}",
    );
    fx.file(
        "packages/app/tsconfig.json",
        "{\"extends\": \"../tsconfig.base.json\"}",
    );
    let types = fx.file("packages/app/types.ts", "");
    let source = fx.file(
        "packages/app/editor.ts",
        "import type { T } from \"@ex/element/types\";\nimport { m } from \"roughjs/bin/math\";\n",
    );
    let math = fx.file("packages/app/math.ts", "");
    fx.import(&source, &types);
    let control = fx.import(&source, &math);
    assert_eq!(
        fx.dropped(),
        vec![(control, DropReason::JsBarePackageImport)]
    );
}

#[test]
fn ts_base_url_entry_is_kept() {
    let mut fx = Fixture::new();
    fx.file(
        "tsconfig.json",
        "{\"compilerOptions\": {\"baseUrl\": \"src\"}}",
    );
    let state = fx.file("src/store/state.ts", "");
    let math = fx.file("src/geometry/math.ts", "");
    let source = fx.file(
        "src/editor.ts",
        "import { s } from \"store/state\";\nimport { m } from \"roughjs/bin/math\";\n",
    );
    fx.import(&source, &state);
    let control = fx.import(&source, &math);
    assert_eq!(
        fx.dropped(),
        vec![(control, DropReason::JsBarePackageImport)]
    );
}

#[test]
fn ts_workspace_package_import_is_kept() {
    let mut fx = Fixture::new();
    fx.file("packages/math/package.json", "{\"name\": \"@ex/math\"}");
    let point = fx.file("packages/math/src/point.ts", "");
    let state = fx.file("packages/math/src/state.ts", "");
    let source = fx.file(
        "packages/app/editor.ts",
        "import { p } from \"@ex/math/point\";\nimport { s } from \"@codemirror/state\";\n",
    );
    fx.import(&source, &point);
    let control = fx.import(&source, &state);
    assert_eq!(
        fx.dropped(),
        vec![(control, DropReason::JsBarePackageImport)]
    );
}

#[test]
fn ts_unreadable_config_keeps_every_edge() {
    let mut fx = Fixture::new();
    fx.file("tsconfig.json", "{\"extends\": \"./missing.json\"}");
    let state = fx.file("src/state.ts", "");
    let source = fx.file(
        "src/editor.ts",
        "import { s } from \"@codemirror/state\";\n",
    );
    fx.import(&source, &state);
    assert!(fx.dropped().is_empty());
}

#[test]
fn ts_config_extending_a_package_config_keeps_every_edge() {
    let mut fx = Fixture::new();
    fx.file(
        "docs/tsconfig.json",
        "{\"extends\": \"@tsconfig/docusaurus/tsconfig.json\"}",
    );
    let homepage = fx.file("docs/src/components/Homepage/index.tsx", "");
    let source = fx.file(
        "docs/src/pages/index.tsx",
        "import Homepage from \"@site/src/components/Homepage\";\n",
    );
    fx.import(&source, &homepage);
    assert!(fx.dropped().is_empty());
}

#[test]
fn ts_specifier_spelling_out_the_target_path_is_kept() {
    let mut fx = Fixture::new();
    let data = fx.file("docs/src/initialData.js", "");
    let button = fx.file("web/src/components/Button.tsx", "");
    let utils = fx.file("web/src/utils.ts", "");
    let internal = fx.file("web/src/internal/state.ts", "");
    let source = fx.file(
        "web/src/app.tsx",
        "import data from \"@site/src/initialData\";\n\
         import { Button } from \"@/components/Button\";\n\
         import { u } from \"~/utils\";\n\
         import { s } from \"#internal/state\";\n",
    );
    fx.import(&source, &data);
    fx.import(&source, &button);
    fx.import(&source, &utils);
    fx.import(&source, &internal);
    assert!(fx.dropped().is_empty());
}

#[test]
fn jsonc_comments_and_trailing_commas_are_stripped() {
    let parsed = super::jsonc::parse_jsonc(
        "{\n  // line\n  \"a\": \"x // not a comment\", /* block */\n  \"b\": [1, 2,],\n}",
    )
    .unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({"a": "x // not a comment", "b": [1, 2]})
    );
}

#[test]
fn stdlib_list_is_sorted_for_binary_search() {
    let list = super::python_stdlib::PYTHON_STDLIB_MODULES;
    assert!(list.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(super::python_stdlib::is_stdlib_module("json"));
    assert!(!super::python_stdlib::is_stdlib_module("scrapy"));
}
