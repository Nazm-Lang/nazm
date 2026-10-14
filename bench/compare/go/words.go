package main

import (
	"fmt"
	"os"
	"sort"
	"strings"
)

func main() {
	text, _ := os.ReadFile(os.Args[1])
	counts := map[string]int{}
	for _, w := range strings.Fields(string(text)) {
		counts[w]++
	}
	words := make([]string, 0, len(counts))
	for w := range counts {
		words = append(words, w)
	}
	sort.Slice(words, func(i, j int) bool {
		a, b := words[i], words[j]
		return counts[a] > counts[b] || (counts[a] == counts[b] && a < b)
	})
	fmt.Println(len(counts))
	for _, w := range words[:10] {
		fmt.Println(w, counts[w])
	}
}
