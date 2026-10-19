public class Bintrees {
    record Node(Node left, Node right) {}

    static Node make(int depth) {
        return depth > 0 ? new Node(make(depth - 1), make(depth - 1)) : new Node(null, null);
    }

    static long check(Node n) {
        return n.left() == null ? 1 : 1 + check(n.left()) + check(n.right());
    }

    public static void main(String[] args) {
        int minDepth = 4;
        int maxDepth = Math.max(args.length > 0 ? Integer.parseInt(args[0]) : 10, minDepth + 2);
        int stretch = maxDepth + 1;
        System.out.println("stretch tree of depth " + stretch + "\t check: " + check(make(stretch)));
        Node longLived = make(maxDepth);
        for (int d = minDepth; d <= maxDepth; d += 2) {
            long iterations = 1L << (maxDepth - d + minDepth);
            long total = 0;
            for (long i = 0; i < iterations; i++) total += check(make(d));
            System.out.println(iterations + "\t trees of depth " + d + "\t check: " + total);
        }
        System.out.println("long lived tree of depth " + maxDepth + "\t check: " + check(longLived));
    }
}
