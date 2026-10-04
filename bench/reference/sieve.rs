// The same sieve as `bench/programs/sieve.nz`, statement for statement (N92).
//
// The closest of the references to what the Nazm program means: a growable vector pushed one
// element at a time, every index bounds-checked, and — built with `-C overflow-checks=on` —
// every addition and multiplication checked, as Nazm's are. What remains different is the
// language's: no reference counting, and a vector of `i64` rather than `Ints`.

fn main() {
    let limit: i64 = 400000;
    let mut sieve: Vec<i64> = Vec::new();
    let mut i: i64 = 0;
    while i <= limit {
        sieve.push(1);
        i = i + 1;
    }
    let mut p: i64 = 2;
    while p * p <= limit {
        if sieve[p as usize] == 1 {
            let mut m = p * p;
            while m <= limit {
                sieve[m as usize] = 0;
                m = m + p;
            }
        }
        p = p + 1;
    }
    let mut count: i64 = 0;
    let mut n: i64 = 2;
    while n <= limit {
        count = count + sieve[n as usize];
        n = n + 1;
    }
    println!("{count}");
}
