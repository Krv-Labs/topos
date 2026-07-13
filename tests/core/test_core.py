from topos.core.morphism import ProgramMorphism


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
    assert morphism.filepath == str(p)
    assert morphism.name == "hello.py"

    from topos.core.omega import EvaluationValue

    eval_val = morphism.classify()
    assert isinstance(eval_val, EvaluationValue)


def test_program_morphism_eq_not_implemented():
    morphism = ProgramMorphism(source="x = 1")
    assert morphism != "not a morphism"
