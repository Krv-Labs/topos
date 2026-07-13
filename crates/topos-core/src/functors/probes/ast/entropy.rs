//! Entropy probe — approximates "algorithmic debt" via a Kolmogorov
//! complexity proxy (compression ratio).
//!
//! This is a relocation, not a new port: the engine already existed in
//! Rust before the v0.4.0 migration, backing a `topos-pyo3` probe since
//! Python already delegated this hot path to the Rust extension. Only
//! the `pyo3` annotations are stripped; the compression logic is
//! unchanged.

use std::io::Write;

/// Estimate Kolmogorov complexity via compression ratio.
///
/// `compressed_size / original_size`, after normalizing `\r\n` to `\n`
/// so the ratio isn't skewed by line-ending convention alone.
pub fn calculate_kolmogorov_proxy(source: &str) -> f64 {
    if source.is_empty() {
        return 0.0;
    }

    let normalized = source.replace("\r\n", "\n");
    let source_bytes = normalized.as_bytes();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    encoder.write_all(source_bytes).unwrap();
    let compressed = encoder.finish().unwrap();

    compressed.len() as f64 / source_bytes.len() as f64
}

/// Detailed entropy analysis result, with a human-readable interpretation.
#[derive(Debug, Clone, PartialEq)]
pub struct EntropyResult {
    pub ratio: f64,
    pub compressed_size: usize,
    pub original_size: usize,
    pub interpretation: String,
}

pub fn calculate_entropy_detailed(source: &str) -> EntropyResult {
    if source.is_empty() {
        return EntropyResult {
            ratio: 0.0,
            compressed_size: 0,
            original_size: 0,
            interpretation: "empty".to_string(),
        };
    }

    let normalized = source.replace("\r\n", "\n");
    let source_bytes = normalized.as_bytes();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    encoder.write_all(source_bytes).unwrap();
    let compressed = encoder.finish().unwrap();

    let ratio = compressed.len() as f64 / source_bytes.len() as f64;
    let interpretation = if ratio < 0.2 {
        "extreme redundancy (possible boilerplate or repetitive data)"
    } else if ratio < 0.5 {
        "low entropy (standard well-structured code)"
    } else if ratio < 0.8 {
        "moderate entropy (complex or dense logic)"
    } else {
        "high entropy (low redundancy; possible noise or obfuscation)"
    };

    EntropyResult {
        ratio,
        compressed_size: compressed.len(),
        original_size: source_bytes.len(),
        interpretation: interpretation.to_string(),
    }
}

/// Entropy for each `block_size`-character chunk of `source`.
pub fn calculate_block_entropy(source: &str, block_size: usize) -> Vec<f64> {
    if source.is_empty() {
        return Vec::new();
    }
    let mut results = Vec::new();
    let mut start = 0;
    loop {
        let block: String = source.chars().skip(start).take(block_size).collect();
        if block.is_empty() {
            break;
        }
        results.push(calculate_kolmogorov_proxy(&block));
        start += block_size;
    }
    results
}

/// Variance in entropy across code blocks — a proxy for "uneven" density.
pub fn calculate_entropy_variance(source: &str, block_size: usize) -> f64 {
    let block_entropies = calculate_block_entropy(source, block_size);
    if block_entropies.len() < 2 {
        return 0.0;
    }
    let n = block_entropies.len() as f64;
    let mean: f64 = block_entropies.iter().sum::<f64>() / n;
    block_entropies
        .iter()
        .map(|&e| (e - mean).powi(2))
        .sum::<f64>()
        / n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kolmogorov_proxy_empty_is_zero() {
        assert_eq!(calculate_kolmogorov_proxy(""), 0.0);
    }

    #[test]
    fn kolmogorov_proxy_redundant_text_compresses_well() {
        let s = "a".repeat(1000);
        assert!(calculate_kolmogorov_proxy(&s) < 0.1);
    }

    #[test]
    fn entropy_detailed_reports_size_and_interpretation() {
        let result = calculate_entropy_detailed("def foo():\n    return 42");
        assert!(result.ratio > 0.0);
        assert!(result.original_size > 0);
        assert!(!result.interpretation.is_empty());
    }

    #[test]
    fn block_entropy_partitions_into_expected_chunk_count() {
        let blocks = calculate_block_entropy("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(blocks.len(), 3); // 10, 10, 6
    }

    #[test]
    fn entropy_variance_is_non_negative() {
        let s = "a".repeat(10) + &"b".repeat(10);
        assert!(calculate_entropy_variance(&s, 5) >= 0.0);
    }
}
