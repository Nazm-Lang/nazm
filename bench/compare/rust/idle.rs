// Concurrency: memory per live task — N threads (100,000 by default) all blocked on one gate at
// once; main waits until every one has started, then opens the gate and they end.
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let n: i64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let started = Arc::new(AtomicI64::new(0));
    let mut hs = Vec::new();
    for _ in 0..n {
        let g = gate.clone();
        let s = started.clone();
        hs.push(thread::spawn(move || {
            s.fetch_add(1, Ordering::SeqCst);
            let (m, c) = &*g;
            let mut open = m.lock().unwrap();
            while !*open {
                open = c.wait(open).unwrap();
            }
        }));
    }
    while started.load(Ordering::SeqCst) < n {
        thread::sleep(Duration::from_millis(1));
    }
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    for h in hs {
        h.join().unwrap();
    }
    println!("{}", started.load(Ordering::SeqCst));
}
