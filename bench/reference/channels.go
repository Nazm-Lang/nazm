// The same hand-off as `bench/programs/channels.nz`, statement for statement (Gate 4 light).
//
// One producer, one consumer, a channel of capacity one, so every message is a synchronisation;
// the producer runs in a scope nested inside the consumer's, as Nazm's does. Go's arithmetic wraps
// rather than failing; nothing here comes near overflow.
package main

import (
	"fmt"
	"sync"
)

func produce(jobs chan int64) {
	var i int64 = 1
	for i <= 50000 {
		jobs <- i
		i = i + 1
	}
}

func consume(jobs chan int64, out chan int64) {
	var sum int64 = 0
	for v := range jobs {
		sum = sum + v
	}
	out <- sum
}

func main() {
	jobs := make(chan int64, 1)
	out := make(chan int64, 1)
	var outer sync.WaitGroup
	outer.Add(1)
	go func() { defer outer.Done(); consume(jobs, out) }()
	var inner sync.WaitGroup
	inner.Add(1)
	go func() { defer inner.Done(); produce(jobs) }()
	inner.Wait()
	close(jobs)
	outer.Wait()
	fmt.Println(<-out)
}
