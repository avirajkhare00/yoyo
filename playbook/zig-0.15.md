# Zig API Ground Truth

**Pinned version: 0.14.1** — this is what the eval harness uses and what Claude is calibrated on.
Training data baseline is 0.13–0.14. Zig 0.15 introduced breaks listed at the bottom — avoid.

## Version pin rationale
- 0.11–0.12: Claude has strong training data but API is older (ArrayList managed)
- 0.13: ArrayList became unmanaged — this is the last major API break before 0.15
- **0.14.1**: stable, well-covered in training data, `std.io.getStdOut()` still works
- 0.15+: `std.io.getStdOut()` removed — outside training data, do not target

## Build commands

```bash
# Build executable (output name = source file name, no -o flag)
zig build-exe main.zig

# Run directly
zig run main.zig

# Build with optimisation
zig build-exe main.zig -O ReleaseFast
```

## Allocator

`GeneralPurposeAllocator` is aliased to `DebugAllocator`. Both names work.

```zig
var gpa = std.heap.DebugAllocator(.{}){};
defer _ = gpa.deinit();
const alloc = gpa.allocator();
```

For simple scripts, `std.heap.page_allocator` needs no deinit:
```zig
const alloc = std.heap.page_allocator;
```

## ArrayList — BREAKING CHANGE from 0.12

`ArrayList` is now **unmanaged**. Allocator is passed to every mutating method, not stored in the struct.

```zig
// WRONG (0.12 style — does not compile in 0.15):
var list = std.ArrayList(u8).init(alloc);
defer list.deinit();
try list.append(v);

// CORRECT (0.15):
var list = std.ArrayList(u8){};
defer list.deinit(alloc);
try list.append(alloc, v);
try list.appendSlice(alloc, slice);
```

Initialise with capacity:
```zig
var list = try std.ArrayList(u8).initCapacity(alloc, 64);
defer list.deinit(alloc);
```

Read items (no allocator needed — read-only):
```zig
for (list.items) |item| { ... }
const len = list.items.len;
```

## Slices (preferred over ArrayList for fixed-size grids)

```zig
// Allocate
const grid = try alloc.alloc(u8, rows * cols);
defer alloc.free(grid);

// Zero
@memset(grid, 0);

// Row-major access
grid[row * cols + col] = 1;
const val = grid[row * cols + col];
```

## Print

```zig
// stderr (always works, no error handling needed)
std.debug.print("value={d}\n", .{val});

// stdout (returns error, needs try or catch)
const stdout = std.io.getStdOut().writer();
try stdout.print("value={d}\n", .{val});
```

## Loops

```zig
// Range loop (0..n exclusive)
for (0..rows) |r| { ... }

// Slice loop with index
for (slice, 0..) |val, i| { ... }

// While
var i: usize = 0;
while (i < n) : (i += 1) { ... }
```

## Error handling

```zig
// Function that can fail
fn foo() !void { ... }
fn bar() ![]u8 { ... }

// Call with try (propagates error)
try foo();
const result = try bar();

// Call with catch (handle inline)
const result = bar() catch |err| { ... };
```

## Structs

```zig
const Grid = struct {
    cells: []u8,
    rows: usize,
    cols: usize,

    fn init(alloc: std.mem.Allocator, rows: usize, cols: usize) !Grid {
        const cells = try alloc.alloc(u8, rows * cols);
        @memset(cells, 0);
        return .{ .cells = cells, .rows = rows, .cols = cols };
    }

    fn deinit(self: Grid, alloc: std.mem.Allocator) void {
        alloc.free(self.cells);
    }
};
```

## Complete working example — Game of Life skeleton

```zig
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
            next[r * cols + c] = if (alive) (if (n == 2 or n == 3) 1 else 0)
                                  else       (if (n == 3) 1 else 0);
        }
    }
    return next;
}

fn printGrid(grid: []const u8, rows: usize, cols: usize) void {
    for (0..rows) |r| {
        for (0..cols) |c| {
            std.debug.print("{s}", .{if (grid[r * cols + c] == 1) "X" else "."});
        }
        std.debug.print("\n", .{});
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

    // Blinker (horizontal)
    grid[1 * cols + 0] = 1;
    grid[1 * cols + 1] = 1;
    grid[1 * cols + 2] = 1;

    printGrid(grid, rows, cols);
    std.debug.print("---\n", .{});

    const gen1 = try nextGeneration(alloc, grid, rows, cols);
    defer alloc.free(gen1);
    printGrid(gen1, rows, cols);
}
```

## Key int cast pattern

Zig 0.15 requires explicit casts between int types. Use `@intCast` wrapped in `@as`:

```zig
// usize → i64 (for signed arithmetic)
const signed = @as(i64, @intCast(unsigned_val));

// i64 → usize (after bounds check)
const idx = @as(usize, @intCast(signed_val));
```
