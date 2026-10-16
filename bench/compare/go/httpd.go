// Concurrency 6.1: an HTTP/1.1 server on loopback (net/http, a goroutine per connection), every
// request answered `200 hello` as text; serves on the address its argument names
// (`127.0.0.1:8082`) until killed.
package main

import (
	"net/http"
	"os"
)

func main() {
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		w.Write([]byte("hello"))
	})
	http.ListenAndServe(os.Args[1], nil)
}
