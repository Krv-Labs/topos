"""
Tests for the free Heyting algebra H(G_qual) and the subobject classifier Ω.

The lattice has 8 elements on the 3-cube of generators
{SIMPLE, COMPOSABLE, SECURE}.  Top is IDEAL (all generators satisfied);
bottom is SLOP (none).
"""

from topos.core.morphism import ProgramMorphism
from topos.core.omega import (
    EvaluationValue,
    Omega,
    verdict_from_generators,
)
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.evaluation.policies.base import Priority


def test_evaluation_value_order():
    lattice = Omega()

    # SLOP ≤ every generator ≤ every pair ≤ IDEAL
    for gen in (
        EvaluationValue.SIMPLE,
        EvaluationValue.COMPOSABLE,
        EvaluationValue.SECURE,
    ):
        assert lattice.leq(EvaluationValue.SLOP, gen)
        assert lattice.leq(gen, EvaluationValue.IDEAL)

    # The three generators are pairwise incomparable
    pairs = [
        (EvaluationValue.SIMPLE, EvaluationValue.COMPOSABLE),
        (EvaluationValue.SIMPLE, EvaluationValue.SECURE),
        (EvaluationValue.COMPOSABLE, EvaluationValue.SECURE),
    ]
    for a, b in pairs:
        assert not lattice.leq(a, b)
        assert not lattice.leq(b, a)

    # SIMPLE ≤ SIMPLE_COMPOSABLE ≤ IDEAL but not SIMPLE_COMPOSABLE ≤ SECURE
    assert lattice.leq(EvaluationValue.SIMPLE, EvaluationValue.SIMPLE_COMPOSABLE)
    assert lattice.leq(EvaluationValue.SIMPLE_COMPOSABLE, EvaluationValue.IDEAL)
    assert not lattice.leq(EvaluationValue.SIMPLE_COMPOSABLE, EvaluationValue.SECURE)


def test_lattice_meet_join():
    lattice = Omega()

    # Pairwise incomparable atoms meet at SLOP (standard lattice GLB)
    assert (
        lattice.meet(EvaluationValue.SIMPLE, EvaluationValue.COMPOSABLE)
        == EvaluationValue.SLOP
    )
    assert (
        lattice.meet(EvaluationValue.SIMPLE, EvaluationValue.SECURE)
        == EvaluationValue.SLOP
    )

    # IDEAL is the top: meet(IDEAL, x) = x
    assert (
        lattice.meet(EvaluationValue.IDEAL, EvaluationValue.SIMPLE)
        == EvaluationValue.SIMPLE
    )
    assert (
        lattice.meet(EvaluationValue.IDEAL, EvaluationValue.SLOP)
        == EvaluationValue.SLOP
    )

    # Join (LUB): SIMPLE ∨ COMPOSABLE = SIMPLE_COMPOSABLE
    assert (
        lattice.join(EvaluationValue.SIMPLE, EvaluationValue.COMPOSABLE)
        == EvaluationValue.SIMPLE_COMPOSABLE
    )
    assert (
        lattice.join(EvaluationValue.SIMPLE_COMPOSABLE, EvaluationValue.SECURE)
        == EvaluationValue.IDEAL
    )
    assert (
        lattice.join(EvaluationValue.SLOP, EvaluationValue.SIMPLE)
        == EvaluationValue.SIMPLE
    )


def test_verdict_from_generators_truth_table():
    # Every subset of {simple, composable, secure} → a unique verdict
    assert (
        verdict_from_generators(simple=False, composable=False, secure=False)
        == EvaluationValue.SLOP
    )
    assert (
        verdict_from_generators(simple=True, composable=False, secure=False)
        == EvaluationValue.SIMPLE
    )
    assert (
        verdict_from_generators(simple=False, composable=True, secure=False)
        == EvaluationValue.COMPOSABLE
    )
    assert (
        verdict_from_generators(simple=False, composable=False, secure=True)
        == EvaluationValue.SECURE
    )
    assert (
        verdict_from_generators(simple=True, composable=True, secure=False)
        == EvaluationValue.SIMPLE_COMPOSABLE
    )
    assert (
        verdict_from_generators(simple=True, composable=False, secure=True)
        == EvaluationValue.SIMPLE_SECURE
    )
    assert (
        verdict_from_generators(simple=False, composable=True, secure=True)
        == EvaluationValue.COMPOSABLE_SECURE
    )
    assert (
        verdict_from_generators(simple=True, composable=True, secure=True)
        == EvaluationValue.IDEAL
    )


