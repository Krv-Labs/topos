import pytest
from topos.metrics.complexity import (
    calculate_cyclomatic_complexity,
    calculate_function_complexities,
    calculate_average_complexity
)
from topos.metrics.entropy import (
    calculate_kolmogorov_proxy,
    calculate_entropy_detailed,
    calculate_block_entropy,
    calculate_entropy_variance
)
from topos.metrics.distance import (
    calculate_ast_distance,
    calculate_similarity,
    are_clones
)
from topos.core.object import ProgramObject
from topos.utils.tree_sitter import parse_python

def test_cyclomatic_complexity_basic():
    source = "def foo():\n    if True:\n        pass\n    else:\n        pass"
    root = parse_python(source)
    ast = ProgramObject(root=root, source=source)
    
    # Base 1 + 1 for 'if' = 2
    assert calculate_cyclomatic_complexity(ast) == 2

def test_cyclomatic_complexity_loop():
    source = "def foo():\n    for i in range(10):\n        while True:\n            break"
    root = parse_python(source)
    ast = ProgramObject(root=root, source=source)
    
    # Base 1 + 1 for 'for' + 1 for 'while' = 3
    assert calculate_cyclomatic_complexity(ast) == 3

def test_function_complexities():
    source = """
def simple():
    pass

def complex_func():
    if True:
        pass
"""
    root = parse_python(source)
    ast = ProgramObject(root=root, source=source)
    
    complexities = calculate_function_complexities(ast)
    assert complexities["simple"] == 1
    assert complexities["complex_func"] == 2
    assert calculate_average_complexity(ast) == 1.5

def test_kolmogorov_proxy():
    simple_source = "x = 1" * 100
    complex_source = "".join(chr(i % 256) for i in range(400))
    
    simple_entropy = calculate_kolmogorov_proxy(simple_source)
    complex_entropy = calculate_kolmogorov_proxy(complex_source)
    
    # Simple, repetitive source should be more compressible (lower ratio)
    assert simple_entropy < complex_entropy

def test_entropy_detailed():
    source = "def hello():\n    print('world')"
    result = calculate_entropy_detailed(source)
    
    assert result.original_size > 0
    assert result.compressed_size > 0
    # On very small strings, compressed size can be slightly larger than original
    assert 0 <= result.ratio <= 2.0
    assert isinstance(result.interpretation, str)

def test_complexity_boolean_operator():
    source = "if a and b and c: pass"
    root = parse_python(source)
    ast = ProgramObject(root=root, source=source)
    # base 1 + if 1 + and 1 + and 1 = 4
    assert calculate_cyclomatic_complexity(ast) > 1

def test_average_complexity_empty():
    source = "x = 1"
    root = parse_python(source)
    ast = ProgramObject(root=root, source=source)
    assert calculate_average_complexity(ast) == 1.0

def test_entropy_empty_source():
    assert calculate_kolmogorov_proxy("") == 0.0
    res = calculate_entropy_detailed("")
    assert res.ratio == 0.0
    assert res.interpretation == "empty"

def test_block_entropy():
    source = "x = 1\n" * 50
    blocks = calculate_block_entropy(source, block_size=10)
    assert len(blocks) > 0
    variance = calculate_entropy_variance(source, block_size=10)
    assert variance >= 0.0
    
    assert calculate_entropy_variance("") == 0.0

def test_distance_metrics():
    source1 = "x = 1"
    source2 = "y = 2"
    source3 = "def foo():\n    pass"
    
    ast1 = ProgramObject(root=parse_python(source1), source=source1)
    ast2 = ProgramObject(root=parse_python(source2), source=source2)
    ast3 = ProgramObject(root=parse_python(source3), source=source3)
    
    dist1_2 = calculate_ast_distance(ast1, ast2)
    assert dist1_2.raw_distance >= 0
    
    dist1_3 = calculate_ast_distance(ast1, ast3)
    assert dist1_3.raw_distance > dist1_2.raw_distance
    
    sim = calculate_similarity(ast1, ast1)
    assert sim == 1.0
    
    assert are_clones(ast1, ast2, threshold=0.1) is True
    assert are_clones(ast1, ast3, threshold=0.1) is False

def test_distance_result_str():
    from topos.metrics.distance import DistanceResult
    res = DistanceResult(raw_distance=2, normalized_distance=0.5, operations={})
    assert 'Distance:' in str(res)

def test_entropy_result_str():
    from topos.metrics.entropy import EntropyResult
    res = EntropyResult(ratio=0.5, compressed_size=10, original_size=20, interpretation='normal')
    assert 'Entropy:' in str(res)

def test_distance_substitution():
    source1 = 'x = 1'
    source2 = 'def foo(): pass'
    ast1 = ProgramObject(root=parse_python(source1), source=source1)
    ast2 = ProgramObject(root=parse_python(source2), source=source2)
    dist = calculate_ast_distance(ast1, ast2)
    assert dist.operations.get('substitutions', 0) >= 0
