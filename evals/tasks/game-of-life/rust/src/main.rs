fn count_neighbors(grid: &[u8], row: usize, col: usize, rows: usize, cols: usize) -> u8 {
    let mut count = 0u8;
    let r = row as i64;
    let c = col as i64;
    for dr in -1i64..=1 {
        for dc in -1i64..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = r + dr;
            let nc = c + dc;
            if nr < 0 || nr >= rows as i64 || nc < 0 || nc >= cols as i64 {
                continue;
            }
            count += grid[nr as usize * cols + nc as usize];
        }
    }
    count
}

fn next_generation(grid: &[u8], rows: usize, cols: usize) -> Vec<u8> {
    let mut next = vec![0u8; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            let n = count_neighbors(grid, r, c, rows, cols);
            let alive = grid[r * cols + c] == 1;
            next[r * cols + c] = if alive {
                if n == 2 || n == 3 { 1 } else { 0 }
            } else {
                if n == 3 { 1 } else { 0 }
            };
        }
    }
    next
}

fn print_grid(grid: &[u8], rows: usize, cols: usize) {
    for r in 0..rows {
        for c in 0..cols {
            print!("{}", if grid[r * cols + c] == 1 { "X" } else { "." });
        }
        println!();
    }
}

fn main() {
    let rows = 3usize;
    let cols = 3usize;
    let mut grid = vec![0u8; rows * cols];
    grid[1 * cols + 0] = 1;
    grid[1 * cols + 1] = 1;
    grid[1 * cols + 2] = 1;

    print_grid(&grid, rows, cols);
    println!("---");
    let gen1 = next_generation(&grid, rows, cols);
    print_grid(&gen1, rows, cols);
    println!("---");
    let gen2 = next_generation(&gen1, rows, cols);
    print_grid(&gen2, rows, cols);
}
