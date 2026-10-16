// Concurrency 6.1: memory per live task — N goroutines (100,000 by default) all blocked on one
// channel at once; main waits until every one has started, then closes the channel and they end.
// Peak resident memory is read from outside (/usr/bin/time -l), against N = 0.
package main

import (
	"fmt"
	"os"
	"strconv"
	"sync"
	"sync/atomic"
	"time"
)

func main() {
	n := 100000
	if len(os.Args) > 1 {
		if v, err := strconv.Atoi(os.Args[1]); err == nil {
			n = v
		}
	}
	gate := make(chan int, 1)
	var started atomic.Int64
	var wg sync.WaitGroup
	wg.Add(n)
	for i := 0; i < n; i++ {
		go func() {
			started.Add(1)
			<-gate
			wg.Done()
		}()
	}
	for started.Load() < int64(n) {
		time.Sleep(time.Millisecond)
	}
	close(gate)
	wg.Wait()
	fmt.Println(started.Load())
}
