//! R1e, S2 and S3: `@std/bytebuf`, `@std/ordmap`, `@std/bitset` and the in-place sorts of
//! `@std/sort` (`docs/spec.md`, *The standard library*).
//!
//! One program exercising each: a `ByteBuf` built in every byte order, aligned, patched, read and
//! cleared, LEB128 at its edges and at both ends of `Int` and `UInt64`, and six hundred
//! fixed-seed values written alike by a `ByteBuf` and `@std/binary`'s `Writer`; an `OrdMap` over
//! `Str`, a program's own `Ord` key and `Int`, the last driven by three thousand fixed-seed
//! insertions, removals and lookups against a sorted vector, with its keys, values, ends,
//! neighbours and ranges compared as it goes, then by the ascending and descending runs that
//! unbalance a plain tree; a `BitSet` against a vector of flags, its union, intersection,
//! difference and walk compared; and the in-place sorts against the copying ones, stability
//! checked by original positions, at the lengths around the insertion runs. Its output is the
//! same under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift, with nothing live.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const PROGRAM: &str = r#####"use "@std/bytebuf";
use "@std/binary";
use "@std/ordmap";
use "@std/bitset";
use "@std/sort";
use "@std/text";

fn text_ints(s: Str) -> Ints {
    let out = ints_new();
    for p in text_split(s, ",") { match text_parse_int(p) { Result.Ok(value: n) => { ints_push(out, n); }, Result.Err(error: e) => {}, }; }
    out
}

// A fixed-seed generator: the same numbers in every implementation.
fn next(seed: Ints) -> Int {
    let s = (ints_get(seed, 0) * 1103515245 + 12345) % 2147483648;
    ints_set(seed, 0, s);
    s / 65536
}

fn hex(b: Bytes) -> Str {
    let digits = "0123456789abcdef";
    let parts = strs_new();
    for i in 0..bytes_len(b) {
        let x = Int(bytes_get(b, i));
        strs_push(parts, str_concat(str_slice(digits, x / 16, x / 16 + 1), str_slice(digits, x % 16, x % 16 + 1)));
    }
    str_join(parts, "")
}

fn uleb(v: UInt64) -> Str { let b = bytebuf_new(); bytebuf_put_uleb128(b, v); hex(bytebuf_to_bytes(b)) }
fn sleb(v: Int) -> Str { let b = bytebuf_new(); bytebuf_put_sleb128(b, v); hex(bytebuf_to_bytes(b)) }

fn same_bytes(a: Bytes, b: Bytes) -> Bool {
    let mut ok = bytes_len(a) == bytes_len(b);
    if ok { for i in 0..bytes_len(a) { if bytes_get(a, i) != bytes_get(b, i) { ok = false; } } }
    ok
}

