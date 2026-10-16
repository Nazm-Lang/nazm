// Concurrency 6.1: channel ping-pong — N round trips (100,000 by default) between two goroutines
// over two channels of capacity 1; prints the sum of the values that came back.
package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	n := 100000
	if len(os.Args) > 1 {
		if v, err := strconv.Atoi(os.Args[1]); err == nil {
			n = v
		}
	}
	ping := make(chan int, 1)
	pong := make(chan int, 1)
	out := make(chan int, 1)
	go func() {
		for v := range ping {
			pong <- v + 1
		}
		close(pong)
	}()
	go func() {
		sum := 0
		for i := 0; i < n; i++ {
			ping <- i
			sum += <-pong
		}
		close(ping)
		out <- sum
	}()
	fmt.Println(<-out)
}
