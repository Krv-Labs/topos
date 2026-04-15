from topos.core.morphism import ProgramMorphism
from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.omega import SubobjectClassifier
from topos.logic.policies.base import Priority


def test_evaluation_value_order():
    # Diamond lattice: BROKEN ≤ {COMPOSABLE, SELF_CONTAINED} ≤ SOUND
    lattice = EvaluationLattice()

    assert lattice.leq(EvaluationValue.BROKEN, EvaluationValue.COMPOSABLE)
    assert lattice.leq(EvaluationValue.BROKEN, EvaluationValue.SELF_CONTAINED)
    assert lattice.leq(EvaluationValue.BROKEN, EvaluationValue.SOUND)
    assert lattice.leq(EvaluationValue.COMPOSABLE, EvaluationValue.SOUND)
    assert lattice.leq(EvaluationValue.SELF_CONTAINED, EvaluationValue.SOUND)

    # COMPOSABLE and SELF_CONTAINED are incomparable
    assert not lattice.leq(EvaluationValue.COMPOSABLE, EvaluationValue.SELF_CONTAINED)
    assert not lattice.leq(EvaluationValue.SELF_CONTAINED, EvaluationValue.COMPOSABLE)


def test_lattice_meet_join():
    lattice = EvaluationLattice()

    # Meet (GLB) — incomparable elements meet at BROKEN
    assert (
        lattice.meet(EvaluationValue.COMPOSABLE, EvaluationValue.SELF_CONTAINED)
        == EvaluationValue.BROKEN
    )
    assert (
        lattice.meet(EvaluationValue.SOUND, EvaluationValue.COMPOSABLE)
        == EvaluationValue.COMPOSABLE
    )
    assert (
        lattice.meet(EvaluationValue.SOUND, EvaluationValue.SELF_CONTAINED)
        == EvaluationValue.SELF_CONTAINED
    )
    assert (
        lattice.meet(EvaluationValue.BROKEN, EvaluationValue.COMPOSABLE)
        == EvaluationValue.BROKEN
    )

    # Join (LUB) — incomparable elements join at SOUND
    assert (
        lattice.join(EvaluationValue.COMPOSABLE, EvaluationValue.SELF_CONTAINED)
        == EvaluationValue.SOUND
    )
    assert (
        lattice.join(EvaluationValue.BROKEN, EvaluationValue.SOUND)
        == EvaluationValue.SOUND
    )
    assert (
        lattice.join(EvaluationValue.BROKEN, EvaluationValue.COMPOSABLE)
        == EvaluationValue.COMPOSABLE
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
    # Clean code should achieve SELF_CONTAINED or better
    assert result.dimensions["structural"] == EvaluationValue.SELF_CONTAINED
    # Score should be in [0, 1]
    assert 0.0 <= result.scores["structural"] <= 1.0


def test_subobject_classifier_invalid():
    classifier = SubobjectClassifier()

    source = "def broken(:"
    morphism = ProgramMorphism(source=source)

    evaluation = classifier.classify(morphism)
    assert evaluation == EvaluationValue.BROKEN


def test_classifier_aggregation():
    classifier = SubobjectClassifier()

    # meet(SOUND, COMPOSABLE) = COMPOSABLE
    combined = classifier.combine(EvaluationValue.SOUND, EvaluationValue.COMPOSABLE)
    assert combined == EvaluationValue.COMPOSABLE

    # meet(COMPOSABLE, SELF_CONTAINED) = BROKEN (incomparable)
    combined2 = classifier.combine(
        EvaluationValue.COMPOSABLE, EvaluationValue.SELF_CONTAINED
    )
    assert combined2 == EvaluationValue.BROKEN


def test_evaluation_value_properties():
    val = EvaluationValue.SOUND
    assert val.symbol == "⊤"
    assert "clean" in val.description.lower() or "composable" in val.description.lower()
    assert "SOUND" in str(val)

    broken = EvaluationValue.BROKEN
    assert broken.symbol == "⊥"

    composable = EvaluationValue.COMPOSABLE
    assert composable.symbol == "◑"
    assert (
        "composable" in composable.description.lower()
        or "coupling" in composable.description.lower()
    )

    sc = EvaluationValue.SELF_CONTAINED
    assert sc.symbol == "◐"
    assert (
        "structural" in sc.description.lower()
        or "self" in sc.description.lower()
        or "stand" in sc.description.lower()
    )


def test_lattice_implies_and_negation():
    lattice = EvaluationLattice()

    # negation(BROKEN) = SOUND (bottom → anything = top in Heyting)
    assert lattice.negation(EvaluationValue.BROKEN) == EvaluationValue.SOUND

    # negation(SOUND) = BROKEN (top → bottom = bottom)
    assert lattice.negation(EvaluationValue.SOUND) == EvaluationValue.BROKEN

    # equivalent(X, X) = True for all X
    for val in EvaluationValue:
        assert lattice.equivalent(val, val)


def test_subobject_classifier_str():
    classifier = SubobjectClassifier()
    morphism = ProgramMorphism(source="x = 1")
    res = classifier.classify_detailed(morphism)
    s = str(res)
    assert "structural" in s


def test_priority_weight_profiles():
    from topos.logic.policies.base import WEIGHT_PROFILES, Priority

    for priority in Priority:
        profile = WEIGHT_PROFILES[priority]
        assert 0.0 <= profile.w_complexity <= 1.0
        assert 0.0 <= profile.w_coupling <= 1.0


def test_score_structural_perfect_code():
    from topos.logic.policies.structural import score_structural

    # Ideal: complexity=0, entropy=0.5
    result = score_structural(0, 0.5, Priority.BALANCED)
    assert result.score == 1.0
    assert result.achieved is True


def test_score_structural_pathological_code():
    from topos.logic.policies.structural import score_structural

    # Worst case: complexity=40, entropy=1.0
    result = score_structural(40, 1.0, Priority.BALANCED)
    assert result.score < 0.6
    assert result.achieved is False


def test_score_structural_priority_shifts():
    from topos.logic.policies.structural import score_structural

    # Code with bad entropy but reasonable complexity
    balanced = score_structural(5, 0.95, Priority.BALANCED)
    self_contained = score_structural(5, 0.95, Priority.SELF_CONTAINED)
    # SELF_CONTAINED upweights complexity (which is good here), so score >= balanced
    assert self_contained.score >= balanced.score


def test_classify_detailed_priority_parameter():
    classifier = SubobjectClassifier()
    morphism = ProgramMorphism(source="x = 1 + 2")

    result_balanced = classifier.classify_detailed(morphism, priority=Priority.BALANCED)
    result_sc = classifier.classify_detailed(morphism, priority=Priority.SELF_CONTAINED)

    assert result_balanced.priority == Priority.BALANCED
    assert result_sc.priority == Priority.SELF_CONTAINED
    # Both should be parseable
    assert result_balanced.is_parseable
    assert result_sc.is_parseable


def test_combine_dimensions_uses_min_score():
    from topos.logic.omega import ClassificationResult

    classifier = SubobjectClassifier()

    r1 = ClassificationResult(
        is_parseable=True,
        dimensions={"structural": EvaluationValue.SELF_CONTAINED},
        scores={"structural": 0.8},
        lattice_element=EvaluationValue.SELF_CONTAINED,
    )
    r2 = ClassificationResult(
        is_parseable=True,
        dimensions={"structural": EvaluationValue.BROKEN},
        scores={"structural": 0.4},
        lattice_element=EvaluationValue.BROKEN,
    )

    combined = classifier.combine_dimensions([r1, r2])
    # Min score = 0.4, below threshold 0.6 → BROKEN
    assert combined["structural"] == EvaluationValue.BROKEN
