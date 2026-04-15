import pytest

from topos.core.morphism import ProgramMorphism
from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.omega import SubobjectClassifier
from topos.logic.policies import section


def test_evaluation_value_order():
    # Basic ordering checks using DEFAULT_COVER (non-total)
    lattice = EvaluationLattice()

    assert lattice.leq(EvaluationValue.BROKEN, EvaluationValue.SOUND)
    assert lattice.leq(EvaluationValue.STABLE, EvaluationValue.SOUND)
    assert lattice.leq(EvaluationValue.COUPLED, EvaluationValue.STABLE)

    # COUPLED and COMPLEX are incomparable
    assert not lattice.leq(EvaluationValue.COUPLED, EvaluationValue.COMPLEX)
    assert not lattice.leq(EvaluationValue.COMPLEX, EvaluationValue.COUPLED)


def test_lattice_meet_join():
    lattice = EvaluationLattice()

    # Meet (And / GLB)
    assert (
        lattice.meet(EvaluationValue.SOUND, EvaluationValue.STABLE)
        == EvaluationValue.STABLE
    )
    assert (
        lattice.meet(EvaluationValue.COUPLED, EvaluationValue.COMPLEX)
        == EvaluationValue.BROKEN
    )

    # Join (Or / LUB)
    assert (
        lattice.join(EvaluationValue.COUPLED, EvaluationValue.COMPLEX)
        == EvaluationValue.STABLE
    )
    assert (
        lattice.join(EvaluationValue.BROKEN, EvaluationValue.SOUND)
        == EvaluationValue.SOUND
    )


def test_subobject_classifier_simple():
    classifier = SubobjectClassifier()

    source = """
def process_data(data):
    \"\"\"Process the input data list.\"\"\"
    result = []
    for item in data:
        if item is not None:
            result.append(item * 2)
    return result
"""
    morphism = ProgramMorphism(source=source)

    result = classifier.classify_detailed(morphism)
    assert result.is_parseable is True
    # Should have a structural dimension
    assert "structural" in result.dimensions
    # Clean code should be STABLE or better
    assert result.dimensions["structural"] >= EvaluationValue.STABLE


def test_subobject_classifier_invalid():
    classifier = SubobjectClassifier()

    source = "def broken(:"
    morphism = ProgramMorphism(source=source)

    evaluation = classifier.classify(morphism)
    assert evaluation == EvaluationValue.BROKEN


def test_classifier_aggregation():
    classifier = SubobjectClassifier()

    # meet(SOUND, COUPLED) = COUPLED (since COUPLED <= STABLE <= SOUND)
    val1 = EvaluationValue.SOUND
    val2 = EvaluationValue.COUPLED

    combined = classifier.combine(val1, val2)
    assert combined == EvaluationValue.COUPLED


def test_evaluation_value_properties():
    val = EvaluationValue.SOUND
    assert val.symbol == "⊤"
    assert "clean" in val.description.lower() or "maintainable" in val.description.lower()
    assert "SOUND" in str(val)


def test_lattice_implies_and_negation():
    lattice = EvaluationLattice()

    # negation(BROKEN) = implies(BROKEN, BROKEN) = SOUND (anything is ≥ BROKEN meet x ≤ BROKEN)
    assert lattice.negation(EvaluationValue.BROKEN) == EvaluationValue.SOUND

    # equivalent
    assert lattice.equivalent(EvaluationValue.STABLE, EvaluationValue.STABLE)
    assert not lattice.equivalent(EvaluationValue.STABLE, EvaluationValue.SOUND)


def test_subobject_classifier_str():
    classifier = SubobjectClassifier()
    morphism = ProgramMorphism(source="x = 1")
    res = classifier.classify_detailed(morphism)
    # Per-dimension format
    s = str(res)
    assert "structural" in s


def test_policies_classify_error():
    with pytest.raises(ValueError):
        section._classify(-100.0, section.complexity_bins)
