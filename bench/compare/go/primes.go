package main

import (
	"fmt"
	"os"
	"strconv"
)

func isPrime(n int) bool {
	if n < 2 {
		return false
	}
	if n%2 == 0 {
		return n == 2
	}
	for d := 3; d*d <= n; d += 2 {
		if n%d == 0 {
			return false
		}
	}
	return true
}

// Blocks of 1,000 numbers below 3,000,000: block b is worker b mod W's.
func count(w, workers int, out chan<- int) {
	found := 0
	for b := w; b < 3000; b += workers {
		for n := b * 1000; n < b*1000+1000; n++ {
			if isPrime(n) {
				found++
			}
		}
	}
	out <- found
}

func main() {
	workers, err := strconv.Atoi(os.Args[1])
	if err != nil {
		workers = 1
	}
	out := make(chan int, workers)
	for w := 0; w < workers; w++ {
		go count(w, workers, out)
	}
	total := 0
	for w := 0; w < workers; w++ {
		total += <-out
	}
	fmt.Println(total)
}
