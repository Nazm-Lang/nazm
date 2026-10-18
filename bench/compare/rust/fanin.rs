// Concurrency: P producer threads (8 by default) each send 1,000,000 / P integers into one channel of
// capacity 1,024; main sums them.
use std::sync::mpsc::sync_channel;
use std::thread;

fn main() {
    let producers: i64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let each = 1_000_000 / producers;
    let (tx, rx) = sync_channel::<i64>(1024);
    for p in 0..producers {
        let tx = tx.clone();
        thread::spawn(move || {
            for i in p * each..p * each + each {
                tx.send(i).unwrap();
            }
        });
    }
    drop(tx);
    let mut sum = 0i64;
    for v in rx {
        sum += v;
    }
    println!("{}", sum);
}
