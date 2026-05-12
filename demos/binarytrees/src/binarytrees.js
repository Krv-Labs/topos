"use strict";

const { Worker, isMainThread, parentPort, workerData } = require('worker_threads');

function TreeNode(left, right) {
    this.left = left;
    this.right = right;
}

function itemCheck(node) {
    if (node.left === null) return 1;
    return 1 + itemCheck(node.left) + itemCheck(node.right);
}

function bottomUpTree(depth) {
    return depth > 0
        ? new TreeNode(bottomUpTree(depth - 1), bottomUpTree(depth - 1))
        : new TreeNode(null, null);
}

if (isMainThread) {
    const maxDepth = Math.max(6, parseInt(process.argv[2]) || 0);
    const stretchDepth = maxDepth + 1;

    const check = itemCheck(bottomUpTree(stretchDepth));
    console.log(`stretch tree of depth ${stretchDepth}\t check: ${check}`);

    const longLivedTree = bottomUpTree(maxDepth);

    const results = [];
    let tasksRemaining = 0;

    for (let depth = 4; depth <= maxDepth; depth += 2) {
        const iterations = 1 << (maxDepth - depth + 4);
        tasksRemaining++;
        
        // Use worker threads for the parallelizable part of the benchmark
        const worker = new Worker(__filename, {
            workerData: { iterations, depth }
        });

        worker.on('message', (msg) => {
            results[depth] = `${iterations}\t trees of depth ${depth}\t check: ${msg}`;
            tasksRemaining--;
            if (tasksRemaining === 0) {
                for (let d = 4; d <= maxDepth; d += 2) {
                    console.log(results[d]);
                }
                console.log(`long lived tree of depth ${maxDepth}\t check: ${itemCheck(longLivedTree)}`);
            }
        });
    }
} else {
    // Worker thread logic
    const { iterations, depth } = workerData;
    let check = 0;
    for (let i = 0; i < iterations; i++) {
        check += itemCheck(bottomUpTree(depth));
    }
    parentPort.postMessage(check);
}
