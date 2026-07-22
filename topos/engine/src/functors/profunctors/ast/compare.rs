//! Distance module — two structural distance metrics between program
//! ASTs.
//!
//! **1. Tree Edit Distance (TED) — [`calculate_ast_distance`]**
//!
//! In topology, distance measures how "far apart" two points are in a
//! space. For programs, we define distance over their AST structures —
//! two programs are "close" if their syntax trees are similar.
//!
//! This metric is useful for:
//! 1. Detecting code clones (near-zero distance)
//! 2. Measuring refactoring impact (how much structure changed)
//! 3. Comparing LLM outputs to reference implementations
//!
//! We implement a Tree Edit Distance (TED) algorithm that counts the
//! minimum number of node insertions, deletions, and relabelings needed
//! to transform one tree into another. The implementation uses the
//! Wagner-Fischer algorithm on node-type sequences (DFS order), which is
//! an approximation of true structural TED optimized for speed.
//!
//! **2. Gromov-Wasserstein Distance (GHW) — [`calculate_ghw_distance`]**
//!
//! The Gromov-Hausdorff distance measures how far two metric spaces are
//! from being isometric — that is, how much distortion is needed to
//! embed one space into the other. The Wasserstein (optimal transport)
//! variant equips each space with a probability measure and finds the
//! coupling that minimizes expected pairwise distance distortion.
//!
//! For ASTs this captures structural topology that TED misses: two
//! programs with identical node-type sequences but different tree
//! shapes will have low TED but high GHW distance.
//!
//! Algorithm (Frank-Wolfe with Sinkhorn projection):
//! 1. Model each AST as a metric measure space `(X, d_X, μ_X)`:
//!    - `X` = set of AST nodes
//!    - `d_X(u, v)` = number of edges on the unique tree path `u → v`
//!    - `μ_X` = uniform probability measure over `X`
//! 2. Find the coupling `T ∈ Π(μ_X, μ_Y)` minimizing the GW cost:
//!    `GW = Σ_{i,j,k,l} (d_X(i,k) − d_Y(j,l))² T[i,j] T[k,l]`
//! 3. Iterate: compute gradient `M`, Sinkhorn-project to update `T`.
//! 4. Normalize by the sum of within-tree second moments to yield `[0, 1]`.
//!
//! # Deviation from the Python original
//!
//! - The Python original delegates the Wagner-Fischer sequence-distance
//!   engine ([`compute_sequence_distance`]) to a pre-existing
//!   `topos-pyo3` extension (`topos.topos_functors.compute_sequence_distance`,
//!   originally `crates/topos-pyo3/src/profunctors.rs`); that engine is
//!   relocated here verbatim (stripped of `pyo3`), per the "relocate,
//!   don't re-derive" convention. `crates/topos-pyo3/src/profunctors.rs`
//!   itself is left untouched — rewiring the Python extension to call
//!   into `topos-core` instead of duplicating the algorithm is out of
//!   this port's scope (issue #145 is `topos-core` only).
//! - `numpy` has no Rust equivalent in this crate (pure Rust, no
//!   linear-algebra dependency was pulled in for one function); the GW
//!   matrix operations ([`sinkhorn`], [`gromov_wasserstein`]) are
//!   implemented directly over `Vec<Vec<f64>>` instead. Node counts are
//!   capped by `GhwOptions::max_nodes` (default 200), so this stays
//!   `O(n²)`-ish and doesn't need a real linear-algebra crate.
//! - Python's `calculate_ghw_distance(..., return_coupling: bool = False)`
//!   optionally attaches the coupling matrix `T` as a dynamic
//!   `result.coupling` attribute. No caller in the ported test suite
//!   requests it, and Rust has no equivalent of bolting an optional
//!   attribute onto a struct after construction; add a `coupling: Option<Vec<Vec<f64>>>`
//!   field if a caller needs it.

use std::collections::{HashMap, VecDeque};
use std::fmt;

use tree_sitter::Node;

use crate::core::morphism::ProgramMorphism;
use crate::core::object::ProgramObject;

// Tree Edit Distance

/// The result of computing AST edit distance.
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceResult {
    /// The absolute edit distance (number of operations).
    pub raw_distance: usize,
    /// Distance normalized by tree sizes (0-1).
    pub normalized_distance: f64,
    /// Breakdown of edit operations (`"insertions"`, `"deletions"`,
    /// `"substitutions"`).
    pub operations: HashMap<String, usize>,
}

