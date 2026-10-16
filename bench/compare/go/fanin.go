// Concurrency 6.1: fan-in — P producers (8 by default) each send 1,000,000 / P integers into one
// channel of capacity 1,024; one consumer (main) sums them and prints the sum.
package main

import (
	"fmt"
	"os"
	"strconv"
	"sync"
)

func main() {
	producers := 8
	if len(os.Args) > 1 {
		if v, err := strconv.Atoi(os.Args[1]); err == nil {
			producers = v
		}
	}
	each := 1000000 / producers
	c := make(chan int, 1024)
	var wg sync.WaitGroup
	wg.Add(producers)
	for p := 0; p < producers; p++ {
		go func(from int) {
			for i := from; i < from+each; i++ {
				c <- i
			}
			wg.Done()
		}(p * each)
	}
	go func() {
		wg.Wait()
		close(c)
	}()
	sum := 0
	for v := range c {
		sum += v
	}
	fmt.Println(sum)
}
