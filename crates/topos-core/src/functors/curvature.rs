//! Forman-Ricci curvature engine.
//!
//! Two variants share one adjacency-indexing layer:
//!   - [`balanced_forman_curvature`]: undirected balanced Forman curvature
//!     (Topping, Di Giovanni, Chamberlain, Dong & Bronstein, "Understanding
//!     over-squashing and bottlenecks on graphs via curvature", ICLR 2022,
//!     arxiv:2111.14522, Definition 1). Applied to the MDG to find dependency
//!     edges worth strengthening (see
//!     [`crate::functors::probes::mdg::curvature`]).
//!   - [`directed_forman_curvature`]: directed Forman-Ricci curvature (Samal
//!     et al.), applied to GitNexus process graphs to find execution choke
//!     points (see [`crate::functors::probes::process::curvature`]).
//!
//! Both are purely local (per-edge) computations over a sparse adjacency
//! index built once from the input edge list — no petgraph needed, since
//! neither formula requires path/component algorithms, only neighbor lookups.
//!
//! Moved from the former `topos-pyo3` extension crate (`frc.rs`) per PR #159
//! review: computation lives in `topos-core`; bindings crates only bind.

use std::collections::{HashMap, HashSet};

/// A directed edge over caller-interned dense `usize` node ids.
#[derive(Clone, Debug, PartialEq)]
pub struct WeightedEdge {
    pub source: usize,
    pub target: usize,
    pub weight: f64,
}

impl WeightedEdge {
    pub fn new(source: usize, target: usize, weight: f64) -> Self {
        Self {
            source,
            target,
            weight,
        }
    }
}

