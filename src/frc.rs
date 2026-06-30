use pyo3::prelude::*;
use std::collections::HashSet;

/// Balanced Forman-Ricci curvature for one edge.
///
/// Topping et al. "Understanding Over-Squashing and Bottlenecks on Graphs
/// via Curvature" (ICLR 2022).  Unweighted, symmetrized graph.
///
/// Formula (per-edge, unweighted):
///   Ric(i,j) = 2/d_i + 2/d_j − 2 + |𝒯(i,j)| · (1/max(d_i,d_j) + 1/min(d_i,d_j))
///
/// where 𝒯(i,j) = N(i) ∩ N(j) (common neighbors).
///
/// Properties:
///   bridge (no shared neighbors, high degree) → Ric → −2 (most negative)
///   triangle-dense edge                        → Ric positive
///   leaves (d=1)                               → Ric = 2 (edge case guard below)
#[pyclass(get_all)]
pub struct EdgeCurvature {
    pub source_idx: usize,
    pub target_idx: usize,
    /// Balanced Forman-Ricci score (Topping 2022).
    pub ric: f64,
    /// True when ric < BRIDGE_THRESHOLD — advisory, uncalibrated.
    pub is_bridge: bool,
}

/// Edges with both endpoints having degree 1 are isolated pairs; their
/// curvature is undefined (0/0 limit). We emit ric=0.0 for them.
const BRIDGE_THRESHOLD: f64 = -1.0;

/// Compute balanced Forman-Ricci curvature for every undirected edge.
///
/// `edges` should be **deduplicated undirected pairs** — if your source
/// graph is directed, symmetrize before calling (add (v,u) for every
/// (u,v), then dedup).  Self-loops are ignored.
#[pyfunction]
pub fn calculate_balanced_frc(
    n_nodes: usize,
    edges: Vec<(usize, usize)>,
) -> Vec<EdgeCurvature> {
    if n_nodes == 0 || edges.is_empty() {
        return vec![];
    }

    // Build adjacency sets for O(1) neighbour lookup.
    let mut adj: Vec<HashSet<usize>> = vec![HashSet::new(); n_nodes];
    for &(u, v) in &edges {
        if u != v && u < n_nodes && v < n_nodes {
            adj[u].insert(v);
            adj[v].insert(u);
        }
    }

    let mut results = Vec::with_capacity(edges.len());
    for &(u, v) in &edges {
        if u == v || u >= n_nodes || v >= n_nodes {
            continue;
        }
        let d_u = adj[u].len();
        let d_v = adj[v].len();

        // Isolated-node guard (should not occur in a connected MDG, but be safe).
        if d_u == 0 || d_v == 0 {
            results.push(EdgeCurvature {
                source_idx: u,
                target_idx: v,
                ric: 0.0,
                is_bridge: false,
            });
            continue;
        }

        // |𝒯(u,v)| — common neighbours (triangle count for this edge).
        let triangles: usize = adj[u].intersection(&adj[v]).count();

        let d_max = d_u.max(d_v) as f64;
        let d_min = d_u.min(d_v) as f64;

        let ric = 2.0 / d_u as f64
            + 2.0 / d_v as f64
            - 2.0
            + triangles as f64 * (1.0 / d_max + 1.0 / d_min);

        results.push(EdgeCurvature {
            source_idx: u,
            target_idx: v,
            ric,
            is_bridge: ric < BRIDGE_THRESHOLD,
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges_for(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
        pairs.to_vec()
    }

    /// Linear chain A-B-C (n=3, edges undirected).
    /// d_A=1, d_B=2, d_C=1.  No triangles.
    /// Edge (A,B): Ric = 2/1 + 2/2 − 2 = 1.0   (leaf edge, positive)
    /// Edge (B,C): same by symmetry
    #[test]
    fn linear_chain() {
        let results = calculate_balanced_frc(3, edges_for(&[(0, 1), (1, 2)]));
        assert_eq!(results.len(), 2);
        for e in &results {
            let ric = (e.ric - 1.0).abs();
            assert!(ric < 1e-9, "expected 1.0, got {}", e.ric);
            assert!(!e.is_bridge);
        }
    }

    /// Triangle A-B-C (n=3, all pairs connected).
    /// d=2 for every node.  triangles=1 for every edge.
    /// Ric = 2/2 + 2/2 − 2 + 1*(1/2+1/2) = 0 + 1 = 1.0
    #[test]
    fn triangle() {
        let results = calculate_balanced_frc(3, edges_for(&[(0, 1), (1, 2), (0, 2)]));
        assert_eq!(results.len(), 3);
        for e in &results {
            assert!((e.ric - 1.0).abs() < 1e-9, "expected 1.0, got {}", e.ric);
            assert!(!e.is_bridge);
        }
    }

    /// Bridge between two 3-cliques.
    /// Nodes 0,1,2 form a triangle; nodes 3,4,5 form a triangle; edge 0-3 is the bridge.
    /// d_0 = 3 (connects to 1,2,3), d_3 = 3 (connects to 0,4,5).
    /// t(0,3) = 0 (no shared neighbours).
    /// Ric(0,3) = 2/3 + 2/3 − 2 + 0 = −2/3 ≈ −0.667
    #[test]
    fn bridge_between_cliques() {
        let edges = edges_for(&[
            (0, 1), (1, 2), (0, 2), // clique 1
            (3, 4), (4, 5), (3, 5), // clique 2
            (0, 3),                 // bridge
        ]);
        let results = calculate_balanced_frc(6, edges);
        let bridge = results.iter().find(|e| {
            (e.source_idx == 0 && e.target_idx == 3)
                || (e.source_idx == 3 && e.target_idx == 0)
        });
        let bridge = bridge.expect("bridge edge not found");
        let expected = 2.0 / 3.0 + 2.0 / 3.0 - 2.0;
        assert!((bridge.ric - expected).abs() < 1e-9);
        // Not bridge-flagged because |Ric| < 1.0 threshold (d=3 nodes)
        assert!(!bridge.is_bridge);
    }

    /// High-degree bridge: node 0 connects to nodes 1-9, node 10 connects to 11-19,
    /// plus a single bridge 0-10.  d_0=10, d_10=10, t=0.
    /// Ric(0,10) = 2/10 + 2/10 − 2 = −1.6  → is_bridge = true
    #[test]
    fn high_degree_bridge_is_flagged() {
        let mut edges: Vec<(usize, usize)> = (1..=9).map(|i| (0, i)).collect();
        edges.extend((11..=19).map(|i| (10, i)));
        edges.push((0, 10)); // the bridge
        let results = calculate_balanced_frc(20, edges);
        let bridge = results
            .iter()
            .find(|e| {
                (e.source_idx == 0 && e.target_idx == 10)
                    || (e.source_idx == 10 && e.target_idx == 0)
            })
            .expect("bridge edge not found");
        let expected = 2.0 / 10.0 + 2.0 / 10.0 - 2.0; // −1.6
        assert!((bridge.ric - expected).abs() < 1e-9);
        assert!(bridge.is_bridge, "expected is_bridge=true, got ric={}", bridge.ric);
    }

    /// Empty inputs produce empty output without panic.
    #[test]
    fn empty_graph() {
        assert!(calculate_balanced_frc(0, vec![]).is_empty());
        assert!(calculate_balanced_frc(5, vec![]).is_empty());
    }
}