fn bytebuf_section(out: OutCap) -> Int ! {io} {
    let b = bytebuf_with_capacity(1);
    bytebuf_push(b, UInt8(7));
    bytebuf_put_u16_le(b, UInt16(258));
    bytebuf_put_u16_be(b, UInt16(258));
    bytebuf_put_u32_le(b, UInt32(16909060));
    bytebuf_put_u32_be(b, UInt32(16909060));
    bytebuf_put_u64_le(b, UInt64(72623859790382856));
    bytebuf_put_u64_be(b, UInt64(72623859790382856));
    bytebuf_push_str(b, "hi");
    bytebuf_push_bytes(b, bytes_from_str("!?"));
    print(f"built {bytebuf_len(b)} {hex(bytebuf_to_bytes(b))}\n");
    let size_at = bytebuf_len(b);
    bytebuf_put_u32_le(b, UInt32(0));
    print(f"aligned {bytebuf_align_to(b, 16)} {bytebuf_align_to(b, 16)} {bytebuf_align_to(b, 0)}\n");
    let snapshot = bytebuf_to_bytes(b);
    print(f"patched {bytebuf_patch_u32_le(b, size_at, UInt32(bytebuf_len(b)))} {bytebuf_patch_u32_be(b, 0 - 1, UInt32(1))} {bytebuf_patch_u64_le(b, bytebuf_len(b) - 7, UInt64(1))} {bytebuf_patch_u64_be(b, bytebuf_len(b) - 8, UInt64(1))}\n");
    print(f"tail {hex(bytebuf_to_bytes(b))}\n");
    print(f"snapshot unchanged {hex(bytes_slice(snapshot, size_at, size_at + 4))}\n");
    let g = match bytebuf_get(b, 0) { Option.Some(value: v) => int_to_str(Int(v)), Option.None() => "none" };
    let past = match bytebuf_get(b, bytebuf_len(b)) { Option.Some(value: v) => int_to_str(Int(v)), Option.None() => "none" };
    print(f"get {g} {past} set {bytebuf_set(b, 1, UInt8(9))} {bytebuf_set(b, bytebuf_len(b), UInt8(9))} {hex(bytes_slice(bytebuf_to_bytes(b), 0, 3))}\n");
    let alias = b;
    bytebuf_clear(alias);
    bytebuf_push(b, UInt8(255));
    print(f"cleared {bytebuf_len(alias)} {hex(bytebuf_to_bytes(alias))}\n");
    // LEB128 at its edges.
    print(f"uleb {uleb(UInt64(0))} {uleb(UInt64(127))} {uleb(UInt64(128))} {uleb(UInt64(624485))} {uleb(UInt64(18446744073709551615))}\n");
    print(f"sleb {sleb(0)} {sleb(0 - 1)} {sleb(63)} {sleb(64)} {sleb(0 - 64)} {sleb(0 - 65)} {sleb(0 - 123456)} {sleb(9223372036854775807)} {sleb(0 - 9223372036854775807 - 1)}\n");
    // Against `@std/binary`'s writer, the same values in the same order.
    let seed = ints_new();
    ints_push(seed, 7);
    let bb = bytebuf_new();
    let w = binary_writer();
    for i in 0..600 {
        let x = next(seed) * 65536 + next(seed);
        let u = UInt64(x) * UInt64(next(seed) + 1);
        match next(seed) % 8 {
            0 => { bytebuf_push(bb, UInt8(x % 256)); binary_put_u8(w, UInt8(x % 256)); },
            1 => { bytebuf_put_u16_le(bb, UInt16(x % 65536)); binary_put_u16_le(w, UInt16(x % 65536)); },
            2 => { bytebuf_put_u16_be(bb, UInt16(x % 65536)); binary_put_u16_be(w, UInt16(x % 65536)); },
            3 => { bytebuf_put_u32_le(bb, UInt32(x)); binary_put_u32_le(w, UInt32(x)); },
            4 => { bytebuf_put_u32_be(bb, UInt32(x)); binary_put_u32_be(w, UInt32(x)); },
            5 => { bytebuf_put_u64_le(bb, u); binary_put_u64_le(w, u); },
            6 => { bytebuf_put_u64_be(bb, u); binary_put_u64_be(w, u); },
            _ => { bytebuf_put_uleb128(bb, u); binary_put_varint(w, u); },
        };
    }
    print(f"binary agrees {same_bytes(bytebuf_to_bytes(bb), binary_finish(w))} {bytebuf_len(bb)}\n");
    0
}

struct Version { major: Int, minor: Int, }

impl Ord for Version {
    fn compare(self: Version, other: Version) -> Int ! {} {
        if self.major != other.major { self.major.compare(other.major) } else { self.minor.compare(other.minor) }
    }
}

fn keys_text(ks: Vec[Str]) -> Str {
    let parts = strs_new();
    for i in 0..vec_len(ks) { strs_push(parts, vec_get(ks, i)); }
    str_join(parts, ",")
}

fn opt_str(o: Option[Str]) -> Str { match o { Option.Some(value: s) => s, Option.None() => "-" } }
fn opt_int(o: Option[Int]) -> Str { match o { Option.Some(value: n) => int_to_str(n), Option.None() => "-" } }

// Where `k` is in the sorted `keys`: its index, or `-(where it would go) - 1`.
fn ref_find(keys: Vec[Int], k: Int) -> Int {
    let mut i = 0;
    while i < vec_len(keys) && vec_get(keys, i) < k { i = i + 1; }
    if i < vec_len(keys) && vec_get(keys, i) == k { i } else { 0 - i - 1 }
}

