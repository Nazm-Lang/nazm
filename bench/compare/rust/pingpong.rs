// Concurrency: N round trips (100,000 by default) between two threads over two channels of capacity 1.
use std::sync::mpsc::sync_channel;
use std::thread;

fn main() {
    let n: i64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let (ping_tx, ping_rx) = sync_channel::<i64>(1);
    let (pong_tx, pong_rx) = sync_channel::<i64>(1);
    let echo = thread::spawn(move || {
        for v in ping_rx {
            pong_tx.send(v + 1).unwrap();
        }
    });
    let mut sum = 0i64;
    for i in 0..n {
        ping_tx.send(i).unwrap();
        sum += pong_rx.recv().unwrap();
    }
    drop(ping_tx);
    echo.join().unwrap();
    println!("{}", sum);
}
