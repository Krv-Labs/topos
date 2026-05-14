from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.utils.tree_sitter import parse_python


def test_program_object_basic():
    source = "def hello():\n    print('world')"
    root = parse_python(source)
    obj = ProgramObject(root=root, source=source)

    assert obj.source == source
    assert obj.language == "python"
    assert obj.is_valid is True
    assert obj.node_count > 0
    assert obj.depth > 0


def test_program_object_traversal():
    source = "x = 1 + 2"
    root = parse_python(source)
    obj = ProgramObject(root=root, source=source)

    nodes = list(obj.traverse())
    assert len(nodes) == obj.node_count

    # Check for specific node types
    assignments = list(obj.nodes_of_type("assignment"))
    assert len(assignments) == 1


def test_program_morphism_basic():
    source = "def add(a, b): return a + b"
    morphism = ProgramMorphism(source=source)

    assert morphism.source == source
    assert morphism.is_valid is True
    assert morphism.ast is not None
    assert "add" in morphism.name or "<morphism:" in morphism.name


def test_program_morphism_invalid_syntax():
    source = "def incomplete_func("
    morphism = ProgramMorphism(source=source)

    # tree-sitter might still "parse" it but with ERROR nodes
    assert morphism.ast is not None
    # ProgramObject.is_valid checks root.has_error
    assert morphism.is_valid is False


def test_program_morphism_equality():
    source1 = "x = 1"
    source2 = "x = 1"
    source3 = "y = 2"

    m1 = ProgramMorphism(source=source1)
    m2 = ProgramMorphism(source=source2)
    m3 = ProgramMorphism(source=source3)

    assert m1 == m2
    assert m1 != m3
    assert hash(m1) == hash(m2)


def test_program_morphism_from_file_and_classify(tmp_path):
    p = tmp_path / "hello.py"
    p.write_text("print('hello world')", encoding="utf-8")
    morphism = ProgramMorphism.from_file(p)
    assert morphism.filepath == p
    assert morphism.name == "hello.py"

    from topos.logic.lattice import EvaluationValue

    eval_val = morphism.classify()
    assert isinstance(eval_val, EvaluationValue)


def test_program_morphism_eq_not_implemented():
    morphism = ProgramMorphism(source="x = 1")
    assert morphism != "not a morphism"


def test_program_object_eq_not_implemented():
    root = parse_python("x = 1")
    obj = ProgramObject(root=root, source="x = 1")
    assert obj != "not a program object"
