"""
Entropy Module
--------------
Approximates the 'Algorithmic Debt' via Kolmogorov complexity proxy.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class EntropyResult:
    """
    The result of entropy analysis.
    """

    ratio: float
    compressed_size: int
    original_size: int
    interpretation: str

    def __str__(self) -> str:
        return (
            f"Entropy: {self.ratio:.3f} "
            f"(compressed={self.compressed_size}, original={self.original_size}, "
            f"interpretation='{self.interpretation}')"
        )


def calculate_kolmogorov_proxy(source: str) -> float:
    """
    Estimate Kolmogorov complexity via compression ratio.
    """
    from topos.topos_functors import calculate_kolmogorov_proxy as rust_calc
    return rust_calc(source)


def calculate_entropy_detailed(source: str) -> EntropyResult:
    """
    Perform detailed entropy analysis with interpretation.
    """
    from topos.topos_functors import calculate_entropy_detailed as rust_calc
    res = rust_calc(source)
    return EntropyResult(
        ratio=res.ratio,
        compressed_size=res.compressed_size,
        original_size=res.original_size,
        interpretation=res.interpretation,
    )


def calculate_block_entropy(source: str, block_size: int = 100) -> list[float]:
    """
    Calculate entropy for each block of the source.
    """
    from topos.topos_functors import calculate_block_entropy as rust_calc
    return rust_calc(source, block_size)


def calculate_entropy_variance(source: str, block_size: int = 100) -> float:
    """
    Calculate variance in entropy across code blocks.
    """
    from topos.topos_functors import calculate_entropy_variance as rust_calc
    return rust_calc(source, block_size)
