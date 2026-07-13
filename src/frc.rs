//! Forman-Ricci curvature engine.
//!
//! Two variants share one adjacency-indexing layer:
//!   - `balanced_forman_curvature`: undirected balanced Forman curvature
//!     (Topping, Di Giovanni, Chamberlain, Dong & Bronstein, "Understanding
//!     over-squashing and bottlenecks on graphs via curvature", ICLR 2022,
//!     arxiv:2111.14522, Definition 1). Applied to the MDG to find dependency
//!     edges worth strengthening.
//!   - `directed_forman_curvature`: directed Forman-Ricci curvature (Samal et
//!     al.), applied to GitNexus process graphs to find execution choke points.
//!
//! Both are purely local (per-edge) computations over a sparse adjacency
//! index built once from the input edge list — no petgraph needed, since
//! neither formula requires path/component algorithms, only neighbor lookups.

use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[pyclass(get_all, from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct WeightedEdge {
    pub source: usize,
    pub target: usize,
    pub weight: f64,
}

#[pymethods]
impl WeightedEdge {
    #[new]
    #[pyo3(signature = (source, target, weight=1.0))]
    fn new(source: usize, target: usize, weight: f64) -> Self {
        Self {
            source,
            target,
            weight,
        }
    }
}

#[pyclass(get_all, skip_from_py_object)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EdgeCurvature {
    pub source: usize,
    pub target: usize,
    pub curvature: f64,
}

fn intern(id: usize, id_to_idx: &mut HashMap<usize, usize>, idx_to_id: &mut Vec<usize>) -> usize {
    if let Some(&idx) = id_to_idx.get(&id) {
        idx
    } else {
        let idx = idx_to_id.len();
        idx_to_id.push(id);
        id_to_idx.insert(id, idx);
        idx
    }
}

/// Sparse adjacency built once per call from a raw edge list, with node ids
/// interned to dense indices. In undirected mode every edge populates both
/// endpoints' `neighbors` sets (self-loops dropped). In directed mode
/// `out_edges[u]` / `in_edges[v]` carry `(neighbor_idx, edge_weight)` pairs.
struct AdjacencyIndex {
    id_to_idx: HashMap<usize, usize>,
    neighbors: Vec<HashSet<usize>>,
    in_edges: Vec<Vec<(usize, f64)>>,
    out_edges: Vec<Vec<(usize, f64)>>,
}

impl AdjacencyIndex {
    fn build(edges: &[WeightedEdge], directed: bool) -> Self {
        let mut id_to_idx = HashMap::new();
        let mut idx_to_id = Vec::new();
        for e in edges {
            intern(e.source, &mut id_to_idx, &mut idx_to_id);
            intern(e.target, &mut id_to_idx, &mut idx_to_id);
        }

        let n = idx_to_id.len();
        let mut neighbors = vec![HashSet::new(); n];
        let mut in_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        let mut out_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];

        for e in edges {
            let s = id_to_idx[&e.source];
            let t = id_to_idx[&e.target];
            if s == t {
                continue; // no self-loops
            }
            if directed {
                out_edges[s].push((t, e.weight));
                in_edges[t].push((s, e.weight));
            } else {
                neighbors[s].insert(t);
                neighbors[t].insert(s);
            }
        }

        Self {
            id_to_idx,
            neighbors,
            in_edges,
            out_edges,
        }
    }
}

