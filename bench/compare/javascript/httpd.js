// Concurrency: an HTTP/1.1 server on loopback (node:http, one event loop), every request answered
// `200 hello` as text; serves on the address its argument names (`127.0.0.1:8084`) until killed.
const http = require("node:http");
const [host, port] = process.argv[2].split(":");
http
    .createServer((req, res) => {
        res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("hello");
    })
    .listen(parseInt(port), host);
