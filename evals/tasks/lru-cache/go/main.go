package main

import "fmt"

type Node struct {
	key, val   int
	prev, next *Node
}

type LRUCache struct {
	capacity   int
	cache      map[int]*Node
	head, tail *Node // sentinels
}

func NewLRUCache(capacity int) *LRUCache {
	head, tail := &Node{}, &Node{}
	head.next = tail
	tail.prev = head
	return &LRUCache{capacity: capacity, cache: make(map[int]*Node), head: head, tail: tail}
}

func (c *LRUCache) remove(n *Node) {
	n.prev.next = n.next
	n.next.prev = n.prev
}

func (c *LRUCache) insertFront(n *Node) {
	n.next = c.head.next
	n.prev = c.head
	c.head.next.prev = n
	c.head.next = n
}

// Get returns the value for key, or -1 if not present. Marks key as recently used.
func (c *LRUCache) Get(key int) int {
	if n, ok := c.cache[key]; ok {
		c.remove(n)
		c.insertFront(n)
		return n.val
	}
	return -1
}

// Put inserts or updates key/value. Evicts the least recently used key when over capacity.
func (c *LRUCache) Put(key, value int) {
	if n, ok := c.cache[key]; ok {
		n.val = value
		c.remove(n)
		c.insertFront(n)
		return
	}
	n := &Node{key: key, val: value}
	c.cache[key] = n
	c.insertFront(n)
	if len(c.cache) > c.capacity {
		lru := c.tail.prev
		c.remove(lru)
		delete(c.cache, lru.key)
	}
}

func main() {
	c := NewLRUCache(2)
	c.Put(1, 1)
	c.Put(2, 2)
	fmt.Println(c.Get(1))  // 1    — moves 1 to MRU, LRU is now 2
	c.Put(3, 3)            // evicts 2
	fmt.Println(c.Get(2))  // -1
	c.Put(4, 4)            // evicts 1
	fmt.Println(c.Get(1))  // -1
	fmt.Println(c.Get(3))  // 3
	fmt.Println(c.Get(4))  // 4
}
