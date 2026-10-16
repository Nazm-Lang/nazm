// Concurrency 6.1: spawn N goroutines (1,000,000 by default), each adding 1 to a shared atomic
// counter; a WaitGroup waits for all of them, then the count is printed.
package main

import (
	"fmt"
	"os"
	"strconv"
	"sync"
	"sync/atomic"
)

func main() {
	n := 1000000
	if len(os.Args) > 1 {
		if v, err := strconv.Atoi(os.Args[1]); err == nil {
			n = v
		}
	}
	var done atomic.Int64
	var wg sync.WaitGroup
	wg.Add(n)
	for i := 0; i < n; i++ {
		go func() {
			done.Add(1)
			wg.Done()
		}()
	}
	wg.Wait()
	fmt.Println(done.Load())
}
