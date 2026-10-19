const int MinDepth = 4;
var maxDepth = Math.Max(args.Length > 0 ? int.Parse(args[0]) : 10, MinDepth + 2);
var stretch = maxDepth + 1;
Console.WriteLine($"stretch tree of depth {stretch}\t check: {Check(Make(stretch))}");
var longLived = Make(maxDepth);
for (int d = MinDepth; d <= maxDepth; d += 2)
{
    long iterations = 1L << (maxDepth - d + MinDepth);
    long total = 0;
    for (long i = 0; i < iterations; i++) total += Check(Make(d));
    Console.WriteLine($"{iterations}\t trees of depth {d}\t check: {total}");
}
Console.WriteLine($"long lived tree of depth {maxDepth}\t check: {Check(longLived)}");

static Node Make(int depth) => depth > 0 ? new Node(Make(depth - 1), Make(depth - 1)) : new Node(null, null);

static long Check(Node n) => n.Left is null ? 1 : 1 + Check(n.Left) + Check(n.Right!);

record Node(Node? Left, Node? Right);
