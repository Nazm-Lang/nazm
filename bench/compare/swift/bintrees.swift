final class Node {
    let left: Node?
    let right: Node?
    init(_ left: Node?, _ right: Node?) { self.left = left; self.right = right }
}

func make(_ depth: Int) -> Node {
    depth > 0 ? Node(make(depth - 1), make(depth - 1)) : Node(nil, nil)
}

func check(_ n: Node) -> Int {
    if let l = n.left, let r = n.right { return 1 + check(l) + check(r) }
    return 1
}

let minDepth = 4
let maxDepth = max(CommandLine.arguments.count > 1 ? Int(CommandLine.arguments[1])! : 10, minDepth + 2)
let stretch = maxDepth + 1
print("stretch tree of depth \(stretch)\t check: \(check(make(stretch)))")
let longLived = make(maxDepth)
for d in stride(from: minDepth, through: maxDepth, by: 2) {
    let iterations = 1 << (maxDepth - d + minDepth)
    var total = 0
    for _ in 0..<iterations { total += check(make(d)) }
    print("\(iterations)\t trees of depth \(d)\t check: \(total)")
}
print("long lived tree of depth \(maxDepth)\t check: \(check(longLived))")
