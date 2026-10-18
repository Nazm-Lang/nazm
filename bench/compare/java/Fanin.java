// Concurrency: P producer virtual threads (8 by default) each send 1,000,000 / P integers into one
// blocking queue of capacity 1,024; main sums them (each producer ends with a -1).
import java.util.concurrent.ArrayBlockingQueue;

public class Fanin {
    public static void main(String[] a) throws Exception {
        int producers = a.length > 0 ? Integer.parseInt(a[0]) : 8;
        int each = 1000000 / producers;
        var c = new ArrayBlockingQueue<Long>(1024);
        for (int p = 0; p < producers; p++) {
            final long from = (long) p * each;
            Thread.ofVirtual().start(() -> {
                try {
                    for (long i = from; i < from + each; i++) c.put(i);
                    c.put(-1L);
                } catch (InterruptedException e) {}
            });
        }
        long sum = 0;
        int ended = 0;
        while (ended < producers) {
            long v = c.take();
            if (v < 0) ended++;
            else sum += v;
        }
        System.out.println(sum);
    }
}
