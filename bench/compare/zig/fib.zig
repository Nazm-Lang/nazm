const std = @import("std");

fn fib(n: u64) u64 {
    return if (n < 2) n else fib(n - 1) + fib(n - 2);
}

pub fn main(init: std.process.Init) !void {
    var buf: [64]u8 = undefined;
    var out: std.Io.File.Writer = .init(.stdout(), init.io, &buf);
    try out.interface.print("{d}\n", .{fib(35)});
    try out.interface.flush();
}