impl fmt::Display for DistanceResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Distance: {} (normalized: {:.3})",
            self.raw_distance, self.normalized_distance
        )
    }
}

/// Compute the tree edit distance between two program ASTs.
///
/// Uses a simplified tree edit distance algorithm based on node type
/// comparison and structural alignment.
///
/// This is an approximation of true tree edit distance, optimized for
/// speed over exactness. For small trees, it's quite accurate; for large
/// trees, it provides a reasonable upper bound.
pub fn calculate_ast_distance(source: &ProgramObject, target: &ProgramObject) -> DistanceResult {
    let source_nodes = source.traverse();
    let target_nodes = target.traverse();

    let source_types: Vec<String> = source_nodes.iter().map(|n| n.kind().to_string()).collect();
    let target_types: Vec<String> = target_nodes.iter().map(|n| n.kind().to_string()).collect();

    let (distance, operations) = compute_sequence_distance(&source_types, &target_types);

    let max_size = source_nodes.len().max(target_nodes.len()).max(1) as f64;
    let normalized = (distance as f64 / max_size).min(1.0);

    DistanceResult {
        raw_distance: distance,
        normalized_distance: normalized,
        operations,
    }
}

/// Compute edit distance between two sequences of node types.
///
/// Uses the Wagner-Fischer algorithm (dynamic programming). Relocated
/// verbatim from `crates/topos-pyo3/src/profunctors.rs`'s
/// `compute_sequence_distance` — see this module's "Deviation from the
/// Python original" note.
pub(crate) fn compute_sequence_distance(
    source: &[String],
    target: &[String],
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

    let operations = count_edit_operations(&dp, source, target, m, n);

    (dp[m][n], operations)
}

/// One step of the Wagner-Fischer backtrace at cell `(i, j)`.
enum EditStep {
    Match,
    Substitute,
    Insert,
    Delete,
    Unreachable,
}

/// Classify which edit produced `dp[i][j]`, by re-deriving which
/// predecessor cell the fill step at `(i, j)` must have used.
fn classify_edit_step(
    dp: &[Vec<usize>],
    source: &[String],
    target: &[String],
    i: usize,
    j: usize,
) -> EditStep {
    if i > 0 && j > 0 && source[i - 1] == target[j - 1] {
        EditStep::Match
    } else if i > 0 && j > 0 && dp[i][j] == dp[i - 1][j - 1] + 1 {
        EditStep::Substitute
    } else if j > 0 && dp[i][j] == dp[i][j - 1] + 1 {
        EditStep::Insert
    } else if i > 0 && dp[i][j] == dp[i - 1][j] + 1 {
        EditStep::Delete
    } else {
        EditStep::Unreachable
    }
}

/// Backtrace the Wagner-Fischer DP table and tally edit operations.
fn count_edit_operations(
    dp: &[Vec<usize>],
    source: &[String],
    target: &[String],
    m: usize,
    n: usize,
) -> HashMap<String, usize> {
    let mut insertions = 0;
    let mut deletions = 0;
    let mut substitutions = 0;

    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        match classify_edit_step(dp, source, target, i, j) {
            EditStep::Match => {
                i -= 1;
                j -= 1;
            }
            EditStep::Substitute => {
                substitutions += 1;
                i -= 1;
                j -= 1;
            }
            EditStep::Insert => {
                insertions += 1;
                j -= 1;
            }
            EditStep::Delete => {
                deletions += 1;
                i -= 1;
            }
            EditStep::Unreachable => {
                // Should not happen with Wagner-Fischer.
                i = i.saturating_sub(1);
                j = j.saturating_sub(1);
            }
        }
    }

    let mut operations = HashMap::new();
    operations.insert("insertions".to_string(), insertions);
    operations.insert("deletions".to_string(), deletions);
    operations.insert("substitutions".to_string(), substitutions);
    operations
}

/// Compute structural similarity between two programs.
///
/// Similarity = `1 - normalized_distance`.
pub fn calculate_similarity(source: &ProgramObject, target: &ProgramObject) -> f64 {
    1.0 - calculate_ast_distance(source, target).normalized_distance
}

/// Normalized AST edit distance between two program morphisms.
///
/// Convenience wrapper around [`calculate_ast_distance`]: extracts the
/// AST from each morphism and returns the normalized result in `[0,
/// 1]`. Returns `1.0` if either morphism is unparseable.
pub fn structural_distance(source: &ProgramMorphism, target: &ProgramMorphism) -> f64 {
    match (&source.ast, &target.ast) {
        (Some(s), Some(t)) => calculate_ast_distance(s, t).normalized_distance,
        _ => 1.0,
    }
}

