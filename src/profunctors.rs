use pyo3::prelude::*;
use std::collections::HashMap;

#[pyclass(get_all)]
pub struct DistanceResult {
    pub raw_distance: usize,
    pub normalized_distance: f64,
    pub operations: HashMap<String, usize>,
}

#[pyfunction]
pub fn compute_sequence_distance(
    source: Vec<String>,
    target: Vec<String>,
) -> (usize, HashMap<String, usize>) {
    let m = source.len();
    let n = target.len();

    let mut dp = vec![vec![0; n + 1]; m + 1];

    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            if source[i - 1] == target[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            } else {
                dp[i][j] = 1 + dp[i - 1][j].min(dp[i][j - 1]).min(dp[i - 1][j - 1]);
            }
        }
    }

    let mut insertions = 0;
    let mut deletions = 0;
    let mut substitutions = 0;

    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && source[i - 1] == target[j - 1] {
            i -= 1;
            j -= 1;
        } else if i > 0 && j > 0 && dp[i][j] == dp[i - 1][j - 1] + 1 {
            substitutions += 1;
            i -= 1;
            j -= 1;
        } else if j > 0 && dp[i][j] == dp[i][j - 1] + 1 {
            insertions += 1;
            j -= 1;
        } else if i > 0 && dp[i][j] == dp[i - 1][j] + 1 {
            deletions += 1;
            i -= 1;
        } else {
            // Should not happen with Wagner-Fischer
            i = i.saturating_sub(1);
            j = j.saturating_sub(1);
        }
    }

    let mut operations = HashMap::new();
    operations.insert("insertions".to_string(), insertions);
    operations.insert("deletions".to_string(), deletions);
    operations.insert("substitutions".to_string(), substitutions);

    (dp[m][n], operations)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that two identical sequences result in an edit distance of 0 and no edit operations.
    #[test]
    fn test_sequence_distance_identity() {
        let s = vec!["a".to_string(), "b".to_string()];
        let t = vec!["a".to_string(), "b".to_string()];
        let (dist, ops) = compute_sequence_distance(s, t);
        assert_eq!(dist, 0);
        assert_eq!(*ops.get("insertions").unwrap(), 0);
        assert_eq!(*ops.get("deletions").unwrap(), 0);
        assert_eq!(*ops.get("substitutions").unwrap(), 0);
    }

    /// Tests distance calculation when a single token is substituted, expecting exactly 1 substitution operation.
    #[test]
    fn test_sequence_distance_substitution() {
        let s = vec!["a".to_string(), "b".to_string()];
        let t = vec!["a".to_string(), "c".to_string()];
        let (dist, ops) = compute_sequence_distance(s, t);
        assert_eq!(dist, 1);
        assert_eq!(*ops.get("substitutions").unwrap(), 1);
    }

    /// Tests distance calculation when a single token is inserted into the target, expecting 1 insertion operation.
    #[test]
    fn test_sequence_distance_insertion() {
        let s = vec!["a".to_string()];
        let t = vec!["a".to_string(), "b".to_string()];
        let (dist, ops) = compute_sequence_distance(s, t);
        assert_eq!(dist, 1);
        assert_eq!(*ops.get("insertions").unwrap(), 1);
    }

    /// Tests distance calculation when a single token is deleted from the source, expecting 1 deletion operation.
    #[test]
    fn test_sequence_distance_deletion() {
        let s = vec!["a".to_string(), "b".to_string()];
        let t = vec!["a".to_string()];
        let (dist, ops) = compute_sequence_distance(s, t);
        assert_eq!(dist, 1);
        assert_eq!(*ops.get("deletions").unwrap(), 1);
    }

    /// Tests the distance between two token sequences (treating whole words as single tokens).
    #[test]
    fn test_sequence_distance_complex() {
        let s = vec!["kitten".to_string()];
        let t = vec!["sitting".to_string()];
        // Note: compute_sequence_distance works on Vec<String>, not characters.
        // So this is 1 substitution if we treat them as single tokens.
        let (dist, _ops) = compute_sequence_distance(s, t);
        assert_eq!(dist, 1);
    }
}