fn ordmap_section(out: OutCap) -> Int ! {io} {
    let m = ordmap_new[Str, Int]();
    for w in text_split("pear,apple,fig,banana,app,cherry,date", ",") { ordmap_set(m, w, str_len(w)); }
    print(f"words {ordmap_len(m)} {keys_text(ordmap_keys(m))} replaced {ordmap_set(m, "fig", 30)} {ordmap_set(m, "kiwi", 4)}\n");
    print(f"first {opt_str(ordmap_first(m))} last {opt_str(ordmap_last(m))} floor {opt_str(ordmap_floor(m, "c"))} {opt_str(ordmap_floor(m, "a"))} ceiling {opt_str(ordmap_ceiling(m, "c"))} {opt_str(ordmap_ceiling(m, "z"))} {opt_str(ordmap_ceiling(m, "fig"))}\n");
    let r = ordmap_range(m, "b", "f");
    let parts = strs_new();
    for i in 0..vec_len(r) { let e = vec_get(r, i); strs_push(parts, f"{e.key}={e.value}"); }
    print(f"range {str_join(parts, ",")} has {ordmap_has(m, "date")} {ordmap_has(m, "dat")} get {opt_int(ordmap_get(m, "fig"))} {opt_int(ordmap_get(m, "zz"))}\n");
    print(f"remove {opt_int(ordmap_remove(m, "apple"))} {opt_int(ordmap_remove(m, "apple"))} {keys_text(ordmap_keys(m))}\n");
    let vs = ordmap_values(m);
    let vparts = strs_new();
    for i in 0..vec_len(vs) { strs_push(vparts, int_to_str(vec_get(vs, i))); }
    print(f"values {str_join(vparts, ",")}\n");
    let e = ordmap_new[Str, Int]();
    print(f"empty {ordmap_len(e)} {opt_str(ordmap_first(e))} {opt_str(ordmap_last(e))} {vec_len(ordmap_entries(e))} {opt_int(ordmap_remove(e, "x"))}\n");
    let vm = ordmap_new[Version, Str]();
    ordmap_set(vm, Version(major: 1, minor: 10), "c");
    ordmap_set(vm, Version(major: 0, minor: 9), "a");
    ordmap_set(vm, Version(major: 1, minor: 2), "b");
    let ves = ordmap_entries(vm);
    let vp = strs_new();
    for i in 0..vec_len(ves) { let x = vec_get(ves, i); strs_push(vp, f"{x.key.major}.{x.key.minor}={x.value}"); }
    print(f"versions {str_join(vp, ",")}\n");
    // Against a sorted vector, under a fixed-seed sequence of insertions, removals and lookups.
    let seed = ints_new();
    ints_push(seed, 42);
    let om = ordmap_new[Int, Int]();
    let rk = vec_new[Int]();
    let rv = vec_new[Int]();
    let mut bad = 0;
    let mut checks = 0;
    for step in 0..3000 {
        let k = next(seed) % 400 - 100;
        let op = next(seed) % 10;
        if op < 5 {
            let at = ref_find(rk, k);
            let replaced = ordmap_set(om, k, step);
            if replaced != (at >= 0) { bad = bad + 1; }
            if at >= 0 { vec_set(rv, at, step); } else {
                let place = 0 - at - 1;
                vec_push(rk, k);
                vec_push(rv, step);
                let mut i = vec_len(rk) - 1;
                while i > place { vec_set(rk, i, vec_get(rk, i - 1)); vec_set(rv, i, vec_get(rv, i - 1)); i = i - 1; }
                vec_set(rk, place, k);
                vec_set(rv, place, step);
            }
        } else {
            if op < 8 {
                let at = ref_find(rk, k);
                let got = ordmap_remove(om, k);
                let want = if at >= 0 { Option[Int].Some(value: vec_get(rv, at)) } else { Option[Int].None() };
                if opt_int(got) != opt_int(want) { bad = bad + 1; }
                if at >= 0 {
                    let mut i = at;
                    while i + 1 < vec_len(rk) { vec_set(rk, i, vec_get(rk, i + 1)); vec_set(rv, i, vec_get(rv, i + 1)); i = i + 1; }
                    let a = vec_pop(rk);
                    let b = vec_pop(rv);
                }
            } else {
                let at = ref_find(rk, k);
                let want = if at >= 0 { Option[Int].Some(value: vec_get(rv, at)) } else { Option[Int].None() };
                if opt_int(ordmap_get(om, k)) != opt_int(want) || ordmap_has(om, k) != (at >= 0) { bad = bad + 1; }
                // The neighbours of `k`.
                let fl = if at >= 0 { Option[Int].Some(value: k) } else { if 0 - at - 1 > 0 { Option[Int].Some(value: vec_get(rk, 0 - at - 2)) } else { Option[Int].None() } };
                let ce = if at >= 0 { Option[Int].Some(value: k) } else { if 0 - at - 1 < vec_len(rk) { Option[Int].Some(value: vec_get(rk, 0 - at - 1)) } else { Option[Int].None() } };
                if opt_int(ordmap_floor(om, k)) != opt_int(fl) || opt_int(ordmap_ceiling(om, k)) != opt_int(ce) { bad = bad + 1; }
            }
        }
        checks = checks + 1;
        if step % 100 == 99 {
            // The whole map, its ends and a range.
            let ks = ordmap_keys(om);
            let vs2 = ordmap_values(om);
            let mut whole = vec_len(ks) == vec_len(rk) && ordmap_len(om) == vec_len(rk);
            if whole { for i in 0..vec_len(ks) { if vec_get(ks, i) != vec_get(rk, i) || vec_get(vs2, i) != vec_get(rv, i) { whole = false; } } }
            if !whole { bad = bad + 1; }
            let lo = next(seed) % 400 - 100;
            let hi = lo + next(seed) % 120;
            let got = ordmap_range(om, lo, hi);
            let want = vec_new[Int]();
            for i in 0..vec_len(rk) { if vec_get(rk, i) >= lo && vec_get(rk, i) < hi { vec_push(want, vec_get(rk, i)); } }
            let mut same = vec_len(got) == vec_len(want);
            if same { for i in 0..vec_len(got) { if vec_get(got, i).key != vec_get(want, i) { same = false; } } }
            if !same { bad = bad + 1; }
            let f = if vec_len(rk) > 0 { Option[Int].Some(value: vec_get(rk, 0)) } else { Option[Int].None() };
            let l = if vec_len(rk) > 0 { Option[Int].Some(value: vec_get(rk, vec_len(rk) - 1)) } else { Option[Int].None() };
            if opt_int(ordmap_first(om)) != opt_int(f) || opt_int(ordmap_last(om)) != opt_int(l) { bad = bad + 1; }
            checks = checks + 1;
        }
    }
    // Ascending and descending runs, then emptied: the shapes that unbalance a plain tree.
    let big = ordmap_new[Int, Int]();
    for i in 0..2000 { ordmap_set(big, i, i); }
    for i in 0..2000 { ordmap_set(big, 0 - i - 1, i); }
    let mut sorted = true;
    let bk = ordmap_keys(big);
    for i in 1..vec_len(bk) { if vec_get(bk, i - 1) >= vec_get(bk, i) { sorted = false; } }
    for i in 0..4000 { ordmap_remove(big, i - 2000); }
    print(f"ordmap differential {checks} checks, {bad} wrong, {vec_len(rk)} left; runs {vec_len(bk)} sorted {sorted}, emptied {ordmap_len(big)}\n");
    0
}

