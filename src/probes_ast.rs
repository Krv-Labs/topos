use pyo3::prelude::*;

const ENTROPY_SIZE_FLOOR_BYTES: usize = 200;
const ENTROPY_NEUTRAL_RATIO: f64 = 0.5; // mirrors SIMPLE.entropy_ideal

/// Raw zlib compressed/original byte ratio with a tiny-input correction for
/// zlib's fixed per-stream overhead.  Below `ENTROPY_SIZE_FLOOR_BYTES`, only
/// ratios above the neutral baseline are blended down toward neutral: this
/// removes false high-entropy failures on tiny dense snippets without hiding
/// genuinely low-entropy repetitive code.
fn compressed_ratio(source_bytes: &[u8]) -> (usize, f64) {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    use std::io::Write;
    encoder.write_all(source_bytes).unwrap();
    let compressed = encoder.finish().unwrap();
    let compressed_len = compressed.len();

    let raw_ratio = compressed_len as f64 / source_bytes.len() as f64;
    let ratio = if source_bytes.len() >= ENTROPY_SIZE_FLOOR_BYTES
        || raw_ratio <= ENTROPY_NEUTRAL_RATIO
    {
        raw_ratio
    } else {
        let confidence = source_bytes.len() as f64 / ENTROPY_SIZE_FLOOR_BYTES as f64;
        confidence * raw_ratio + (1.0 - confidence) * ENTROPY_NEUTRAL_RATIO
    };
    (compressed_len, ratio)
}

#[pyfunction]
pub fn calculate_kolmogorov_proxy(source: &str) -> f64 {
    if source.is_empty() {
        return 0.0;
    }

    let binding = source.replace("\r\n", "\n");
    let source_bytes = binding.as_bytes();
    let (_, ratio) = compressed_ratio(source_bytes);

    ratio
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

    let binding = source.replace("\r\n", "\n");
    let source_bytes = binding.as_bytes();
    let (compressed_size, ratio) = compressed_ratio(source_bytes);

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
        compressed_size,
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
        // Need to be careful with UTF-8 boundaries if block_size is in chars,
        // but the Python version used slicing which is also tricky.
        // For simplicity, we'll follow Python's slicing behavior (chars).
        let block: String = source.chars().skip(start).take(block_size).collect();
        if block.is_empty() {
            break;
        }
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
    let variance: f64 = block_entropies
        .iter()
        .map(|&e| (e - mean).powi(2))
        .sum::<f64>()
        / n;

    variance
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures that an empty string returns a baseline Kolmogorov complexity proxy (compression ratio) of 0.0.
    #[test]
    fn test_kolmogorov_proxy_empty() {
        assert_eq!(calculate_kolmogorov_proxy(""), 0.0);
    }

    /// Verifies that highly repetitive text compresses extremely well, leading to a very low compression ratio.
    #[test]
    fn test_kolmogorov_proxy_redundant() {
        let s = "a".repeat(1000);
        let ratio = calculate_kolmogorov_proxy(&s);
        assert!(ratio < 0.1); // High redundancy should lead to low ratio
    }

    /// Checks that the detailed entropy function properly extracts file sizes and produces an interpretation message.
    #[test]
    fn test_entropy_detailed() {
        let result = calculate_entropy_detailed("def foo():\n    return 42");
        assert!(result.ratio > 0.0);
        assert!(result.original_size > 0);
        assert!(!result.interpretation.is_empty());
    }

    /// Validates that text is correctly partitioned into chunks and individual chunk entropies are calculated.
    #[test]
    fn test_block_entropy() {
        let s = "abcdefghijklmnopqrstuvwxyz";
        let blocks = calculate_block_entropy(s, 10);
        assert_eq!(blocks.len(), 3); // 10, 10, 6
    }

    /// Ensures the variance of block entropies is computed properly and does not fail on mixed-pattern strings.
    #[test]
    fn test_entropy_variance() {
        let s = "a".repeat(10) + &"b".repeat(10);
        let variance = calculate_entropy_variance(&s, 5);
        assert!(variance >= 0.0);
    }

    /// Issue #152: a tiny, structurally-simple array-lookup function must no
    /// longer score above the SIMPLE `max_entropy` gate (0.8) just because
    /// zlib's fixed per-stream overhead dominates the ratio on short input.
    #[test]
    fn test_kolmogorov_proxy_tiny_dense_function_stays_in_band() {
        let array_lookup = "pub fn probe(x: u8) -> &'static str {\n    const T: [&str; 8] = [\"a\", \"b\", \"c\", \"d\", \"e\", \"f\", \"g\", \"h\"];\n    T[x as usize]\n}\n";
        let ratio = calculate_kolmogorov_proxy(array_lookup);
        assert!(
            ratio <= 0.8,
            "expected tiny array-lookup fn to pass the entropy gate, got {ratio}"
        );
    }

    /// Above the size floor, the blend must be a no-op: the ratio should
    /// equal the plain compressed/original byte ratio exactly.
    #[test]
    fn test_kolmogorov_proxy_unaffected_above_size_floor() {
        let s = "x".repeat(ENTROPY_SIZE_FLOOR_BYTES + 1);
        let (compressed_len, ratio) = compressed_ratio(s.as_bytes());
        let raw_ratio = compressed_len as f64 / s.len() as f64;
        assert_eq!(ratio, raw_ratio);
    }

    /// Below the size floor, the blended ratio must sit strictly between
    /// the raw (unblended) ratio and the neutral baseline.
    #[test]
    fn test_kolmogorov_proxy_blends_toward_neutral_below_floor() {
        let s = "x=1";
        let (compressed_len, blended) = compressed_ratio(s.as_bytes());
        let raw_ratio = compressed_len as f64 / s.len() as f64;
        assert!(raw_ratio > ENTROPY_NEUTRAL_RATIO);
        assert!(blended < raw_ratio);
        assert!(blended > ENTROPY_NEUTRAL_RATIO);
    }

    /// Below-floor repetitive code should keep its raw low-entropy signal.
    #[test]
    fn test_kolmogorov_proxy_does_not_lift_low_entropy_below_floor() {
        let s = "a".repeat(100);
        let (compressed_len, ratio) = compressed_ratio(s.as_bytes());
        let raw_ratio = compressed_len as f64 / s.len() as f64;
        assert!(raw_ratio < ENTROPY_NEUTRAL_RATIO);
        assert_eq!(ratio, raw_ratio);
    }
}
