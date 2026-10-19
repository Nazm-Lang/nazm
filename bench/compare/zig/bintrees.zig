const std = @import("std");

const Node = struct { left: ?*Node, right: ?*Node };

fn make(a: std.mem.Allocator, depth: u32) !*Node {
    const n = try a.create(Node);
    n.* = if (depth > 0)
        .{ .left = try make(a, depth - 1), .right = try make(a, depth - 1) }
    else
        .{ .left = null, .right = null };
    return n;
}

fn check(n: *const Node) i64 {
    return if (n.left) |l| 1 + check(l) + check(n.right.?) else 1;
}

fn release(a: std.mem.Allocator, n: *Node) void {
    if (n.left) |l| {
        release(a, l);
        release(a, n.right.?);
    }
    a.destroy(n);
}

pub fn main(init: std.process.Init) !void {
    const a = init.gpa;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    const min_depth: u32 = 4;
    const arg: u32 = if (args.len > 1) try std.fmt.parseInt(u32, args[1], 10) else 10;
    const max_depth = @max(arg, min_depth + 2);
    var buf: [1024]u8 = undefined;
    var out: std.Io.File.Writer = .init(.stdout(), init.io, &buf);
    const stretch = max_depth + 1;
    const s = try make(a, stretch);
    try out.interface.print("stretch tree of depth {d}\t check: {d}\n", .{ stretch, check(s) });
    release(a, s);
    const long_lived = try make(a, max_depth);
    var d = min_depth;
    while (d <= max_depth) : (d += 2) {
        const iterations = @as(u64, 1) << @intCast(max_depth - d + min_depth);
        var total: i64 = 0;
        var i: u64 = 0;
        while (i < iterations) : (i += 1) {
            const t = try make(a, d);
            total += check(t);
            release(a, t);
        }
        try out.interface.print("{d}\t trees of depth {d}\t check: {d}\n", .{ iterations, d, total });
    }
    try out.interface.print("long lived tree of depth {d}\t check: {d}\n", .{ max_depth, check(long_lived) });
    release(a, long_lived);
    try out.interface.flush();
}
