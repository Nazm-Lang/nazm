// Concurrency: spawn N virtual threads (1,000,000 by default), each adding 1 to a shared atomic; the
// executor's close waits for all of them.
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicLong;

public class Spawn {
    public static void main(String[] a) {
        int n = a.length > 0 ? Integer.parseInt(a[0]) : 1000000;
        AtomicLong done = new AtomicLong();
        try (var ex = Executors.newVirtualThreadPerTaskExecutor()) {
            for (int i = 0; i < n; i++) ex.submit(() -> done.incrementAndGet());
        }
        System.out.println(done.get());
    }
}
