const std = @import("std");

const Node = struct {
    key: i32,
    val: i32,
    prev: ?*Node,
    next: ?*Node,
};

const LRUCache = struct {
    alloc: std.mem.Allocator,
    capacity: usize,
    map: std.AutoHashMap(i32, *Node),
    head: *Node, // sentinel MRU side
    tail: *Node, // sentinel LRU side

    fn init(alloc: std.mem.Allocator, capacity: usize) !LRUCache {
        const head = try alloc.create(Node);
        const tail = try alloc.create(Node);
        head.* = .{ .key = 0, .val = 0, .prev = null, .next = tail };
        tail.* = .{ .key = 0, .val = 0, .prev = head, .next = null };
        return LRUCache{
            .alloc = alloc,
            .capacity = capacity,
            .map = std.AutoHashMap(i32, *Node).init(alloc),
            .head = head,
            .tail = tail,
        };
    }

    fn deinit(self: *LRUCache) void {
        var it = self.map.iterator();
        while (it.next()) |entry| self.alloc.destroy(entry.value_ptr.*);
        self.map.deinit();
        self.alloc.destroy(self.head);
        self.alloc.destroy(self.tail);
    }

    // Remove a node from the linked list (does not free it).
    fn remove(self: *LRUCache, node: *Node) void {
        _ = self;
        if (node.prev) |p| p.next = node.next;
        if (node.next) |n| n.prev = node.prev;
    }

    // Insert node right after head (MRU position).
    fn insertFront(self: *LRUCache, node: *Node) void {
        node.next = self.head.next;
        node.prev = self.head;
        if (self.head.next) |n| n.prev = node;
        self.head.next = node;
    }

    // get returns value for key, or -1. Marks key as recently used.
    fn get(self: *LRUCache, key: i32) i32 {
        if (self.map.get(key)) |node| {
            self.remove(node);
            self.insertFront(node);
            return node.val;
        }
        return -1;
    }

    // put inserts or updates key/value. Evicts LRU when over capacity.
    fn put(self: *LRUCache, key: i32, value: i32) !void {
        if (self.map.get(key)) |node| {
            node.val = value;
            self.remove(node);
            self.insertFront(node);
            return;
        }
        const node = try self.alloc.create(Node);
        node.* = .{ .key = key, .val = value, .prev = null, .next = null };
        try self.map.put(key, node);
        self.insertFront(node);
        if (self.map.count() > self.capacity) {
            const lru = self.tail.prev.?;
            self.remove(lru);
            _ = self.map.remove(lru.key);
            self.alloc.destroy(lru);
        }
    }
};

pub fn main() !void {
    var gpa = std.heap.DebugAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    var c = try LRUCache.init(alloc, 2);
    defer c.deinit();

    const w = std.io.getStdOut().writer();

    try c.put(1, 1);
    try c.put(2, 2);
    try w.print("{d}\n", .{c.get(1)});  // 1
    try c.put(3, 3);                     // evicts 2
    try w.print("{d}\n", .{c.get(2)});  // -1
    try c.put(4, 4);                     // evicts 1
    try w.print("{d}\n", .{c.get(1)});  // -1
    try w.print("{d}\n", .{c.get(3)});  // 3
    try w.print("{d}\n", .{c.get(4)});  // 4
}
