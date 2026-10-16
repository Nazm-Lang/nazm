// Concurrency 6.1: the shared HTTP/1.1 load generator — C keep-alive connections (64 by default),
// each a goroutine sending `GET /` and reading the whole response in a loop for D seconds (10 by
// default) after a one-second warm-up; prints requests per second, and the errors and non-200s.
//
//	httpload URL [C] [D]
package main

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"strconv"
	"sync"
	"sync/atomic"
	"time"
)

func main() {
	url := os.Args[1]
	conns, secs := 64, 10
	if len(os.Args) > 2 {
		conns, _ = strconv.Atoi(os.Args[2])
	}
	if len(os.Args) > 3 {
		secs, _ = strconv.Atoi(os.Args[3])
	}
	tr := &http.Transport{MaxIdleConns: conns, MaxIdleConnsPerHost: conns, DisableCompression: true}
	client := &http.Client{Transport: tr}
	var ok, bad, counting atomic.Int64
	var stop atomic.Bool
	var wg sync.WaitGroup
	wg.Add(conns)
	for i := 0; i < conns; i++ {
		go func() {
			defer wg.Done()
			for !stop.Load() {
				resp, err := client.Get(url)
				if err != nil {
					bad.Add(1)
					continue
				}
				io.Copy(io.Discard, resp.Body)
				resp.Body.Close()
				if counting.Load() == 1 {
					if resp.StatusCode == 200 {
						ok.Add(1)
					} else {
						bad.Add(1)
					}
				}
			}
		}()
	}
	time.Sleep(time.Second)
	counting.Store(1)
	start := time.Now()
	time.Sleep(time.Duration(secs) * time.Second)
	counting.Store(0)
	elapsed := time.Since(start).Seconds()
	stop.Store(true)
	wg.Wait()
	fmt.Printf("%.0f requests/s (%d in %.2f s, %d errors or non-200)\n", float64(ok.Load())/elapsed, ok.Load(), elapsed, bad.Load())
}
