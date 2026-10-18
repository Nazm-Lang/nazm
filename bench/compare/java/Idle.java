// Concurrency: memory per live task — N virtual threads (100,000 by default) all blocked on one latch at
// once; main waits until every one has started, then opens the latch and they end.
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicLong;

public class Idle {
    public static void main(String[] a) throws Exception {
        int n = a.length > 0 ? Integer.parseInt(a[0]) : 100000;
        var gate = new CountDownLatch(1);
        var started = new AtomicLong();
        try (var ex = Executors.newVirtualThreadPerTaskExecutor()) {
            for (int i = 0; i < n; i++)
                ex.submit(() -> {
                    started.incrementAndGet();
                    gate.await();
                    return null;
                });
            while (started.get() < n) Thread.sleep(1);
            gate.countDown();
        }
        System.out.println(started.get());
    }
}
