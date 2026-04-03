import pytest
from topos.logic.lattice import EvaluationValue, EvaluationLattice
from topos.logic.omega import SubobjectClassifier
from topos.core.morphism import ProgramMorphism

def test_evaluation_value_order():
    # Basic ordering checks if they were a total chain (they aren't, but let's check the lattice)
    lattice = EvaluationLattice() # default uses total chain fallback or DEFAULT_COVER
    
    # Using the DEFAULT_COVER which is non-total
    assert lattice.leq(EvaluationValue.INVALID, EvaluationValue.VERIFIED)
    assert lattice.leq(EvaluationValue.COMMODITY, EvaluationValue.VERIFIED)
    assert lattice.leq(EvaluationValue.NOISY, EvaluationValue.COMMODITY)
    
    # Check incomparable elements
    # NOISY and WEAK are both <= COMMODITY but not necessarily related to each other in a simple chain
    # In DEFAULT_COVER:
    # INVALID -> HALLUCINATED, NOISY, WEAK, COMMODITY
    # NOISY -> COMMODITY
    # WEAK -> COMMODITY
    # COMMODITY -> VERIFIED
    
    assert not lattice.leq(EvaluationValue.NOISY, EvaluationValue.WEAK)
    assert not lattice.leq(EvaluationValue.WEAK, EvaluationValue.NOISY)

def test_lattice_meet_join():
    lattice = EvaluationLattice()
    
    # Meet (And / GLB)
    assert lattice.meet(EvaluationValue.VERIFIED, EvaluationValue.COMMODITY) == EvaluationValue.COMMODITY
    assert lattice.meet(EvaluationValue.NOISY, EvaluationValue.WEAK) == EvaluationValue.INVALID
    
    # Join (Or / LUB)
    assert lattice.join(EvaluationValue.NOISY, EvaluationValue.WEAK) == EvaluationValue.COMMODITY
    assert lattice.join(EvaluationValue.INVALID, EvaluationValue.VERIFIED) == EvaluationValue.VERIFIED

def test_subobject_classifier_simple():
    classifier = SubobjectClassifier()
    
    # Use a slightly longer piece of code to avoid high compression ratios (entropy) on tiny strings
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
    assert result.is_valid is True
    # Clean code should be VERIFIED or at least COMMODITY
    assert result.evaluation >= EvaluationValue.COMMODITY

def test_subobject_classifier_invalid():
    classifier = SubobjectClassifier()
    
    source = "def broken(:"
    morphism = ProgramMorphism(source=source)
    
    evaluation = classifier.classify(morphism)
    assert evaluation == EvaluationValue.INVALID

def test_classifier_aggregation():
    classifier = SubobjectClassifier()
    
    # Meet of VERIFIED and NOISY should be NOISY (if they are in a chain)
    # Wait, in our lattice NOISY <= COMMODITY <= VERIFIED.
    # So meet(VERIFIED, NOISY) = NOISY.
    
    val1 = EvaluationValue.VERIFIED
    val2 = EvaluationValue.NOISY
    
    combined = classifier.combine(val1, val2)
    assert combined == EvaluationValue.NOISY

from topos.logic.policies import EvaluationSection, ObservationBin, section

def test_evaluation_value_properties():
    val = EvaluationValue.VERIFIED
    assert val.symbol == '⊤'
    assert 'Verified' in val.description
    assert 'VERIFIED' in str(val)

def test_lattice_implies_and_negation():
    lattice = EvaluationLattice()
    
    # negation = implies(val, BOTTOM)
    # VERIFIED -> INVALID should be INVALID or similar in Heyting
    assert lattice.negation(EvaluationValue.INVALID) == EvaluationValue.VERIFIED
    
    # equivalent
    assert lattice.equivalent(EvaluationValue.COMMODITY, EvaluationValue.COMMODITY) is True
    assert lattice.equivalent(EvaluationValue.COMMODITY, EvaluationValue.VERIFIED) is False

def test_subobject_classifier_str():
    classifier = SubobjectClassifier()
    morphism = ProgramMorphism(source='x = 1')
    res = classifier.classify_detailed(morphism)
    assert 'Classification:' in str(res)

def test_policies_classify_error():
    with pytest.raises(ValueError):
        section._classify(-100.0, section.complexity_bins)
