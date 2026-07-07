// The Computer Language Benchmarks Game
// https://salsa.debian.org/benchmarksgame-team/benchmarksgame/
//
// Go transliteration for topos multilang AST fixtures.

package main

import (
	"fmt"
	"os"
	"strconv"
)

type Tree struct {
	left  *Tree
	right *Tree
}

func bottomUpTree(depth int) *Tree {
	if depth <= 0 {
		return &Tree{}
	}
	return &Tree{
		left:  bottomUpTree(depth - 1),
		right: bottomUpTree(depth - 1),
	}
}

func itemCheck(tree *Tree) int {
	if tree.left == nil {
		return 1
	}
	return 1 + itemCheck(tree.left) + itemCheck(tree.right)
}

func main() {
	n := 10
	if len(os.Args) > 1 {
		parsed, err := strconv.Atoi(os.Args[1])
		if err == nil {
			n = parsed
		}
	}

	minDepth := 4
	maxDepth := n
	if minDepth+2 > maxDepth {
		maxDepth = minDepth + 2
	}
	stretchDepth := maxDepth + 1

	stretchTree := bottomUpTree(stretchDepth)
	fmt.Printf("stretch tree of depth %d\t check: %d\n", stretchDepth, itemCheck(stretchTree))

	longLivedTree := bottomUpTree(maxDepth)

	for depth := minDepth; depth <= maxDepth; depth += 2 {
		iterations := 1 << uint(maxDepth-depth+minDepth)
		check := 0
		for i := 0; i < iterations; i++ {
			tree := bottomUpTree(depth)
			check += itemCheck(tree)
		}
		fmt.Printf("%d\t trees of depth %d\t check: %d\n", iterations, depth, check)
	}

	fmt.Printf("long lived tree of depth %d\t check: %d\n", maxDepth, itemCheck(longLivedTree))
}
