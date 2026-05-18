use pyo3::prelude::*;
use std::collections::HashMap;

#[pyfunction]
pub fn calculate_kolmogorov_proxy(source: &str) -> f64 {
    if source.is_empty() {
        return 0.0;
    }

    let source_bytes = source.replace("\r\n", "\n").as_bytes();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    use std::io::Write;
    encoder.write_all(source_bytes).unwrap();
    let compressed = encoder.finish().unwrap();

    compressed.len() as f64 / source_bytes.len() as f64
}

#[pyclass(get_all)]
pub struct EntropyResult {
    pub ratio: f64,
    pub compressed_size: usize,
    pub original_size: usize,
    pub interpretation: String,
}

#[pyfunction]
pub fn calculate_entropy_detailed(source: &str) -> EntropyResult {
    if source.is_empty() {
        return EntropyResult {
            ratio: 0.0,
            compressed_size: 0,
            original_size: 0,
            interpretation: "empty".to_string(),
        };
    }

    let source_bytes = source.replace("\r\n", "\n").as_bytes();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    use std::io::Write;
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

#[pyfunction]
pub fn calculate_block_entropy(source: &str, block_size: usize) -> Vec<f64> {
    if source.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let end = (start + block_size).min(source.len());
        // Need to be careful with UTF-8 boundaries if block_size is in chars, 
        // but the Python version used slicing which is also tricky.
        // For simplicity, we'll follow Python's slicing behavior (chars).
        let block: String = source.chars().skip(start).take(block_size).collect();
        if block.is_empty() { break; }
        results.push(calculate_kolmogorov_proxy(&block));
        start += block_size;
    }
    results
}

#[pyfunction]
pub fn calculate_entropy_variance(source: &str, block_size: usize) -> f64 {
    let block_entropies = calculate_block_entropy(source, block_size);

    if block_entropies.len() < 2 {
        return 0.0;
    }

    let n = block_entropies.len() as f64;
    let mean: f64 = block_entropies.iter().sum::<f64>() / n;
    let variance: f64 = block_entropies.iter().map(|&e| (e - mean).powi(2)).sum::<f64>() / n;

    variance
}
