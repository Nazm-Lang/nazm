// Concurrency: N round trips (100,000 by default) between two virtual threads over two blocking queues
// of capacity 1.
import java.util.concurrent.ArrayBlockingQueue;

public class Pingpong {
    public static void main(String[] a) throws Exception {
        int n = a.length > 0 ? Integer.parseInt(a[0]) : 100000;
        var ping = new ArrayBlockingQueue<Long>(1);
        var pong = new ArrayBlockingQueue<Long>(1);
        var out = new ArrayBlockingQueue<Long>(1);
        Thread.ofVirtual().start(() -> {
            try {
                while (true) {
                    long v = ping.take();
                    if (v < 0) break;
                    pong.put(v + 1);
                }
            } catch (InterruptedException e) {}
        });
        Thread.ofVirtual().start(() -> {
            try {
                long sum = 0;
                for (long i = 0; i < n; i++) {
                    ping.put(i);
                    sum += pong.take();
                }
                ping.put(-1L);
                out.put(sum);
            } catch (InterruptedException e) {}
        });
        System.out.println(out.take());
    }
}