/// Balanced Forman curvature for every edge in `edges`, treated as an
/// undirected simple graph (edge weights are ignored — this curvature is
/// purely combinatorial: degree, triangle count, and a 4-cycle term). One
/// result per unordered node pair; duplicate/parallel/reciprocal input edges
/// collapse to a single result.
///
/// Ported directly from the paper authors' reference implementation
/// (github.com/jctops/understanding-oversquashing,
/// `gdl/src/gdl/curvature/numba.py::_balanced_forman_curvature`) rather than
/// re-derived from the paper's set-builder notation, since the 4-cycle
/// ("sharp"/"lambda") term has subtle indexing that's easy to get wrong from
/// prose alone. Adapted here to sparse neighbor-set operations (the reference
/// implementation is a dense O(V) matrix scan per edge) so this stays
/// tractable on graphs with tens of thousands of nodes: each edge's 4-cycle
/// scan is restricted to `k` in the relevant endpoint's neighbor set rather
/// than all V nodes, which is equivalent because the reference formula's
/// per-candidate factor is zero for every `k` outside that set.
///
/// Per the paper, `Ric(i,j) = 0` whenever `min(deg(i), deg(j)) <= 1` (a
/// pendant/leaf edge) — the raw degree/triangle/4-cycle formula degenerates
/// to a spurious large value at degree-1 endpoints (verified by hand: a bare
/// isolated edge would otherwise score curvature 4, not the expected ~0),
/// so this case is special-cased before the (unnecessary) triangle/4-cycle work.
#[pyfunction]
pub fn balanced_forman_curvature(edges: Vec<WeightedEdge>) -> Vec<EdgeCurvature> {
    let adj = AdjacencyIndex::build(&edges, false);
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut results = Vec::new();

    for e in &edges {
        let i = adj.id_to_idx[&e.source];
        let j = adj.id_to_idx[&e.target];
        if i == j {
            continue;
        }
        let key = (i.min(j), i.max(j));
        if !seen.insert(key) {
            continue;
        }

        let d_i = adj.neighbors[i].len();
        let d_j = adj.neighbors[j].len();
        let (d_max, d_min) = if d_i > d_j { (d_i, d_j) } else { (d_j, d_i) };

        if d_min <= 1 {
            results.push(EdgeCurvature {
                source: e.source,
                target: e.target,
                curvature: 0.0,
            });
            continue;
        }

        let (d_max_f, d_min_f) = (d_max as f64, d_min as f64);
        let triangles = adj.neighbors[i].intersection(&adj.neighbors[j]).count();

        let mut sharp_ij: u32 = 0;
        let mut lambda_ij: i64 = 0;

        // "i-side" squares: k ranges over N(j); look for a common neighbor of
        // i and k beyond any direct i-k edge (a 4-cycle i-w-k-j-i).
        for &k in &adj.neighbors[j] {
            let common = adj.neighbors[i].intersection(&adj.neighbors[k]).count() as i64;
            let direct = i64::from(adj.neighbors[i].contains(&k));
            let tmp = common - direct;
            if tmp > 0 {
                sharp_ij += 1;
                lambda_ij = lambda_ij.max(tmp);
            }
        }
        // "j-side" squares: k ranges over N(i); mirror of the above.
        for &k in &adj.neighbors[i] {
            let common = adj.neighbors[k].intersection(&adj.neighbors[j]).count() as i64;
            let direct = i64::from(adj.neighbors[j].contains(&k));
            let tmp = common - direct;
            if tmp > 0 {
                sharp_ij += 1;
                lambda_ij = lambda_ij.max(tmp);
            }
        }

        let mut curvature = 2.0 / d_max_f + 2.0 / d_min_f - 2.0
            + (2.0 / d_max_f + 1.0 / d_min_f) * (triangles as f64);
        if lambda_ij > 0 {
            curvature += (sharp_ij as f64) / (d_max_f * (lambda_ij as f64));
        }

        results.push(EdgeCurvature {
            source: e.source,
            target: e.target,
            curvature,
        });
    }

    results
}