// Gromov-Wasserstein Tree Distance

/// The result of computing Gromov-Wasserstein distance between two
/// ASTs.
#[derive(Debug, Clone, PartialEq)]
pub struct GHWDistanceResult {
    /// Normalized GW cost in `[0, 1]`. Zero means the trees are
    /// isometric under the uniform measure; one means no structural
    /// correspondence was found.
    pub gw_distance: f64,
    /// Unnormalized GW cost, useful for comparisons at a fixed scale
    /// (e.g. when both trees have the same number of nodes).
    pub raw_gw_cost: f64,
    /// Number of nodes used from the source tree (after any
    /// subsampling).
    pub n_nodes_source: usize,
    /// Number of nodes used from the target tree.
    pub n_nodes_target: usize,
    /// Number of outer GW iterations executed.
    pub n_iterations: usize,
    /// True if the coupling change fell below `tol` before `n_iter` was
    /// exhausted.
    pub converged: bool,
}

impl fmt::Display for GHWDistanceResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.converged {
            "converged"
        } else {
            "max_iter"
        };
        write!(
            f,
            "GHW Distance: {:.4} (raw: {:.4}, {} in {} iter)",
            self.gw_distance, self.raw_gw_cost, status, self.n_iterations
        )
    }
}

/// Options for [`calculate_ghw_distance`] — Rust's answer to Python's
/// keyword-argument defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GhwOptions {
    /// Subsampling cap. If a tree has more than `max_nodes` nodes, the
    /// first `max_nodes` in DFS order are used. Structural (non-leaf)
    /// nodes appear early in DFS, so this retains the most
    /// topologically informative portion of the tree.
    pub max_nodes: usize,
    /// Maximum outer GW iterations (Frank-Wolfe steps).
    pub n_iter: usize,
    /// Sinkhorn entropy regularization. Smaller values yield sharper
    /// couplings but may require more Sinkhorn iterations to converge.
    pub reg: f64,
    /// Convergence tolerance on `‖T_new − T‖_F`. Iteration stops early
    /// when the coupling update is below this threshold.
    pub tol: f64,
}

impl Default for GhwOptions {
    fn default() -> Self {
        GhwOptions {
            max_nodes: 200,
            n_iter: 50,
            reg: 0.5,
            tol: 1e-6,
        }
    }
}

/// Build the pairwise tree-path distance matrix for a list of AST
/// nodes.
///
/// Constructs a bidirectional adjacency list from parent→child edges
/// (using [`tree_sitter::Node::id`] for identity — the Rust equivalent
/// of Python's `id(node)`), then runs BFS from every node to fill an
/// `(n×n)` distance matrix. Only edges whose endpoints both appear in
/// `nodes` are included, so subsampled lists work correctly.
fn tree_path_distances(nodes: &[Node]) -> Vec<Vec<f64>> {
    let n = nodes.len();
    let id_to_idx: HashMap<usize, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id(), i))
        .collect();

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, node) in nodes.iter().enumerate() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(&j) = id_to_idx.get(&child.id()) {
                adj[i].push(j);
                adj[j].push(i);
            }
        }
    }

    let mut dist = vec![vec![(n + 1) as f64; n]; n];
    for (i, row) in dist.iter_mut().enumerate() {
        row[i] = 0.0;
    }

    for start in 0..n {
        let mut visited = vec![false; n];
        visited[start] = true;
        let mut queue: VecDeque<usize> = VecDeque::from([start]);
        while let Some(u) = queue.pop_front() {
            for &v in &adj[u] {
                if !visited[v] {
                    visited[v] = true;
                    dist[start][v] = dist[start][u] + 1.0;
                    queue.push_back(v);
                }
            }
        }
    }

    dist
}