fn bitset_section(out: OutCap) -> Int ! {io} {
    let a = bitset_new();
    bitset_add(a, 3);
    bitset_add(a, 64);
    bitset_add(a, 200);
    print(f"bitset {bitset_add(a, 3)} {bitset_add(a, 0 - 1)} {bitset_count(a)} {bitset_has(a, 64)} {bitset_has(a, 65)} {bitset_has(a, 0 - 1)} {bitset_has(a, 100000)}\n");
    let b = bitset_new();
    bitset_add(b, 64);
    bitset_add(b, 5);
    bitset_add(b, 1000);
    print(f"union {fmt(bitset_items(bitset_union(a, b)))} meet {fmt(bitset_items(bitset_intersection(a, b)))} minus {fmt(bitset_items(bitset_difference(a, b)))}\n");
    print(f"next {opt_int(bitset_next(a, 0 - 5))} {opt_int(bitset_next(a, 4))} {opt_int(bitset_next(a, 64))} {opt_int(bitset_next(a, 65))} {opt_int(bitset_next(a, 201))}\n");
    print(f"remove {bitset_remove(a, 64)} {bitset_remove(a, 64)} {bitset_remove(a, 99999)} equal {bitset_equal(a, b)} {bitset_equal(bitset_union(a, a), a)}\n");
    bitset_remove(b, 1000);
    let c = bitset_new();
    bitset_add(c, 5);
    bitset_add(c, 64);
    print(f"trailing words {bitset_equal(b, c)} {bitset_equal(c, b)} clear {bitset_clear(b)} {bitset_count(b)} {bitset_equal(b, bitset_new())}\n");
    // Against a vector of flags.
    let seed = ints_new();
    ints_push(seed, 99);
    let s = bitset_new();
    let t = bitset_new();
    let fs = vec_new[Bool]();
    let ft = vec_new[Bool]();
    for i in 0..300 { vec_push(fs, false); vec_push(ft, false); }
    let mut bad = 0;
    for step in 0..2000 {
        let i = next(seed) % 300;
        let op = next(seed) % 4;
        if op == 0 { if bitset_add(s, i) == vec_get(fs, i) { bad = bad + 1; } vec_set(fs, i, true); }
        if op == 1 { if bitset_remove(s, i) != vec_get(fs, i) { bad = bad + 1; } vec_set(fs, i, false); }
        if op == 2 { bitset_add(t, i); vec_set(ft, i, true); }
        if op == 3 { if bitset_has(s, i) != vec_get(fs, i) { bad = bad + 1; } }
        if step % 200 == 199 {
            let u = bitset_items(bitset_union(s, t));
            let m = bitset_items(bitset_intersection(s, t));
            let d = bitset_items(bitset_difference(s, t));
            let wu = ints_new();
            let wm = ints_new();
            let wd = ints_new();
            let ws = ints_new();
            for j in 0..300 {
                let x = vec_get(fs, j);
                let y = vec_get(ft, j);
                if x || y { ints_push(wu, j); }
                if x && y { ints_push(wm, j); }
                if x && !y { ints_push(wd, j); }
                if x { ints_push(ws, j); }
            }
            // Walking by `bitset_next` finds what `bitset_items` lists.
            let walked = ints_new();
            let mut from = 0;
            let mut going = true;
            while going {
                match bitset_next(s, from) {
                    Option.Some(value: k) => { ints_push(walked, k); from = k + 1; },
                    Option.None() => { going = false; },
                };
            }
            if fmt(u) != fmt(wu) || fmt(m) != fmt(wm) || fmt(d) != fmt(wd) || fmt(walked) != fmt(ws) || bitset_count(s) != ints_len(ws) { bad = bad + 1; }
        }
    }
    print(f"bitset differential {bad} wrong, {bitset_count(s)} held\n");
    0
}

