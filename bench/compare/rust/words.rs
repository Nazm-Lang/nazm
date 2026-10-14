use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for w in text.split_ascii_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    let mut all: Vec<_> = counts.iter().collect();
    all.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("{}", counts.len());
    for (w, n) in &all[..10] {
        println!("{w} {n}");
    }
}