/// Sinkhorn-Knopp scaling to project a Gibbs kernel into `Π(μ, ν)`.
///
/// Iterates row and column normalization until the doubly-stochastic
/// constraint (row marginals = `μ`, column marginals = `ν`) is
/// satisfied.
fn sinkhorn(kernel: &[Vec<f64>], mu: &[f64], nu: &[f64], n_iter: usize) -> Vec<Vec<f64>> {
    const EPS: f64 = 1e-300;
    let n = mu.len();
    let m = nu.len();

    let mut u = vec![1.0_f64; n];
    let mut v = vec![0.0_f64; m];

    for _ in 0..n_iter {
        let ktu = mat_t_vec(kernel, &u);
        v = (0..m).map(|j| nu[j] / ktu[j].max(EPS)).collect();
        let kv = mat_vec(kernel, &v);
        u = (0..n).map(|i| mu[i] / kv[i].max(EPS)).collect();
    }
    let ktu = mat_t_vec(kernel, &u);
    v = (0..m).map(|j| nu[j] / ktu[j].max(EPS)).collect();

    (0..n)
        .map(|i| (0..m).map(|j| u[i] * kernel[i][j] * v[j]).collect())
        .collect()
}

/// Frank-Wolfe iterations for the entropic Gromov-Wasserstein problem.
///
/// At each step linearizes the GW cost around the current coupling `T`,
/// then uses Sinkhorn to solve the resulting linear transport
/// subproblem.
///
/// GW gradient (w.r.t. `T`, given marginals `μ, ν`):
/// `M[i,j] = 2 * ((D1²@μ)[i] + (D2²@ν)[j] − 2*(D1@T@D2)[i,j])`
///
/// The Gibbs kernel uses a shift for numerical stability:
/// `K = exp(−(M − min(M)) / reg)`. Since Sinkhorn is scale-invariant,
/// this is equivalent to the unshifted kernel but avoids float
/// underflow when `M` entries are large relative to `reg`.
fn gromov_wasserstein(
    d1: &[Vec<f64>],
    d2: &[Vec<f64>],
    n_iter: usize,
    reg: f64,
    tol: f64,
) -> (Vec<Vec<f64>>, usize, bool) {
    let n = d1.len();
    let m = d2.len();
    let mu = vec![1.0 / n as f64; n];
    let nu = vec![1.0 / m as f64; m];

    let mut t = outer(&mu, &nu);

    // Constant across iterations.
    let d1sq_mu: Vec<f64> = (0..n)
        .map(|i| (0..n).map(|k| d1[i][k].powi(2) * mu[k]).sum())
        .collect();
    let d2sq_nu: Vec<f64> = (0..m)
        .map(|j| (0..m).map(|l| d2[j][l].powi(2) * nu[l]).sum())
        .collect();

    let mut converged = false;
    let mut iterations_run = 0;

    for it in 1..=n_iter {
        iterations_run = it;
        let d1t = matmul(d1, &t); // n x m
        let cross = matmul(&d1t, d2); // n x m

        let mut m_mat = vec![vec![0.0_f64; m]; n];
        let mut m_min = f64::INFINITY;
        for i in 0..n {
            for j in 0..m {
                let val = 2.0 * (d1sq_mu[i] + d2sq_nu[j] - 2.0 * cross[i][j]);
                m_mat[i][j] = val;
                if val < m_min {
                    m_min = val;
                }
            }
        }
        let kernel: Vec<Vec<f64>> = m_mat
            .iter()
            .map(|row| row.iter().map(|&v| (-(v - m_min) / reg).exp()).collect())
            .collect();
        let t_new = sinkhorn(&kernel, &mu, &nu, 100);

        let mut delta_sq = 0.0;
        for i in 0..n {
            for j in 0..m {
                let d = t_new[i][j] - t[i][j];
                delta_sq += d * d;
            }
        }
        t = t_new;
        if delta_sq.sqrt() < tol {
            converged = true;
            break;
        }
    }

    (t, iterations_run, converged)
}