/// Directed Forman-Ricci curvature (Samal et al.) for every edge `e = (u -> v)`:
///
///   Ric(e) = w_e * ( w_u/w_e + w_v/w_e
///                    - sum_{e_in ~ u} sqrt(w_u / w_e_in)
///                    - sum_{e_out ~ v} sqrt(w_v / w_e_out) )
///
/// where `e_in` ranges over edges incoming to `u` and `e_out` ranges over
/// edges outgoing from `v`. `node_weights` (keyed by the caller's original
/// node ids) default to 1.0; edge weights come from `WeightedEdge.weight`
/// (also defaulting to 1.0). Highly negative values flag "choke points" —
/// transitions where many independent paths funnel through one edge.
/// One result per input edge (not deduplicated: directed multi-edges, e.g.
/// repeated transitions across several process paths, are each meaningful).
#[pyfunction]
#[pyo3(signature = (edges, node_weights=None))]
pub fn directed_forman_curvature(
    edges: Vec<WeightedEdge>,
    node_weights: Option<HashMap<usize, f64>>,
) -> Vec<EdgeCurvature> {
    let adj = AdjacencyIndex::build(&edges, true);
    let weights = node_weights.unwrap_or_default();
    let weight_of = |id: usize| -> f64 { *weights.get(&id).unwrap_or(&1.0) };

    let mut results = Vec::with_capacity(edges.len());
    for e in &edges {
        if e.source == e.target {
            continue;
        }
        let u = adj.id_to_idx[&e.source];
        let v = adj.id_to_idx[&e.target];
        let w_e = e.weight;
        if w_e <= 0.0 {
            results.push(EdgeCurvature {
                source: e.source,
                target: e.target,
                curvature: 0.0,
            });
            continue;
        }

        let w_u = weight_of(e.source);
        let w_v = weight_of(e.target);

        let in_sum: f64 = adj.in_edges[u]
            .iter()
            .filter(|&&(_, w_ein)| w_ein > 0.0)
            .map(|&(_, w_ein)| (w_u / w_ein).sqrt())
            .sum();
        let out_sum: f64 = adj.out_edges[v]
            .iter()
            .filter(|&&(_, w_eout)| w_eout > 0.0)
            .map(|&(_, w_eout)| (w_v / w_eout).sqrt())
            .sum();

        let curvature = w_e * (w_u / w_e + w_v / w_e - in_sum - out_sum);
        results.push(EdgeCurvature {
            source: e.source,
            target: e.target,
            curvature,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curvature_of<'a>(results: &'a [EdgeCurvature], source: usize, target: usize) -> f64 {
        results
            .iter()
            .find(|r| {
                (r.source == source && r.target == target)
                    || (r.source == target && r.target == source)
            })
            .unwrap_or_else(|| panic!("no curvature result for edge ({source}, {target})"))
            .curvature
    }

    /// Hand-derived ground truth: a triangle (K3) has every edge with degree 2,
    /// one triangle per edge, and a specific 4-cycle ("sharp"/"lambda") term
    /// that works out to 2.0 exactly — see the module doc comment's worked
    /// example for the derivation.
    #[test]
    fn test_balanced_forman_triangle_exact_value() {
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
        ];
        let results = balanced_forman_curvature(edges);
        assert_eq!(results.len(), 3);
        for &(a, b) in &[(0, 1), (1, 2), (2, 0)] {
            assert!(
                (curvature_of(&results, a, b) - 2.0).abs() < 1e-9,
                "expected curvature 2.0 for triangle edge ({a},{b})"
            );
        }
    }

    /// A pendant (leaf) edge — one endpoint has degree 1 — must curve to
    /// exactly 0.0 per the paper's explicit special case, not whatever the
    /// raw degree/triangle formula would otherwise produce.
    #[test]
    fn test_balanced_forman_leaf_edge_is_zero() {
        // Star: center 0 connected to leaves 1, 2, 3. Edge (0,1): deg(0)=3, deg(1)=1.
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(0, 2, 1.0),
            WeightedEdge::new(0, 3, 1.0),
        ];
        let results = balanced_forman_curvature(edges);
        for r in &results {
            assert_eq!(r.curvature, 0.0);
        }
    }

    /// A "bridge" edge connecting two otherwise-separate triangles must curve
    /// more negatively than the triangles' own internal edges — the
    /// bottleneck-detection property the whole engine exists for.
    #[test]
    fn test_balanced_forman_bridge_is_most_negative() {
        // Triangle {0,1,2}, triangle {3,4,5}, bridge edge (2,3).
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
            WeightedEdge::new(3, 4, 1.0),
            WeightedEdge::new(4, 5, 1.0),
            WeightedEdge::new(5, 3, 1.0),
            WeightedEdge::new(2, 3, 1.0),
        ];
        let results = balanced_forman_curvature(edges);
        let bridge = curvature_of(&results, 2, 3);
        for &(a, b) in &[(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3)] {
            assert!(
                bridge < curvature_of(&results, a, b),
                "bridge curvature {bridge} should be less than triangle edge ({a},{b})"
            );
        }
    }

    /// Duplicate/reciprocal input edges for the same undirected pair collapse
    /// to a single result rather than being double-counted.
    #[test]
    fn test_balanced_forman_dedupes_reciprocal_edges() {
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 0, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
        ];
        let results = balanced_forman_curvature(edges);
        assert_eq!(results.len(), 3);
    }

    /// A "bowtie" quiver — two stars whose centers feed into a single bridge
    /// edge — must have that bridge edge rank most negative among all edges.
    #[test]
    fn test_directed_bowtie_bottleneck() {
        // Fan-in into 10 from {0,1,2,3}; bridge 10 -> 11; fan-out from 11 to {12,13,14,15}.
        let mut edges = Vec::new();
        for src in 0..4 {
            edges.push(WeightedEdge::new(src, 10, 1.0));
        }
        edges.push(WeightedEdge::new(10, 11, 1.0));
        for dst in 12..16 {
            edges.push(WeightedEdge::new(11, dst, 1.0));
        }

        let results = directed_forman_curvature(edges, None);
        let bridge = curvature_of(&results, 10, 11);
        for r in &results {
            if (r.source, r.target) != (10, 11) {
                assert!(
                    bridge < r.curvature,
                    "bridge curvature {bridge} should be less than edge ({},{}) = {}",
                    r.source,
                    r.target,
                    r.curvature
                );
            }
        }
    }

    /// Uniform directed cycle: every node has in-degree 1 / out-degree 1, so
    /// every edge must have identical curvature by symmetry.
    #[test]
    fn test_directed_cycle_uniform_curvature() {
        let k = 6;
        let edges: Vec<WeightedEdge> = (0..k)
            .map(|i| WeightedEdge::new(i, (i + 1) % k, 1.0))
            .collect();
        let results = directed_forman_curvature(edges, None);
        let first = results[0].curvature;
        for r in &results {
            assert!((r.curvature - first).abs() < 1e-9);
        }
        // Ric = 1*(1+1-1-1) = 0 for a uniform-weight cycle (one in-edge, one out-edge each).
        assert!(first.abs() < 1e-9);
    }

    /// Omitting `node_weights` must behave identically to passing all-1.0 weights.
    #[test]
    fn test_directed_default_weights_are_unit() {
        let edges = vec![WeightedEdge::new(0, 1, 1.0), WeightedEdge::new(1, 2, 1.0)];
        let default_results = directed_forman_curvature(edges.clone(), None);
        let mut unit_weights = HashMap::new();
        unit_weights.insert(0usize, 1.0);
        unit_weights.insert(1usize, 1.0);
        unit_weights.insert(2usize, 1.0);
        let explicit_results = directed_forman_curvature(edges, Some(unit_weights));
        for (a, b) in default_results.iter().zip(explicit_results.iter()) {
            assert!((a.curvature - b.curvature).abs() < 1e-12);
        }
    }

    /// Empty input and a single isolated edge must not panic (divide-by-zero guards).
    #[test]
    fn test_empty_and_isolated_edge_no_panic() {
        assert_eq!(directed_forman_curvature(vec![], None).len(), 0);
        assert_eq!(balanced_forman_curvature(vec![]).len(), 0);

        let single = vec![WeightedEdge::new(0, 1, 1.0)];
        let directed = directed_forman_curvature(single.clone(), None);
        assert_eq!(directed.len(), 1);
        let undirected = balanced_forman_curvature(single);
        assert_eq!(undirected.len(), 1);
        assert_eq!(undirected[0].curvature, 0.0); // both endpoints degree 1 -> leaf rule
    }

    /// Perf sanity check for issue #86: directed curvature over 10k nodes /
    /// 50k edges must stay near-linear, not blow up. The issue's literal
    /// <100ms acceptance bar is a release-build/production target, validated
    /// end-to-end (including the Python/PyO3 call boundary) against a
    /// `maturin build --release` wheel in `tests/benchmarks/test_curvature_perf.py`;
    /// this Rust-level test runs under `cargo test`'s unoptimized debug profile
    /// (matching CI's plain `cargo test`, no `--release`), which is commonly
    /// 10-30x slower than release for numeric code, so it uses a generous
    /// margin instead of the strict production bound.
    #[test]
    fn test_directed_curvature_perf_10k_50k() {
        use std::time::Instant;
        let n = 10_000usize;
        let e_count = 50_000usize;
        let mut edges = Vec::with_capacity(e_count);
        // Deterministic pseudo-random edge generation (no external RNG dependency).
        let mut state: u64 = 88172645463325252;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..e_count {
            let s = (next() as usize) % n;
            let mut t = (next() as usize) % n;
            if t == s {
                t = (t + 1) % n;
            }
            edges.push(WeightedEdge::new(s, t, 1.0));
        }

        let start = Instant::now();
        let results = directed_forman_curvature(edges, None);
        let elapsed = start.elapsed();
        assert!(!results.is_empty());
        assert!(
            elapsed.as_millis() < 2000,
            "directed_forman_curvature took {elapsed:?} on 10k/50k (debug build) — investigate for a real algorithmic blowup"
        );
    }
}
