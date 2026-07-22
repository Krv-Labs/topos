//! One-shot COMPOSABLE cypher-load timer for scripts/bench_mdg_loaders.py.
//!
//! Usage: bench_cypher_mdg <project_root> <lbug_path>

use std::env;
use std::path::PathBuf;
use std::time::Instant;

use topos_engine::adapters::gitnexus::current_git_branch;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;

fn main() {
    let mut args = env::args().skip(1);
    let project_root = PathBuf::from(args.next().expect("project_root"));
    let lbug = PathBuf::from(args.next().expect("lbug_path"));
    let branch = current_git_branch(&project_root);

    let t0 = Instant::now();
    let graph =
        ModuleDependencyGraph::from_lbug_path(&lbug, "bench", &project_root, branch.as_deref())
            .expect("mdg load");
    let ms = t0.elapsed().as_secs_f64();
    println!(
        "nodes={} rels={} secs={:.3}",
        graph.nodes.len(),
        graph.relationships.len(),
        ms
    );
}
