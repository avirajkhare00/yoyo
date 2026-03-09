package main

import (
	"container/heap"
	"fmt"
	"math"
)

type Edge struct {
	to, weight int
}

// Item for the priority queue
type Item struct {
	node, dist int
}

type PQ []Item

func (pq PQ) Len() int            { return len(pq) }
func (pq PQ) Less(i, j int) bool  { return pq[i].dist < pq[j].dist }
func (pq PQ) Swap(i, j int)       { pq[i], pq[j] = pq[j], pq[i] }
func (pq *PQ) Push(x any)         { *pq = append(*pq, x.(Item)) }
func (pq *PQ) Pop() any           { old := *pq; n := len(old); x := old[n-1]; *pq = old[:n-1]; return x }

// dijkstra returns shortest distances from start to every node.
// Use math.MaxInt for unreachable nodes. Hint: use the PQ type above.
func dijkstra(graph [][]Edge, start int) []int {
	_ = heap.Init // keep import live until you fill this in
	// TODO: fill in
	return nil
}

func main() {
	// 5 nodes
	// 0→2 w1, 0→1 w4, 2→1 w2, 1→3 w1, 2→3 w5, 3→4 w3
	graph := make([][]Edge, 5)
	graph[0] = []Edge{{2, 1}, {1, 4}}
	graph[1] = []Edge{{3, 1}}
	graph[2] = []Edge{{1, 2}, {3, 5}}
	graph[3] = []Edge{{4, 3}}
	graph[4] = []Edge{}

	dist := dijkstra(graph, 0)
	for i, d := range dist {
		fmt.Printf("%d: %d\n", i, d)
	}
}