def test_subobject_classifier_simple_generator():
    classifier = CharacteristicMorphism()

    source = """
def process_data(data):
    result = []
    for item in data:
        if item is not None:
            result.append(item * 2)
    return result
"""
    morphism = ProgramMorphism(source=source)
    # Attach CFG so the SIMPLE generator gets the cyclomatic probe.
    cfg = morphism.build_cfg()

    result = classifier.classify_detailed(morphism, representations=[cfg])
    assert result.is_parseable is True
    assert "simple" in result.dimensions

    # Check that score is high enough
    score = result.scores.get("simple", 0.0)
    assert score > 0.5, f"Expected SIMPLE score > 0.5, got {score}"

    # If the score is below the 0.6 threshold it will be SLOP
    assert result.dimensions["simple"] in {EvaluationValue.SIMPLE, EvaluationValue.SLOP}
    assert 0.0 <= result.scores["simple"] <= 1.0


def test_subobject_classifier_invalid():
    classifier = CharacteristicMorphism()

    source = "def broken(:"
    morphism = ProgramMorphism(source=source)

    evaluation = classifier.classify(morphism)
    assert evaluation == EvaluationValue.SLOP


def test_classifier_aggregation():
    classifier = CharacteristicMorphism()

    # meet(IDEAL, COMPOSABLE) = COMPOSABLE
    assert (
        classifier.combine(EvaluationValue.IDEAL, EvaluationValue.COMPOSABLE)
        == EvaluationValue.COMPOSABLE
    )

    # meet(SIMPLE, SECURE) = SLOP (pairwise incomparable atoms)
    assert (
        classifier.combine(EvaluationValue.SIMPLE, EvaluationValue.SECURE)
        == EvaluationValue.SLOP
    )


def test_evaluation_value_properties():
    assert EvaluationValue.IDEAL.symbol == "🥇"
    assert EvaluationValue.SLOP.symbol == "❌"
    assert EvaluationValue.SIMPLE.symbol == "🥉"
    assert EvaluationValue.COMPOSABLE.symbol == "🥉"
    assert EvaluationValue.SECURE.symbol == "🥉"

    assert "gold" in EvaluationValue.IDEAL.description.lower()
    assert (
        "composable" in EvaluationValue.COMPOSABLE.description.lower()
        or "coupling" in EvaluationValue.COMPOSABLE.description.lower()
    )


def test_lattice_implies_and_negation():
    lattice = Omega()

    # negation(SLOP) = IDEAL (bottom → bottom = top)
    assert lattice.negation(EvaluationValue.SLOP) == EvaluationValue.IDEAL
    # negation(IDEAL) = SLOP
    assert lattice.negation(EvaluationValue.IDEAL) == EvaluationValue.SLOP

    for val in EvaluationValue:
        assert lattice.equivalent(val, val)


def test_subobject_classifier_str():
    classifier = CharacteristicMorphism()
    morphism = ProgramMorphism(source="x = 1")
    res = classifier.classify_detailed(morphism)
    s = str(res)
    # Should reference at least one generator dimension
    assert any(g in s for g in ("simple", "composable", "secure"))


def test_priority_weight_profiles():
    from topos.evaluation.policies.base import WEIGHT_PROFILES

    for priority in Priority:
        profile = WEIGHT_PROFILES[priority]
        assert 0.0 <= profile.w_complexity <= 1.0
        assert 0.0 <= profile.w_coupling <= 1.0
        assert 0.0 <= profile.w_taint <= 1.0


