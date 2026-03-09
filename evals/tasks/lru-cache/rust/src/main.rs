use std::collections::HashMap;

struct LRUCache {
    capacity: usize,
    map: HashMap<i32, i32>,
    order: std::collections::VecDeque<i32>, // front = MRU, back = LRU
}

impl LRUCache {
    fn new(capacity: usize) -> LRUCache {
        LRUCache { capacity, map: HashMap::new(), order: std::collections::VecDeque::new() }
    }

    // Returns value for key, or -1 if not present. Marks key as recently used.
    fn get(&mut self, key: i32) -> i32 {
        if let Some(&val) = self.map.get(&key) {
            self.order.retain(|&k| k != key);
            self.order.push_front(key);
            val
        } else {
            -1
        }
    }

    // Inserts or updates key/value. Evicts least recently used when over capacity.
    fn put(&mut self, key: i32, value: i32) {
        if self.map.contains_key(&key) {
            self.order.retain(|&k| k != key);
        } else if self.map.len() >= self.capacity {
            if let Some(lru) = self.order.pop_back() {
                self.map.remove(&lru);
            }
        }
        self.map.insert(key, value);
        self.order.push_front(key);
    }
}

fn main() {
    let mut c = LRUCache::new(2);
    c.put(1, 1);
    c.put(2, 2);
    println!("{}", c.get(1)); // 1
    c.put(3, 3);              // evicts 2
    println!("{}", c.get(2)); // -1
    c.put(4, 4);              // evicts 1
    println!("{}", c.get(1)); // -1
    println!("{}", c.get(3)); // 3
    println!("{}", c.get(4)); // 4
}
