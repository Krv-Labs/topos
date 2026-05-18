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

    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
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
            if i > 0 { i -= 1; }
            if j > 0 { j -= 1; }
        }
    }

    let mut operations = HashMap::new();
    operations.insert("insertions".to_string(), insertions);
    operations.insert("deletions".to_string(), deletions);
    operations.insert("substitutions".to_string(), substitutions);

    (dp[m][n], operations)
}
