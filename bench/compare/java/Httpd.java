// Concurrency: an HTTP/1.1 server on loopback (com.sun.net.httpserver, a virtual thread per request),
// every request answered `200 hello` as text; serves on the address its argument names until killed.
import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.util.concurrent.Executors;

public class Httpd {
    public static void main(String[] a) throws Exception {
        String[] hp = a[0].split(":");
        var s = HttpServer.create(new InetSocketAddress(hp[0], Integer.parseInt(hp[1])), 1024);
        s.createContext("/", ex -> {
            byte[] b = "hello".getBytes();
            ex.getResponseHeaders().set("Content-Type", "text/plain; charset=utf-8");
            ex.sendResponseHeaders(200, b.length);
            try (var o = ex.getResponseBody()) { o.write(b); }
        });
        s.setExecutor(Executors.newVirtualThreadPerTaskExecutor());
        s.start();
    }
}