/// Compute the Gromov-Wasserstein distance between two program ASTs.
///
/// Models each AST as a metric measure space: nodes as points,
/// tree-path length (edge hops) as the metric, and a uniform
/// probability measure over nodes. Finds the optimal coupling `T ∈
/// Π(μ, ν)` that minimizes the GW cost:
///
/// `GW = Σ_{i,j,k,l} (d_X(i,k) − d_Y(j,l))² T[i,j] T[k,l]`
///
/// via a Frank-Wolfe loop with entropic (Sinkhorn) projection.
///
/// **When to use this over [`calculate_ast_distance`]**: the TED
/// implementation compares sequences of node types and ignores tree
/// topology. Two programs with identical node-type multisets but
/// different nesting structure will score near-zero TED but high GHW
/// distance. Use GHW when structural shape — depth, branching, subtree
/// distribution — matters.
///
/// **Normalization**: `gw_distance = raw_gw_cost / (μᵀD1²μ + νᵀD2²ν)`.
/// The denominator is the maximum possible GW cost (when the
/// cross-term vanishes), so `gw_distance` lies in `[0, 1]` with `0`
/// meaning the trees are metrically isometric under the uniform
/// measure.
pub fn calculate_ghw_distance(
    source: &ProgramObject,
    target: &ProgramObject,
    options: GhwOptions,
) -> GHWDistanceResult {
    let mut source_nodes = source.traverse();
    let mut target_nodes = target.traverse();
    source_nodes.truncate(options.max_nodes);
    target_nodes.truncate(options.max_nodes);

    let d1 = tree_path_distances(&source_nodes);
    let d2 = tree_path_distances(&target_nodes);

    let (t, n_iterations, converged) =
        gromov_wasserstein(&d1, &d2, options.n_iter, options.reg, options.tol);

    let n = source_nodes.len();
    let m = target_nodes.len();
    let mu = vec![1.0 / n as f64; n];
    let nu = vec![1.0 / m as f64; m];

    // GW cost via trace identity — avoids building the O(n²m²) tensor:
    //   GW = μᵀD1²μ + νᵀD2²ν − 2·tr(D2·TᵀD1T)
    let tt_d1 = matmul(&transpose(&t), &d1); // m x n
    let tt_d1_t = matmul(&tt_d1, &t); // m x m
    let mut cross = 0.0;
    for i in 0..m {
        for j in 0..m {
            cross += d2[i][j] * tt_d1_t[i][j];
        }
    }

    let self_term = {
        let s1: f64 = (0..n)
            .map(|i| {
                (0..n)
                    .map(|k| mu[i] * d1[i][k].powi(2) * mu[k])
                    .sum::<f64>()
            })
            .sum();
        let s2: f64 = (0..m)
            .map(|i| {
                (0..m)
                    .map(|k| nu[i] * d2[i][k].powi(2) * nu[k])
                    .sum::<f64>()
            })
            .sum();
        s1 + s2
    };

    let raw_gw_cost = self_term - 2.0 * cross;
    let gw_distance = if self_term > 0.0 {
        (raw_gw_cost / self_term).clamp(0.0, 1.0)
    } else {
        0.0
    };

    GHWDistanceResult {
        gw_distance,
        raw_gw_cost,
        n_nodes_source: n,
        n_nodes_target: m,
        n_iterations,
        converged,
    }
}

fn outer(a: &[f64], b: &[f64]) -> Vec<Vec<f64>> {
    a.iter()
        .map(|&x| b.iter().map(|&y| x * y).collect())
        .collect()
}

/// `a @ b` for dense `Vec<Vec<f64>>` matrices.
fn matmul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let k = b.len();
    let m = if k > 0 { b[0].len() } else { 0 };
    let mut out = vec![vec![0.0_f64; m]; n];
    for i in 0..n {
        for kk in 0..k {
            let aik = a[i][kk];
            if aik == 0.0 {
                continue;
            }
            for j in 0..m {
                out[i][j] += aik * b[kk][j];
            }
        }
    }
    out
}

/// `a @ v` for a dense matrix and a column vector.
fn mat_vec(a: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    a.iter()
        .map(|row| row.iter().zip(v).map(|(x, y)| x * y).sum())
        .collect()
}

/// `a.T @ v`, without materializing the transpose.
fn mat_t_vec(a: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    let cols = a.first().map_or(0, Vec::len);
    let mut out = vec![0.0_f64; cols];
    for (row, &vi) in a.iter().zip(v) {
        for (j, &aij) in row.iter().enumerate() {
            out[j] += aij * vi;
        }
    }
    out
}