def test_score_simple_perfect_code():
    from topos.evaluation.policies.simple import score_simple

    # Ideal: cyclomatic=0, entropy=0.5, max_func=0 — score is 1.0
    result = score_simple(0, 0.5, 0, Priority.SECURE)
    assert result.score == 1.0
    assert result.achieved is True


def test_score_simple_pathological_code():
    from topos.evaluation.policies.simple import score_simple

    # Worst case: cyclomatic=40, entropy=1.0, max_func=20 — score is 0.0
    result = score_simple(40, 1.0, 20, Priority.SECURE)
    assert result.score == 0.0
    assert result.achieved is False


def test_score_simple_independent_thresholds():
    from topos.evaluation.policies.simple import score_simple

    # Pass all 3
    result = score_simple(10, 0.5, 5)
    assert result.achieved is True

    # Fail cyclomatic
    result = score_simple(16, 0.5, 5)
    assert result.achieved is False

    # Fail entropy
    result = score_simple(10, 0.9, 5)
    assert result.achieved is False

    # Fail max func
    result = score_simple(10, 0.5, 11)
    assert result.achieved is False


def test_score_secure_clean_code():
    from topos.evaluation.policies.secure import score_secure

    # Ideal case: 0 danger, 0 taint → score is 1.0
    result = score_secure(dangerous_calls=0, taint_flows=0)
    assert result.score == 1.0
    assert result.achieved is True


def test_score_secure_dangerous_code():
    from topos.evaluation.policies.secure import score_secure

    # Pathological case: many issues → low score, not achieved
    result = score_secure(dangerous_calls=20, taint_flows=20)
    assert result.score < 0.1
    assert result.achieved is False


def test_score_secure_independent_thresholds():
    from topos.evaluation.policies.secure import score_secure

    # Pass both
    assert score_secure(dangerous_calls=0, taint_flows=0).achieved is True

    # Fail danger
    assert score_secure(dangerous_calls=1, taint_flows=0).achieved is False

    # Fail taint
    assert score_secure(dangerous_calls=0, taint_flows=1).achieved is False


def test_classify_detailed_priority_parameter():
    classifier = CharacteristicMorphism()
    morphism = ProgramMorphism(source="x = 1 + 2")

    result_secure = classifier.classify_detailed(morphism, priority=Priority.SECURE)
    result_simple = classifier.classify_detailed(morphism, priority=Priority.SIMPLE)

    assert result_secure.priority == Priority.SECURE
    assert result_simple.priority == Priority.SIMPLE
    assert result_secure.is_parseable
    assert result_simple.is_parseable


def test_combine_dimensions_uses_min_score():
    classifier = CharacteristicMorphism()

    r1 = ClassificationResult(
        is_parseable=True,
        dimensions={"simple": EvaluationValue.SIMPLE},
        scores={"simple": 0.8},
        lattice_element=EvaluationValue.SIMPLE,
    )
    r2 = ClassificationResult(
        is_parseable=True,
        dimensions={"simple": EvaluationValue.SLOP},
        scores={"simple": 0.3},
        lattice_element=EvaluationValue.SLOP,
    )

    combined = classifier.combine_dimensions([r1, r2])
    # Min score = 0.3, below calibrated threshold 0.40 → SLOP for SIMPLE
    assert combined["simple"] == EvaluationValue.SLOP


def test_combine_dimensions_counts_parse_failures_as_simple_slop():
    classifier = CharacteristicMorphism()

    good = ClassificationResult(
        is_parseable=True,
        dimensions={"simple": EvaluationValue.SIMPLE},
        scores={"simple": 0.9},
        lattice_element=EvaluationValue.SIMPLE,
    )
    parse_failure = ClassificationResult(is_parseable=False)

    combined = classifier.combine_dimensions([good, parse_failure])
    assert combined["simple"] == EvaluationValue.SLOP
