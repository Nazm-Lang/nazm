use std::thread;

fn is_prime(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    if n % 2 == 0 {
        return n == 2;
    }
    let mut d = 3;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 2;
    }
    true
}

// Blocks of 1,000 numbers below 3,000,000: block b is worker b mod W's.
fn count(w: i64, workers: i64) -> i64 {
    (w..3000)
        .step_by(workers as usize)
        .map(|b| (b * 1000..b * 1000 + 1000).filter(|&n| is_prime(n)).count() as i64)
        .sum()
}

fn main() {
    let workers: i64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let total: i64 = thread::scope(|s| {
        let handles: Vec<_> = (0..workers).map(|w| s.spawn(move || count(w, workers))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum()
    });
    println!("{total}");
}
