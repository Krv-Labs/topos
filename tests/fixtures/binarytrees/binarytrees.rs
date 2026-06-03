// The Computer Language Benchmarks Game
// https://salsa.debian.org/benchmarksgame-team/benchmarksgame/
//
// contributed by 1011X
// contributed by Michal Muskala
// modified by Jiri Vymazal
// modified by Jeremy Zerfas

extern crate rayon;
extern crate typed_arena;

use rayon::prelude::*;
use typed_arena::Arena;

struct Tree<'a> {
    left: Option<&'a Tree<'a>>,
    right: Option<&'a Tree<'a>>,
}

fn item_check(tree: &Tree) -> i32 {
    if let (Some(left), Some(right)) = (tree.left, tree.right) {
        1 + item_check(left) + item_check(right)
    } else {
        1
    }
}

fn bottom_up_tree<'a>(arena: &'a Arena<Tree<'a>>, depth: i32) -> &'a Tree<'a> {
    let tree = arena.alloc(Tree {
        left: None,
        right: None,
    });
    if depth > 0 {
        tree.left = Some(bottom_up_tree(arena, depth - 1));
        tree.right = Some(bottom_up_tree(arena, depth - 1));
    }
    tree
}

fn main() {
    let n = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(10);
    let min_depth = 4;
    let max_depth = if min_depth + 2 > n { min_depth + 2 } else { n };
    let stretch_depth = max_depth + 1;

    {
        let arena = Arena::new();
        let stretch_tree = bottom_up_tree(&arena, stretch_depth);
        println!(
            "stretch tree of depth {}\t check: {}",
            stretch_depth,
            item_check(stretch_tree)
        );
    }

    let long_lived_arena = Arena::new();
    let long_lived_tree = bottom_up_tree(&long_lived_arena, max_depth);

    let messages: Vec<_> = (min_depth..=max_depth)
        .into_par_iter()
        .step_by(2)
        .map(|depth| {
            let iterations = 1 << (max_depth - depth + min_depth);
            let mut check = 0;

            for _ in 0..iterations {
                let arena = Arena::new();
                let tree = bottom_up_tree(&arena, depth);
                check += item_check(tree);
            }

            format!("{}\t trees of depth {}\t check: {}", iterations, depth, check)
        })
        .collect();

    for message in messages {
        println!("{}", message);
    }

    println!(
        "long lived tree of depth {}\t check: {}",
        max_depth,
        item_check(long_lived_tree)
    );
}
