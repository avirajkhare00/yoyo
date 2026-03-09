const std = @import("std");

fn countNeighbors(grid: []const u8, row: usize, col: usize, rows: usize, cols: usize) u8 {
    var count: u8 = 0;
    const r = @as(i64, @intCast(row));
    const c = @as(i64, @intCast(col));
    var dr: i64 = -1;
    while (dr <= 1) : (dr += 1) {
        var dc: i64 = -1;
        while (dc <= 1) : (dc += 1) {
            if (dr == 0 and dc == 0) continue;
            const nr = r + dr;
            const nc = c + dc;
            if (nr < 0 or nr >= @as(i64, @intCast(rows))) continue;
            if (nc < 0 or nc >= @as(i64, @intCast(cols))) continue;
            count += grid[@as(usize, @intCast(nr)) * cols + @as(usize, @intCast(nc))];
        }
    }
    return count;
}

fn nextGeneration(alloc: std.mem.Allocator, grid: []const u8, rows: usize, cols: usize) ![]u8 {
    const next = try alloc.alloc(u8, rows * cols);
    for (0..rows) |r| {
        for (0..cols) |c| {
            const n = countNeighbors(grid, r, c, rows, cols);
            const alive = grid[r * cols + c] == 1;
            next[r * cols + c] = if (alive) (if (n == 2 or n == 3) @as(u8, 1) else 0)
                                 else       (if (n == 3) @as(u8, 1) else 0);
        }
    }
    return next;
}

fn printGrid(grid: []const u8, rows: usize, cols: usize) void {
    const w = std.io.getStdOut().writer();
    for (0..rows) |r| {
        for (0..cols) |c| {
            w.print("{s}", .{if (grid[r * cols + c] == 1) "X" else "."}) catch {};
        }
        w.print("\n", .{}) catch {};
    }
}

pub fn main() !void {
    var gpa = std.heap.DebugAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    const rows: usize = 3;
    const cols: usize = 3;
    const grid = try alloc.alloc(u8, rows * cols);
    defer alloc.free(grid);
    @memset(grid, 0);

    grid[1 * cols + 0] = 1;
    grid[1 * cols + 1] = 1;
    grid[1 * cols + 2] = 1;

    printGrid(grid, rows, cols);
    const w = std.io.getStdOut().writer();
    try w.print("---\n", .{});
    const gen1 = try nextGeneration(alloc, grid, rows, cols);
    defer alloc.free(gen1);
    printGrid(gen1, rows, cols);
    try w.print("---\n", .{});
    const gen2 = try nextGeneration(alloc, gen1, rows, cols);
    defer alloc.free(gen2);
    printGrid(gen2, rows, cols);
}
