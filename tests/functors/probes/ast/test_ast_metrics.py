from topos.functors.probes.ast.entropy import (
    calculate_block_entropy,
    calculate_entropy_variance,
    calculate_kolmogorov_proxy,
)


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


def test_distance_result_str():
    from topos.topos_functors import DistanceResult

    res = DistanceResult(raw_distance=2, normalized_distance=0.5, operations={})
    assert "Distance:" in str(res)
