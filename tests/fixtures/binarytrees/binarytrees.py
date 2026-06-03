# The Computer Language Benchmarks Game
# https://benchmarksgame-team.pages.debian.net/benchmarksgame/
#
# contributed by Antoine Pitrou
# modified by Dominique Wahli and Daniel Nanz
# modified by Joerg Baumann

import sys
import multiprocessing as mp

def make_tree(d):
    """Recursively creates a binary tree of depth d."""
    if d > 0:
        d -= 1
        return (make_tree(d), make_tree(d))
    return (None, None)

def check_tree(node):
    """Recursively traverses the tree to return a checksum."""
    (l, r) = node
    if l is None:
        return 1
    else:
        return 1 + check_tree(l) + check_tree(r)

def make_check(itde):
    """Helper for multiprocessing: creates and checks a tree."""
    i, d = itde
    return check_tree(make_tree(d))

def get_argchunks(i, d, chunksize=5000):
    """Yields chunks of arguments to avoid overhead in multiprocessing."""
    chunk = []
    for k in range(1, i + 1):
        chunk.append((k, d))
        if len(chunk) == chunksize:
            yield chunk
            chunk = []
    if chunk:
        yield chunk

def main(n, min_depth=4):
    max_depth = max(min_depth + 2, n)
    stretch_depth = max_depth + 1

    # 1. Stretch tree
    print('stretch tree of depth {0}\t check: {1}'.format(
        stretch_depth, make_check((0, stretch_depth))))

    # 2. Long-lived tree
    long_lived_tree = make_tree(max_depth)

    # 3. Iterative trees of increasing depths
    mmd = max_depth + min_depth
    if mp.cpu_count() > 1:
        pool = mp.Pool()
        chunkmap = pool.map
    else:
        chunkmap = map

    for d in range(min_depth, stretch_depth, 2):
        i = 2 ** (mmd - d)
        cs = 0
        for argchunk in get_argchunks(i, d):
            cs += sum(chunkmap(make_check, argchunk))
        print('{0}\t trees of depth {1}\t check: {2}'.format(i, d, cs))

    # 4. Check long-lived tree
    print('long lived tree of depth {0}\t check: {1}'.format(
        max_depth, check_tree(long_lived_tree)))

if __name__ == '__main__':
    # Default N is 10 if not provided
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    main(n)
