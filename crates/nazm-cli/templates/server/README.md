# {{name}}

A worker pool over channels serving the requests in a file: `nazm run . -- requests.txt`. Nazm has
neither sockets nor a way to read standard input, so a request is a line of a file. `nazm test
tests` checks the request handler; `nazm build . -o {{name}}` builds it.