fn transpose(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    if a.is_empty() {
        return Vec::new();
    }
    let rows = a.len();
    let cols = a[0].len();
    let mut out = vec![vec![0.0_f64; rows]; cols];
    for (i, row) in a.iter().enumerate() {
        for (j, &val) in row.iter().enumerate() {
            out[j][i] = val;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn build(source: &str) -> ProgramObject {
        let result = parse_source(source, "python", None).expect("parse should not fail");
        ProgramObject::new(
            result.tree,
            result.source,
            result.language,
            result.uast_root,
        )
    }

    #[test]
    fn distance_metrics() {
        let ast1 = build("x = 1");
        let ast2 = build("y = 2");
        let ast3 = build("def foo():\n    pass");

        let dist1_2 = calculate_ast_distance(&ast1, &ast2);
        let dist1_3 = calculate_ast_distance(&ast1, &ast3);
        assert!(dist1_3.raw_distance > dist1_2.raw_distance);

        let sim = calculate_similarity(&ast1, &ast1);
        assert_eq!(sim, 1.0);
    }

    #[test]
    fn distance_result_display() {
        let res = DistanceResult {
            raw_distance: 2,
            normalized_distance: 0.5,
            operations: HashMap::new(),
        };
        assert!(res.to_string().contains("Distance:"));
    }

    #[test]
    fn distance_substitution() {
        let ast1 = build("x = 1");
        let ast2 = build("def foo(): pass");
        let dist = calculate_ast_distance(&ast1, &ast2);
        assert!(*dist.operations.get("substitutions").unwrap_or(&0) <= dist.raw_distance);
    }

    #[test]
    fn structural_distance_returns_one_for_unparseable() {
        let a = ProgramMorphism::new("x = 1", "python");
        let b = ProgramMorphism::new("PROGRAM. HELLO.", "cobol");
        assert_eq!(structural_distance(&a, &b), 1.0);
    }

    #[test]
    fn structural_distance_zero_for_identical() {
        let a = ProgramMorphism::new("x = 1", "python");
        let b = ProgramMorphism::new("x = 1", "python");
        assert_eq!(structural_distance(&a, &b), 0.0);
    }

    // Gromov-Wasserstein distance tests

    /// Same tree compared to itself should give distance ≈ 0.
    #[test]
    fn ghw_identity() {
        let source = "def foo(x):\n    if x > 0:\n        return x\n    return -x";
        let ast = build(source);
        let result = calculate_ghw_distance(&ast, &ast, GhwOptions::default());
        assert!(result.gw_distance < 0.05);
        assert_eq!(result.n_nodes_source, result.n_nodes_target);
    }

    /// Structurally different programs should have a non-trivial GHW
    /// distance.
    #[test]
    fn ghw_divergent() {
        let simple = build("x = 1");
        let complex_src = build(
            "class Foo:\n    def bar(self, x, y):\n        for i in range(x):\n            if i % 2 == 0:\n                yield i * y\n    def baz(self):\n        return [self.bar(i, i) for i in range(10)]",
        );
        let result = calculate_ghw_distance(&simple, &complex_src, GhwOptions::default());
        assert!(result.gw_distance > 0.2);
    }

    /// GHW distance should be approximately symmetric.
    #[test]
    fn ghw_symmetry() {
        let a = build("def foo(x):\n    return x + 1");
        let b = build("for i in range(10):\n    print(i)\nx = sum(range(5))");
        let d_ab = calculate_ghw_distance(&a, &b, GhwOptions::default()).gw_distance;
        let d_ba = calculate_ghw_distance(&b, &a, GhwOptions::default()).gw_distance;
        assert!((d_ab - d_ba).abs() < 0.05);
    }

    /// Trees exceeding `max_nodes` should be capped at `max_nodes`.
    #[test]
    fn ghw_subsampling() {
        let mut lines = vec!["def foo():".to_string(), "    x = 0".to_string()];
        for i in 0..40 {
            lines.push(format!("    x = x + {i}"));
        }
        lines.push("    return x".to_string());
        let ast = build(&lines.join("\n"));

        let options = GhwOptions {
            max_nodes: 30,
            ..GhwOptions::default()
        };
        let result = calculate_ghw_distance(&ast, &ast, options);
        assert!(result.n_nodes_source <= 30);
        assert!(result.n_nodes_target <= 30);
    }

    /// Typical small programs should converge within the iteration
    /// budget.
    #[test]
    fn ghw_converged() {
        let ast = build("def add(a, b):\n    return a + b");
        let result = calculate_ghw_distance(&ast, &ast, GhwOptions::default());
        assert!(result.converged);
    }

    /// `GHWDistanceResult`'s `Display` should mention "GHW Distance".
    #[test]
    fn ghw_result_display() {
        let res = GHWDistanceResult {
            gw_distance: 0.42,
            raw_gw_cost: 1.23,
            n_nodes_source: 10,
            n_nodes_target: 12,
            n_iterations: 7,
            converged: true,
        };
        let s = res.to_string();
        assert!(s.contains("GHW Distance"));
        assert!(s.contains("converged"));
    }
}
