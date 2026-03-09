package main

import "fmt"

func countNeighbors(grid [][]byte, row, col, rows, cols int) int {
	count := 0
	for dr := -1; dr <= 1; dr++ {
		for dc := -1; dc <= 1; dc++ {
			if dr == 0 && dc == 0 {
				continue
			}
			nr, nc := row+dr, col+dc
			if nr < 0 || nr >= rows || nc < 0 || nc >= cols {
				continue
			}
			if grid[nr][nc] == 1 {
				count++
			}
		}
	}
	return count
}

func nextGeneration(grid [][]byte, rows, cols int) [][]byte {
	next := make([][]byte, rows)
	for i := range next {
		next[i] = make([]byte, cols)
	}
	for r := 0; r < rows; r++ {
		for c := 0; c < cols; c++ {
			n := countNeighbors(grid, r, c, rows, cols)
			if grid[r][c] == 1 {
				if n == 2 || n == 3 {
					next[r][c] = 1
				}
			} else {
				if n == 3 {
					next[r][c] = 1
				}
			}
		}
	}
	return next
}

func printGrid(grid [][]byte, rows, cols int) {
	for r := 0; r < rows; r++ {
		for c := 0; c < cols; c++ {
			if grid[r][c] == 1 {
				fmt.Print("X")
			} else {
				fmt.Print(".")
			}
		}
		fmt.Println()
	}
}

func main() {
	rows, cols := 3, 3
	grid := make([][]byte, rows)
	for i := range grid {
		grid[i] = make([]byte, cols)
	}
	grid[1][0] = 1
	grid[1][1] = 1
	grid[1][2] = 1

	printGrid(grid, rows, cols)
	fmt.Println("---")
	gen1 := nextGeneration(grid, rows, cols)
	printGrid(gen1, rows, cols)
	fmt.Println("---")
	gen2 := nextGeneration(gen1, rows, cols)
	printGrid(gen2, rows, cols)
}
