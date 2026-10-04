// The same sieve as `bench/programs/sieve.nz`, statement for statement (N92).
//
// A slice appended one element at a time and every index bounds-checked, as `Ints` is; Go's
// arithmetic wraps rather than failing, so this reference's arithmetic is unchecked.
package main

import "fmt"

func main() {
	var limit int64 = 400000
	sieve := []int64{}
	var i int64 = 0
	for i <= limit {
		sieve = append(sieve, 1)
		i = i + 1
	}
	var p int64 = 2
	for p*p <= limit {
		if sieve[p] == 1 {
			m := p * p
			for m <= limit {
				sieve[m] = 0
				m = m + p
			}
		}
		p = p + 1
	}
	var count int64 = 0
	var n int64 = 2
	for n <= limit {
		count = count + sieve[n]
		n = n + 1
	}
	fmt.Println(count)
}
