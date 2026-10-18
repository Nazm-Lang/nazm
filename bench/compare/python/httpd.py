# Concurrency: an HTTP/1.1 server on loopback (http.server.ThreadingHTTPServer, a thread per
# connection, keep-alive), every request answered `200 hello` as text; serves on the address its
# argument names (`127.0.0.1:8085`) until killed.
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        b = b"hello"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def log_message(self, *a):
        pass


host, port = sys.argv[1].split(":")
ThreadingHTTPServer.request_queue_size = 256
ThreadingHTTPServer((host, int(port)), H).serve_forever()
