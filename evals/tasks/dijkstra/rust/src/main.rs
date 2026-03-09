use std::collections::BinaryHeap;
use std::cmp::Reverse;

// graph[u] = list of (v, weight)
// Returns shortest distances from start; u64::MAX for unreachable nodes.
fn dijkstra(graph: &Vec<Vec<(usize, u64)>>, start: usize) -> Vec<u64> {
    // TODO: fill in
    let _ = (graph, start);
    vec![]
}

fn main() {
    // 5 nodes: 0→2 w1, 0→1 w4, 2→1 w2, 1→3 w1, 2→3 w5, 3→4 w3
    let mut graph: Vec<Vec<(usize, u64)>> = vec![vec![]; 5];
    graph[0] = vec![(2, 1), (1, 4)];
    graph[1] = vec![(3, 1)];
    graph[2] = vec![(1, 2), (3, 5)];
    graph[3] = vec![(4, 3)];
    graph[4] = vec![];

    let dist = dijkstra(&graph, 0);
    for (i, d) in dist.iter().enumerate() {
        println!("{}: {}", i, d);
    }
}
