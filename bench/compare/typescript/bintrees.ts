type Node = { left: Node | null; right: Node | null };

function make(depth: number): Node {
  return depth > 0 ? { left: make(depth - 1), right: make(depth - 1) } : { left: null, right: null };
}

function check(n: Node): number {
  return n.left === null ? 1 : 1 + check(n.left) + check(n.right!);
}

const minDepth = 4;
const maxDepth = Math.max(process.argv.length > 2 ? parseInt(process.argv[2], 10) : 10, minDepth + 2);
const stretch = maxDepth + 1;
console.log(`stretch tree of depth ${stretch}\t check: ${check(make(stretch))}`);
const longLived = make(maxDepth);
for (let d = minDepth; d <= maxDepth; d += 2) {
  const iterations = 2 ** (maxDepth - d + minDepth);
  let total = 0;
  for (let i = 0; i < iterations; i++) total += check(make(d));
  console.log(`${iterations}\t trees of depth ${d}\t check: ${total}`);
}
console.log(`long lived tree of depth ${maxDepth}\t check: ${check(longLived)}`);
