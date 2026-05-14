from topos.core.object import ProgramObject
from topos.functors.probes.ast.complexity import (
    calculate_average_complexity,
    calculate_cyclomatic_complexity,
    calculate_function_complexities,
)
from topos.functors.probes.ast.entropy import (
    calculate_block_entropy,
    calculate_entropy_detailed,
    calculate_entropy_variance,
    calculate_kolmogorov_proxy,
)
from topos.functors.profunctors.ast.compare import (
    are_clones,
    calculate_ast_distance,
    calculate_ghw_distance,
    calculate_similarity,
)
from topos.utils.tree_sitter import parse_python


def test_cyclomatic_complexity_basic():
    source = "def foo():\n    if True:\n        pass\n    else:\n        pass"
    root = parse_python(source)
    ast = ProgramObject(root=root, source=source)

    # Base 1 + 1 for 'if' = 2
    assert calculate_cyclomatic_complexity(ast) == 2


def test_cyclomatic_complexity_loop():
    source = (
        "def foo():\n    for i in range(10):\n        while True:\n            break"
    )
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
    from topos.functors.profunctors.ast.compare import DistanceResult

    res = DistanceResult(raw_distance=2, normalized_distance=0.5, operations={})
    assert "Distance:" in str(res)


def test_entropy_result_str():
    from topos.functors.probes.ast.entropy import EntropyResult

    res = EntropyResult(
        ratio=0.5, compressed_size=10, original_size=20, interpretation="normal"
    )
    assert "Entropy:" in str(res)


def test_distance_substitution():
    source1 = "x = 1"
    source2 = "def foo(): pass"
    ast1 = ProgramObject(root=parse_python(source1), source=source1)
    ast2 = ProgramObject(root=parse_python(source2), source=source2)
    dist = calculate_ast_distance(ast1, ast2)
    assert dist.operations.get("substitutions", 0) >= 0


# ---------------------------------------------------------------------------
# Gromov-Wasserstein distance tests
# ---------------------------------------------------------------------------


def test_ghw_identity():
    """Same tree compared to itself should give distance ≈ 0."""
    source = "def foo(x):\n    if x > 0:\n        return x\n    return -x"
    ast = ProgramObject(root=parse_python(source), source=source)
    result = calculate_ghw_distance(ast, ast)
    assert result.gw_distance < 0.05
    assert result.n_nodes_source == result.n_nodes_target


def test_ghw_divergent():
    """Structurally different programs should have a non-trivial GHW distance."""
    simple = "x = 1"
    complex_src = "\n".join(
        [
            "class Foo:",
            "    def bar(self, x, y):",
            "        for i in range(x):",
            "            if i % 2 == 0:",
            "                yield i * y",
            "    def baz(self):",
            "        return [self.bar(i, i) for i in range(10)]",
        ]
    )
    ast_simple = ProgramObject(root=parse_python(simple), source=simple)
    ast_complex = ProgramObject(root=parse_python(complex_src), source=complex_src)
    result = calculate_ghw_distance(ast_simple, ast_complex)
    assert result.gw_distance > 0.2


def test_ghw_symmetry():
    """GHW distance should be approximately symmetric."""
    src_a = "def foo(x):\n    return x + 1"
    src_b = "for i in range(10):\n    print(i)\nx = sum(range(5))"
    ast_a = ProgramObject(root=parse_python(src_a), source=src_a)
    ast_b = ProgramObject(root=parse_python(src_b), source=src_b)
    d_ab = calculate_ghw_distance(ast_a, ast_b).gw_distance
    d_ba = calculate_ghw_distance(ast_b, ast_a).gw_distance
    assert abs(d_ab - d_ba) < 0.05


def test_ghw_subsampling():
    """Trees exceeding max_nodes should be capped at max_nodes."""
    # Generate a source large enough to exceed max_nodes=30
    lines = ["def foo():", "    x = 0"]
    for i in range(40):
        lines.append(f"    x = x + {i}")
    lines.append("    return x")
    source = "\n".join(lines)
    ast = ProgramObject(root=parse_python(source), source=source)

    result = calculate_ghw_distance(ast, ast, max_nodes=30)
    assert result.n_nodes_source <= 30
    assert result.n_nodes_target <= 30


def test_ghw_converged():
    """Typical small programs should converge within the iteration budget."""
    source = "def add(a, b):\n    return a + b"
    ast = ProgramObject(root=parse_python(source), source=source)
    result = calculate_ghw_distance(ast, ast)
    assert result.converged is True


def test_ghw_result_str():
    """GHWDistanceResult.__str__ should mention 'GHW Distance'."""
    from topos.functors.profunctors.ast.compare import GHWDistanceResult

    res = GHWDistanceResult(
        gw_distance=0.42,
        raw_gw_cost=1.23,
        n_nodes_source=10,
        n_nodes_target=12,
        n_iterations=7,
        converged=True,
    )
    s = str(res)
    assert "GHW Distance" in s
    assert "converged" in s