/// Curvature of one edge, in the caller's original node ids.
#[derive(Clone, Debug, PartialEq)]
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
pub fn balanced_forman_curvature(edges: &[WeightedEdge]) -> Vec<EdgeCurvature> {
    let adj = AdjacencyIndex::build(edges, false);
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut results = Vec::new();

    for e in edges {
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
/// ```text
/// Ric(e) = w_e * ( w_u/w_e + w_v/w_e
///                  - sum_{e_in ~ u} sqrt(w_u / w_e_in)
///                  - sum_{e_out ~ v} sqrt(w_v / w_e_out) )
/// ```
///
/// where `e_in` ranges over edges incoming to `u` and `e_out` ranges over
/// edges outgoing from `v`. `node_weights` (keyed by the caller's original
/// node ids) default to 1.0; edge weights come from `WeightedEdge.weight`
/// (also defaulting to 1.0). Highly negative values flag "choke points" —
/// transitions where many independent paths funnel through one edge.
/// One result per input edge (not deduplicated: directed multi-edges, e.g.
/// repeated transitions across several process paths, are each meaningful).
pub fn directed_forman_curvature(
    edges: &[WeightedEdge],
    node_weights: Option<&HashMap<usize, f64>>,
) -> Vec<EdgeCurvature> {
    let adj = AdjacencyIndex::build(edges, true);
    let weight_of = |id: usize| -> f64 {
        node_weights
            .and_then(|w| w.get(&id).copied())
            .unwrap_or(1.0)
    };

    let mut results = Vec::with_capacity(edges.len());
    for e in edges {
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

    fn curvature_of(results: &[EdgeCurvature], source: usize, target: usize) -> f64 {
        results
            .iter()
            .find(|c| {
                (c.source == source && c.target == target)
                    || (c.source == target && c.target == source)
            })
            .map(|c| c.curvature)
            .expect("edge not found in results")
    }

    /// Triangle: every node has degree 2 and one triangle per edge.
    /// Base term 2/2 + 2/2 - 2 = 0, triangle term (2/2 + 1/2)*1 = 1.5, and
    /// the reference implementation's sharp/lambda pass (which ranges over
    /// k = i and k = j too) adds 2/(2*2) = 0.5 — so exactly 2.0, matching
    /// the paper authors' numba reference on this graph.
    #[test]
    fn balanced_forman_triangle_exact_value() {
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
        ];
        let results = balanced_forman_curvature(&edges);
        assert_eq!(results.len(), 3);
        for c in &results {
            assert!((c.curvature - 2.0).abs() < 1e-9, "got {}", c.curvature);
        }
    }

    /// A pendant edge (degree-1 endpoint) is defined as curvature 0.
    #[test]
    fn balanced_forman_leaf_edge_is_zero() {
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
            WeightedEdge::new(2, 3, 1.0), // pendant
        ];
        let results = balanced_forman_curvature(&edges);
        assert_eq!(curvature_of(&results, 2, 3), 0.0);
    }

    /// A bridge between two triangles is the most negative edge in the graph.
    #[test]
    fn balanced_forman_bridge_is_most_negative() {
        let edges = vec![
            // triangle A: 0-1-2
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
            // triangle B: 3-4-5
            WeightedEdge::new(3, 4, 1.0),
            WeightedEdge::new(4, 5, 1.0),
            WeightedEdge::new(5, 3, 1.0),
            // bridge
            WeightedEdge::new(2, 3, 1.0),
        ];
        let results = balanced_forman_curvature(&edges);
        let bridge = curvature_of(&results, 2, 3);
        for c in &results {
            if (c.source, c.target) != (2, 3) && (c.target, c.source) != (2, 3) {
                assert!(bridge <= c.curvature);
            }
        }
    }

    /// Reciprocal directed edges (a->b, b->a) collapse to one undirected result.
    #[test]
    fn balanced_forman_dedupes_reciprocal_edges() {
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 0, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
        ];
        let results = balanced_forman_curvature(&edges);
        assert_eq!(results.len(), 3);
    }

    /// Bow-tie: 0->2, 1->2, 2->3, 3->4, 3->5. The middle edge 2->3 funnels
    /// two incoming paths into two outgoing paths — most negative curvature.
    #[test]
    fn directed_bowtie_bottleneck() {
        let edges = vec![
            WeightedEdge::new(0, 2, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 3, 1.0),
            WeightedEdge::new(3, 4, 1.0),
            WeightedEdge::new(3, 5, 1.0),
        ];
        let results = directed_forman_curvature(&edges, None);
        let middle = results
            .iter()
            .find(|c| c.source == 2 && c.target == 3)
            .unwrap()
            .curvature;
        for c in &results {
            if !(c.source == 2 && c.target == 3) {
                assert!(middle <= c.curvature, "middle {middle} vs {}", c.curvature);
            }
        }
    }

    /// A directed cycle is degree-regular: every edge gets the same curvature.
    #[test]
    fn directed_cycle_uniform_curvature() {
        let edges = vec![
            WeightedEdge::new(0, 1, 1.0),
            WeightedEdge::new(1, 2, 1.0),
            WeightedEdge::new(2, 0, 1.0),
        ];
        let results = directed_forman_curvature(&edges, None);
        assert_eq!(results.len(), 3);
        let first = results[0].curvature;
        for c in &results {
            assert!((c.curvature - first).abs() < 1e-9);
        }
    }

    /// With unit weights, Ric(u->v) = 2 - in_deg(u) - out_deg(v).
    #[test]
    fn directed_default_weights_are_unit() {
        let edges = vec![WeightedEdge::new(0, 1, 1.0), WeightedEdge::new(1, 2, 1.0)];
        let results = directed_forman_curvature(&edges, None);
        // 0->1: in_deg(0)=0, out_deg(1)=1 → 2 - 0 - 1 = 1
        assert_eq!(results[0].curvature, 1.0);
        // 1->2: in_deg(1)=1, out_deg(2)=0 → 2 - 1 - 0 = 1
        assert_eq!(results[1].curvature, 1.0);
    }

    #[test]
    fn empty_and_isolated_edge_no_panic() {
        assert!(balanced_forman_curvature(&[]).is_empty());
        assert!(directed_forman_curvature(&[], None).is_empty());
        let lone = vec![WeightedEdge::new(7, 9, 1.0)];
        assert_eq!(balanced_forman_curvature(&lone)[0].curvature, 0.0);
        // Directed: in_deg(7)=0, out_deg(9)=0 → Ric = 2.
        assert_eq!(directed_forman_curvature(&lone, None)[0].curvature, 2.0);
    }
}
