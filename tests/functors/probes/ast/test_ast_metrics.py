from topos.core.object import ProgramObject
from topos.functors.probes.ast.entropy import (
    calculate_block_entropy,
    calculate_entropy_variance,
    calculate_kolmogorov_proxy,
)
from topos.functors.profunctors.ast.compare import (
    calculate_ast_distance,
    calculate_ghw_distance,
    calculate_similarity,
)
from topos.utils.tree_sitter import parse_python


def test_kolmogorov_proxy():
    simple_source = "x = 1" * 100
    complex_source = "".join(chr(i % 256) for i in range(400))

    simple_entropy = calculate_kolmogorov_proxy(simple_source)
    complex_entropy = calculate_kolmogorov_proxy(complex_source)

    # Simple, repetitive source should be more compressible (lower ratio)
    assert simple_entropy < complex_entropy


def test_entropy_empty_source():
    assert calculate_kolmogorov_proxy("") == 0.0


def test_block_entropy():
    source = "x = 1\n" * 50
    blocks = calculate_block_entropy(source, block_size=10)
    assert len(blocks) > 0
    variance = calculate_entropy_variance(source, block_size=10)
    assert variance >= 0.0

    assert calculate_entropy_variance("") == 0.0


def test_kolmogorov_proxy_tiny_dense_function_passes_simple_gate():
    """Issue #152: a 3-line array-lookup refactor of an 8-arm match must not
    score worse than the match it replaced, and must fall within the SIMPLE
    entropy band -- previously zlib's fixed per-stream overhead dominated
    the ratio on such a short input and flagged this textbook simplifying
    refactor as a regression."""
    from topos.evaluation.policies.calibration import SIMPLE

    array_lookup = (
        "pub fn probe(x: u8) -> &'static str {\n"
        '    const T: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];\n'
        "    T[x as usize]\n"
        "}\n"
    )
    match8 = (
        "pub fn probe(x: u8) -> &'static str {\n"
        "    match x {\n"
        '        0 => "a", 1 => "b", 2 => "c", 3 => "d",\n'
        '        4 => "e", 5 => "f", 6 => "g", _ => "h",\n'
        "    }\n"
        "}\n"
    )

    array_ratio = calculate_kolmogorov_proxy(array_lookup)
    match_ratio = calculate_kolmogorov_proxy(match8)

    assert SIMPLE.min_entropy <= array_ratio <= SIMPLE.max_entropy
    assert SIMPLE.min_entropy <= match_ratio <= SIMPLE.max_entropy
    assert array_ratio <= match_ratio + 0.1


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


def test_distance_result_str():
    from topos.functors.profunctors.ast.compare import DistanceResult

    res = DistanceResult(raw_distance=2, normalized_distance=0.5, operations={})
    assert "Distance:" in str(res)


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
