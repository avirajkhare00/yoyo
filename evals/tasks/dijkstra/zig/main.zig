const std = @import("std");

const Edge = struct { to: usize, weight: u64 };

const Item = struct {
    node: usize,
    dist: u64,

    fn lessThan(_: void, a: Item, b: Item) std.math.Order {
        return std.math.order(a.dist, b.dist);
    }
};

// dijkstra returns shortest distances from start; std.math.maxInt(u64) for unreachable.
// Caller owns returned slice.
fn dijkstra(alloc: std.mem.Allocator, graph: []const []const Edge, start: usize) ![]u64 {
    const n = graph.len;
    const dist = try alloc.alloc(u64, n);
    @memset(dist, std.math.maxInt(u64));
    dist[start] = 0;

    var pq = std.PriorityQueue(Item, void, Item.lessThan).init(alloc, {});
    defer pq.deinit();
    try pq.add(.{ .node = start, .dist = 0 });

    while (pq.count() > 0) {
        const cur = pq.remove();
        if (cur.dist > dist[cur.node]) continue;
        for (graph[cur.node]) |e| {
            const nd = cur.dist + e.weight;
            if (nd < dist[e.to]) {
                dist[e.to] = nd;
                try pq.add(.{ .node = e.to, .dist = nd });
            }
        }
    }

    return dist;
}

pub fn main() !void {
    var gpa = std.heap.DebugAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    // 5 nodes: 0→2 w1, 0→1 w4, 2→1 w2, 1→3 w1, 2→3 w5, 3→4 w3
    const e0 = [_]Edge{ .{ .to = 2, .weight = 1 }, .{ .to = 1, .weight = 4 } };
    const e1 = [_]Edge{.{ .to = 3, .weight = 1 }};
    const e2 = [_]Edge{ .{ .to = 1, .weight = 2 }, .{ .to = 3, .weight = 5 } };
    const e3 = [_]Edge{.{ .to = 4, .weight = 3 }};
    const e4 = [_]Edge{};

    const graph = [_][]const Edge{ &e0, &e1, &e2, &e3, &e4 };

    const dist = try dijkstra(alloc, &graph, 0);
    defer alloc.free(dist);

    const w = std.io.getStdOut().writer();
    for (dist, 0..) |d, i| {
        try w.print("{d}: {d}\n", .{ i, d });
    }
}