fn fmt(xs: Ints) -> Str {
    let parts = strs_new();
    for i in 0..ints_len(xs) { strs_push(parts, int_to_str(ints_get(xs, i))); }
    str_join(parts, ",")
}

struct Pair { key: Int, at: Int, }

fn sort_section(out: OutCap) -> Int ! {io} {
    let seed = ints_new();
    ints_push(seed, 5);
    let mut bad = 0;
    for n in text_ints("0,1,2,15,16,17,31,33,64,100,257,1000") {
        let v = vec_new[Pair]();
        for i in 0..n { vec_push(v, Pair(key: next(seed) % 10, at: i)); }
        let want = vec_sort_by(v, fn(a: Pair, b: Pair) -> Bool { a.key < b.key });
        let alias = v;
        vec_sort_in_place_by(v, fn(a: Pair, b: Pair) -> Bool { a.key < b.key });
        for i in 0..n {
            // Equal keys keep their order: stability, checked by the original positions.
            if vec_get(alias, i).key != vec_get(want, i).key || vec_get(alias, i).at != vec_get(want, i).at { bad = bad + 1; }
        }
        let xs = ints_new();
        for i in 0..n { ints_push(xs, next(seed) % 1000 - 500); }
        let wx = ints_sort(xs);
        ints_sort_in_place(xs);
        if fmt(xs) != fmt(wx) { bad = bad + 1; }
        let ss = strs_new();
        for i in 0..n { strs_push(ss, str_slice("zyxwvutsrqponmlkjihgfedcba", next(seed) % 26, 26)); }
        let ws = strs_sort(ss);
        strs_sort_in_place(ss);
        for i in 0..n { if strs_get(ss, i) != strs_get(ws, i) { bad = bad + 1; } }
    }
    let mut calls = 0;
    let d = vec_new[Int]();
    for i in 0..40 { vec_push(d, 40 - i); }
    vec_sort_in_place_by(d, fn(a: Int, b: Int) -> Bool { calls = calls + 1; a > b });
    let small = ints_new();
    for x in text_ints("5,-3,9,0,9,1") { ints_push(small, x); }
    ints_sort_in_place(small);
    print(f"sort {bad} wrong; descending {vec_get(d, 0)}..{vec_get(d, 39)} in {calls} calls; {fmt(small)}\n");
    0
}

