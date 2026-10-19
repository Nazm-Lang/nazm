struct Node {
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

fn make(depth: u32) -> Box<Node> {
    if depth > 0 {
        Box::new(Node { left: Some(make(depth - 1)), right: Some(make(depth - 1)) })
    } else {
        Box::new(Node { left: None, right: None })
    }
}

fn check(n: &Node) -> i64 {
    match (&n.left, &n.right) {
        (Some(l), Some(r)) => 1 + check(l) + check(r),
        _ => 1,
    }
}

fn main() {
    let min_depth = 4;
    let arg: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let max_depth = arg.max(min_depth + 2);
    let stretch = max_depth + 1;
    println!("stretch tree of depth {}\t check: {}", stretch, check(&make(stretch)));
    let long_lived = make(max_depth);
    for d in (min_depth..=max_depth).step_by(2) {
        let iterations = 1u64 << (max_depth - d + min_depth);
        let mut total = 0;
        for _ in 0..iterations {
            total += check(&make(d));
        }
        println!("{}\t trees of depth {}\t check: {}", iterations, d, total);
    }
    println!("long lived tree of depth {}\t check: {}", max_depth, check(&long_lived));
}
