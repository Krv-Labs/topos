import ast as py_ast
import json
from dataclasses import asdict, is_dataclass
from pathlib import Path

from topos.graphs.ast.dispatch import parse_source
from topos.utils.tree_sitter import find_errors, node_to_sexp


def _to_jsonable(value):
    if is_dataclass(value):
        return asdict(value)
    if isinstance(value, dict):
        return {k: _to_jsonable(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_to_jsonable(v) for v in value]
    return value


def _serialize_native_ast(native_ast_obj, language: str) -> str:
    if native_ast_obj is None:
        return ""
    if language == "python":
        return py_ast.dump(native_ast_obj, indent=2)
    return repr(native_ast_obj)


def main():
    src_dir = Path("demos/binarytrees/src")
    ast_dir = Path("demos/binarytrees/asts")
    tree_dir = ast_dir / "treesitter"
    uast_dir = ast_dir / "uast"
    native_dir = ast_dir / "native"
    tree_dir.mkdir(parents=True, exist_ok=True)
    uast_dir.mkdir(parents=True, exist_ok=True)
    native_dir.mkdir(parents=True, exist_ok=True)

    extension_map = {
        ".py": "python",
        ".js": "javascript",
        ".rs": "rust",
        ".cpp": "cpp",
    }

    for src_file in src_dir.iterdir():
        if src_file.suffix not in extension_map:
            continue

        lang = extension_map[src_file.suffix]
        print(f"Parsing {src_file.name} ({lang})...")

        source = src_file.read_text()
        result = parse_source(
            source=source,
            language=lang,
            backend="hybrid",
            file=str(src_file),
        )
        root = result.root

        # Check for errors
        errors = find_errors(root)
        if errors:
            print(f"  WARNING: Found {len(errors)} error nodes in {src_file.name}")

        sexp = node_to_sexp(root)

        # Use a more specific name to avoid collisions if stems are identical
        safe_suffix = src_file.suffix.replace(".", "_")
        base_name = f"{src_file.stem}{safe_suffix}"

        tree_file = tree_dir / f"{base_name}.ast.txt"
        tree_file.write_text(sexp)
        print(f"  Saved tree-sitter AST to {tree_file}")

        uast_file = uast_dir / f"{base_name}.uast.json"
        uast_payload = _to_jsonable(result.uast_root)
        uast_file.write_text(json.dumps(uast_payload, indent=2))
        print(f"  Saved UAST to {uast_file}")

        native_file = native_dir / f"{base_name}.native.txt"
        native_file.write_text(_serialize_native_ast(result.native_ast, lang))
        print(f"  Saved native AST to {native_file}")


if __name__ == "__main__":
    main()
