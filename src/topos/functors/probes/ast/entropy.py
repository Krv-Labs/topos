"""
Entropy Module
--------------
Approximates the 'Algorithmic Debt' via Kolmogorov complexity proxy.

Mathematical Inspiration:
    Kolmogorov complexity K(x) is the length of the shortest program
    that produces x. It's uncomputable in general, but we can approximate
    it using compression ratios.

    For source code:
    - Low entropy (high compressibility) suggests repetitive, redundant code
    - High entropy (low compressibility) may indicate noise or obfuscation
    - Moderate entropy is typical of well-structured, meaningful code

    We use zlib compression as our proxy. The ratio of compressed to
    original size gives us an entropy estimate:

        entropy_proxy = len(compress(x)) / len(x)

    This module returns raw compression ratios and size data only.
    Any thresholding, labeling, or qualitative interpretation lives in
    the evaluation policies.
"""

from __future__ import annotations

import zlib
from dataclasses import dataclass


@dataclass
class EntropyResult:
    """
    The result of entropy analysis.

    Attributes:
        ratio: Compression ratio (compressed_size / original_size).
        compressed_size: Size after zlib compression.
        original_size: Original source size in bytes.
        interpretation: Human-readable interpretation of the entropy ratio.
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

    Uses zlib compression to approximate the intrinsic information
    content of the source code.

    Args:
        source: The source code as a string.

    Returns:
        A ratio in (0, 1] representing the compression ratio.
        Lower values indicate more compressible (redundant) code.
        Higher values indicate less compressible code.
    """
    if not source:
        return 0.0

    source_bytes = source.encode("utf-8")
    compressed = zlib.compress(source_bytes, level=9)

    return len(compressed) / len(source_bytes)


def calculate_entropy_detailed(source: str) -> EntropyResult:
    """
    Perform detailed entropy analysis with interpretation.

    Args:
        source: The source code as a string.

    Returns:
        An EntropyResult with raw ratio and size data.
    """
    if not source:
        return EntropyResult(
            ratio=0.0,
            compressed_size=0,
            original_size=0,
            interpretation="empty",
        )

    source_bytes = source.encode("utf-8")
    compressed = zlib.compress(source_bytes, level=9)

    ratio = len(compressed) / len(source_bytes)

    if ratio < 0.2:
        interp = "extreme redundancy (possible boilerplate or repetitive data)"
    elif ratio < 0.5:
        interp = "low entropy (standard well-structured code)"
    elif ratio < 0.8:
        interp = "moderate entropy (complex or dense logic)"
    else:
        interp = "high entropy (low redundancy; possible noise or obfuscation)"

    return EntropyResult(
        ratio=ratio,
        compressed_size=len(compressed),
        original_size=len(source_bytes),
        interpretation=interp,
    )


def calculate_block_entropy(source: str, block_size: int = 100) -> list[float]:
    """
    Calculate entropy for each block of the source.

    Useful for identifying sections of code with unusual entropy
    (e.g., embedded data, obfuscated sections).
    """
    if not source:
        return []

    blocks = [source[i : i + block_size] for i in range(0, len(source), block_size)]

    return [calculate_kolmogorov_proxy(block) for block in blocks]


def calculate_entropy_variance(source: str, block_size: int = 100) -> float:
    """
    Calculate variance in entropy across code blocks.

    High variance may indicate mixed content (e.g., code with
    embedded base64 data or dramatically different coding styles).
    """
    block_entropies = calculate_block_entropy(source, block_size)

    if len(block_entropies) < 2:
        return 0.0

    mean = sum(block_entropies) / len(block_entropies)
    variance = sum((e - mean) ** 2 for e in block_entropies) / len(block_entropies)

    return variance
