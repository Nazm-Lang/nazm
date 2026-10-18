// Concurrency: spawn N OS threads (1,000,000 by default), each adding 1 to a shared atomic; they are
// spawned and joined 1,000 at a time (a process cannot hold a million live threads), then the count
// is printed.
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::thread;

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let done = Arc::new(AtomicI64::new(0));
    let mut left = n;
    while left > 0 {
        let batch = left.min(1000);
        let hs: Vec<_> = (0..batch)
            .map(|_| {
                let d = done.clone();
                thread::spawn(move || {
                    d.fetch_add(1, Ordering::SeqCst);
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        left -= batch;
    }
    println!("{}", done.load(Ordering::SeqCst));
}
