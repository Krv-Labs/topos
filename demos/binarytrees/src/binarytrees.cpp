/* The Computer Language Benchmarks Game
 * https://salsa.debian.org/benchmarksgame-team/benchmarksgame/
 *
 * Contributed by Jon Harrop
 * Modified by Alex Mizrahi
 * Modified by Andreas Schäfer
 * Modified by Antoine Pitrou
 * Modified by Federico G. Schwindt
 */

#include <iostream>
#include <stdlib.h>
#include <stdio.h>
#include <apr_pools.h>

struct Node {
    Node *l, *r;

    int check() const {
        if (l)
            return l->check() + r->check() + 1;
        else
            return 1;
    }
};

Node *make(int d, apr_pool_t *pool) {
    Node *n = (Node *)apr_palloc(pool, sizeof(Node));
    if (d > 0) {
        n->l = make(d - 1, pool);
        n->r = make(d - 1, pool);
    } else {
        n->l = n->r = nullptr;
    }
    return n;
}

int main(int argc, char *argv[]) {
    int min_depth = 4;
    int max_depth = (argc > 1) ? atoi(argv[1]) : 10;
    if (max_depth < min_depth + 2) max_depth = min_depth + 2;

    apr_initialize();

    // Stretch tree
    {
        apr_pool_t *pool;
        apr_pool_create(&pool, NULL);
        Node *n = make(max_depth + 1, pool);
        std::cout << "stretch tree of depth " << (max_depth + 1) << "\t check: " << n->check() << "\n";
        apr_pool_destroy(pool);
    }

    // Long-lived tree
    apr_pool_t *long_lived_pool;
    apr_pool_create(&long_lived_pool, NULL);
    Node *long_lived_tree = make(max_depth, long_lived_pool);

    // Work loop
    for (int d = min_depth; d <= max_depth; d += 2) {
        int iterations = 1 << (max_depth - d + min_depth);
        int check = 0;

        for (int i = 1; i <= iterations; ++i) {
            apr_pool_t *pool;
            apr_pool_create(&pool, NULL);
            Node *n = make(d, pool);
            check += n->check();
            apr_pool_destroy(pool);
        }

        std::cout << iterations << "\t trees of depth " << d << "\t check: " << check << "\n";
    }

    std::cout << "long lived tree of depth " << max_depth << "\t check: " << long_lived_tree->check() << "\n";

    apr_pool_destroy(long_lived_pool);
    apr_terminate();

    return 0;
}
