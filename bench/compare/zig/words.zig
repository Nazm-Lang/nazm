const std = @import("std");

const Entry = struct { word: []const u8, count: u32 };

fn before(_: void, a: Entry, b: Entry) bool {
    return a.count > b.count or (a.count == b.count and std.mem.order(u8, a.word, b.word) == .lt);
}

pub fn main(init: std.process.Init) !void {
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    const text = try std.Io.Dir.cwd().readFileAlloc(init.io, args[1], init.gpa, .unlimited);
    var counts = std.StringHashMap(u32).init(init.gpa);
    var words = std.mem.tokenizeAny(u8, text, " \n");
    while (words.next()) |w| {
        const e = try counts.getOrPut(w);
        e.value_ptr.* = if (e.found_existing) e.value_ptr.* + 1 else 1;
    }
    var all: std.ArrayList(Entry) = .empty;
    var it = counts.iterator();
    while (it.next()) |e| try all.append(init.gpa, .{ .word = e.key_ptr.*, .count = e.value_ptr.* });
    std.mem.sort(Entry, all.items, {}, before);
    var buf: [4096]u8 = undefined;
    var out: std.Io.File.Writer = .init(.stdout(), init.io, &buf);
    try out.interface.print("{d}\n", .{counts.count()});
    for (all.items[0..10]) |e| try out.interface.print("{s} {d}\n", .{ e.word, e.count });
    try out.interface.flush();
}