fn main(out: OutCap) -> Int ! {io} {
    bytebuf_section(out);
    ordmap_section(out);
    bitset_section(out);
    sort_section(out);
    0
}
"#####;

const EXPECTED: &str = r#####"built 33 07020101020403020101020304080706050403020101020304050607086869213f
aligned 48 48 48
patched true false false true
tail 07020101020403020101020304080706050403020101020304050607086869213f300000000000000000000000000001
snapshot unchanged 00000000
get 7 none set true false 070901
cleared 1 ff
uleb 00 7f 8001 e58e26 ffffffffffffffffff01
sleb 00 7f 3f c000 40 bf7f c0bb78 ffffffffffffffffff00 8080808080808080807f
binary agrees true 2711
words 7 app,apple,banana,cherry,date,fig,pear replaced true false
first app last pear floor banana - ceiling cherry - fig
range banana=6,cherry=6,date=4 has true false get 30 -
remove 5 - app,banana,cherry,date,fig,kiwi,pear
values 3,6,6,4,30,4,4
empty 0 - - 0 -
versions 0.9=a,1.2=b,1.10=c
ordmap differential 3030 checks, 0 wrong, 240 left; runs 4000 sorted true, emptied 0
bitset false false 3 true false false false
union 3,5,64,200,1000 meet 64 minus 3,200
next 3 64 64 200 -
remove true false false equal false true
trailing words true true clear 0 0 true
bitset differential 0 wrong, 139 held
sort 0 wrong; descending 40..1 in 85 calls; -3,0,1,5,9,9
0
"#####;

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

#[test]
fn bytebuf_ordmap_bitset_and_in_place_sort_run_alike_everywhere_with_nothing_live() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        runs.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        runs.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe)
                    .current_dir(d.path())
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, r) in runs {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, EXPECTED, "{label}");
        if label != "lir oracle" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {r:?}"));
            assert!(
                line.split_whitespace()
                    .filter(|w| w.starts_with("live="))
                    .all(|w| w == "live=0"),
                "{label}: {line}"
            );
        }
    }
}

const ASCENDING: &str = r#####"use "@std/ordmap";

fn main(out: OutCap) -> Int ! {io} {
    let m = ordmap_new[Int, Int]();
    for i in 0..300000 { ordmap_set(m, i, i * 2); }
    for i in 0..150000 { ordmap_remove(m, i * 2); }
    for i in 0..1000 { ordmap_set(m, 0 - i, i); }
    let mut sum = 0;
    let vs = ordmap_values(m);
    for i in 0..vec_len(vs) { sum = sum + vec_get(vs, i); }
    print(f"{ordmap_len(m)} {sum}\n");
    0
}
"#####;

#[test]
fn an_ordmap_stays_balanced_under_ascending_keys() {
    // Three hundred thousand ascending keys, half removed, natively: a tree that did not rebalance
    // would recurse that deep and compare quadratically often; this one finishes well within the
    // limit.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), ASCENDING).unwrap();
    let exe = d.path().join("ascending");
    let b = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "--opt-level",
            "2",
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ],
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let r = common::run_within(
        common::bounded_exe(&exe).current_dir(d.path()),
        std::time::Duration::from_secs(20),
    )
    .expect("an ordmap of ascending keys did not finish within 20 s");
    assert_eq!(r.code, Some(0), "{r:?}");
    // 150,000 odd keys valued twice themselves, and 1,000 non-positive ones valued 0..999.
    assert_eq!(r.stdout, "151000 45000499500\n0\n");
}

#[test]
fn a_key_type_without_ord_is_refused() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "use \"@std/ordmap\";\nstruct P { x: Int, }\nfn main() -> Int { let m = ordmap_new[P, Int](); ordmap_len(m) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "prog.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("Ord"), "{}", r.stderr);
}
