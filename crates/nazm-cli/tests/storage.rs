//! Gate 3: the storage engine reference workload (`examples/apps/pagestore/`,
//! `docs/systems-domains.md` Part M).
//!
//! `pagestore` is a Nazm program: fixed 256-byte checksummed pages written in place, behind a redo
//! log of whole page images. These tests build it once and then crash a run at **every** write
//! boundary it has — each crash writing half of that write, as a power cut would — and check what a
//! recovering open finds against a model of every acknowledged write: each one there, with its last
//! acknowledged value, and nothing else but possibly the one write in flight. Then the store is
//! held by one process at a time, reopened with what it had, and served to concurrent readers and
//! one writer through channels. No database-maturity claim: one page size, sixteen pages, `Int`
//! keys and values.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The engine, built once into `dir`.
fn engine(dir: &Path) -> PathBuf {
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/pagestore/pagestore.nz");
    let out = dir.join(crate::common::exe_name("pagestore"));
    let r = run_bounded(
        Command::new(NAZM)
            .arg("build")
            .arg(&src)
            .arg("-o")
            .arg(&out),
        "nazm build",
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    out
}

fn run(exe: &Path, store: &Path, script: &Path, extra: &[&str]) -> Ran {
    run_bounded(
        Command::new(exe).arg(store).arg(script).args(extra),
        "pagestore",
    )
}

/// The store's contents, from a `dump` line.
fn contents(dump: &str) -> BTreeMap<i64, i64> {
    dump.trim()
        .strip_prefix("dump ->")
        .unwrap_or_else(|| panic!("not a dump: {dump}"))
        .split_whitespace()
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap();
            (k.parse().unwrap(), v.parse().unwrap())
        })
        .collect()
}

/// Puts over several pages, overwrites, checkpoints mid-way and a reopen: every kind of write the
/// engine makes.
const SCRIPT: &str = "put 1 10\nput 17 170\nput 2 20\nput 1 11\ncheckpoint\nput 33 330\n\
put 18 180\nreopen\nput 2 21\nput 49 490\nput 1 12\ncheckpoint\nput 65 650\nput 3 30\n";

#[test]
fn a_crash_at_every_write_boundary_recovers_every_acknowledged_write() {
    let d = tempfile::tempdir().unwrap();
    let exe = engine(d.path());
    let script = d.path().join("script.txt");
    std::fs::write(&script, SCRIPT).unwrap();
    let dump = d.path().join("dump.txt");
    std::fs::write(&dump, "dump\n").unwrap();
    // The writes an uncrashed run makes: every boundary there is.
    let clean = d.path().join("clean");
    std::fs::create_dir(&clean).unwrap();
    let r = run(&exe, &clean, &script, &[]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let total: usize = r
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("writes "))
        .unwrap()
        .parse()
        .unwrap();
    assert!(total > 20, "{}", r.stdout);
    let puts: Vec<(i64, i64)> = SCRIPT
        .lines()
        .filter_map(|l| l.strip_prefix("put "))
        .map(|kv| {
            let (k, v) = kv.split_once(' ').unwrap();
            (k.parse().unwrap(), v.parse().unwrap())
        })
        .collect();
    let mut torn_pages_repaired = 0;
    for crash in 1..=total {
        let store = d.path().join(format!("crash{crash}"));
        std::fs::create_dir(&store).unwrap();
        let r = run(&exe, &store, &script, &[&crash.to_string()]);
        assert_eq!(r.code, Some(3), "boundary {crash}: {r:?}");
        assert!(
            r.stderr.contains(&format!("crashed at write {crash}")),
            "{}",
            r.stderr
        );
        let acked = r.stdout.lines().filter(|l| l.ends_with(" -> ok")).count();
        // A crash leaves the lock: a plain open refuses, and `recover` breaks it.
        let refused = run(&exe, &store, &dump, &[]);
        assert_eq!(refused.code, Some(1), "boundary {crash}");
        assert!(refused.stderr.contains("LOCK"), "{}", refused.stderr);
        let back = run(&exe, &store, &dump, &["0", "recover"]);
        assert_eq!(back.code, Some(0), "boundary {crash}: {back:?}");
        let got = contents(back.stdout.lines().next().unwrap());
        // The model: every acknowledged put, in order; then, possibly, the one in flight.
        let mut model: BTreeMap<i64, i64> = BTreeMap::new();
        for (k, v) in &puts[..acked] {
            model.insert(*k, *v);
        }
        let mut with_next = model.clone();
        if let Some((k, v)) = puts.get(acked) {
            with_next.insert(*k, *v);
        }
        assert!(
            got == model || got == with_next,
            "boundary {crash}: {acked} acknowledged, found {got:?}, wanted {model:?} or {with_next:?}"
        );
        if got == with_next && got != model {
            torn_pages_repaired += 1;
        }
    }
    // Some crashes tore a page after its record was in the log: the log made it whole.
    assert!(
        torn_pages_repaired > 0,
        "no crash fell between a record and its page"
    );
}

#[test]
fn the_store_is_one_processs_and_a_reopen_finds_what_it_held() {
    let d = tempfile::tempdir().unwrap();
    let exe = engine(d.path());
    let store = d.path().join("s");
    std::fs::create_dir(&store).unwrap();
    let script = d.path().join("script.txt");
    std::fs::write(
        &script,
        "put 5 50\nput 21 210\nreopen\nget 5\nget 21\nget 6\ndump\n",
    )
    .unwrap();
    let r = run(&exe, &store, &script, &[]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert!(
        r.stdout
            .contains("get 5 -> 50\nget 21 -> 210\nget 6 -> (none)\n"),
        "{}",
        r.stdout
    );
    // A second process, the store held: refused by its lock.
    std::fs::write(store.join("LOCK"), "").unwrap();
    let held = run(&exe, &store, &script, &[]);
    assert_eq!(held.code, Some(1));
    assert!(
        held.stderr.contains("held by another process"),
        "{}",
        held.stderr
    );
    std::fs::remove_file(store.join("LOCK")).unwrap();
    // The fixed layout, read without the engine: page 5's first slot holds key 5 and value 50.
    let pages = std::fs::read(store.join("pages")).unwrap();
    assert_eq!(pages.len(), 16 * 256);
    let at = |o: usize| i64::from_le_bytes(pages[o..o + 8].try_into().unwrap());
    assert_eq!(
        (at(5 * 256), at(5 * 256 + 8)),
        (5, 2),
        "page 5: its number, two keys"
    );
    assert_eq!((at(5 * 256 + 16), at(5 * 256 + 24)), (5, 50));
    assert_eq!((at(5 * 256 + 32), at(5 * 256 + 40)), (21, 210));
    // A closed store's log is empty: everything is in the pages.
    assert_eq!(std::fs::metadata(store.join("wal")).unwrap().len(), 0);
}

#[test]
fn readers_and_one_writer_share_the_store_through_channels() {
    let d = tempfile::tempdir().unwrap();
    let exe = engine(d.path());
    let store = d.path().join("s");
    std::fs::create_dir(&store).unwrap();
    let script = d.path().join("script.txt");
    std::fs::write(&script, "serve 4 25\nget 24\ndump\n").unwrap();
    let r = run(&exe, &store, &script, &[]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert!(
        r.stdout
            .contains("serve 4 25 -> served 25 puts and 100 gets"),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("get 24 -> 240"), "{}", r.stdout);
    let dump = r.stdout.lines().find(|l| l.starts_with("dump")).unwrap();
    let got = contents(dump);
    let want: BTreeMap<i64, i64> = (0..25).map(|i| (i, i * 10)).collect();
    assert_eq!(got, want);
}

/// A whole record whose checksum does not hold is not replayed, whatever it carries: here a sealed
/// image of another page, which, applied, would change the store without a word. A crash cuts a
/// record short; this is the record a crash cannot make and a bit flip can.
#[test]
fn a_whole_record_whose_checksum_fails_is_not_replayed() {
    let d = tempfile::tempdir().unwrap();
    let exe = engine(d.path());
    let store = d.path().join("s");
    std::fs::create_dir(&store).unwrap();
    let script = d.path().join("script.txt");
    std::fs::write(&script, "put 1 10\nput 2 20\n").unwrap();
    let dump = d.path().join("dump.txt");
    std::fs::write(&dump, "dump\n").unwrap();
    let r = run(&exe, &store, &script, &[]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    // Page 2's sealed image, offered as page 1's next version, its checksum wrong.
    let pages = std::fs::read(store.join("pages")).unwrap();
    let mut record = Vec::new();
    record.extend_from_slice(&5_638_093_645_399_082_316_i64.to_le_bytes());
    record.extend_from_slice(&1_i64.to_le_bytes());
    record.extend_from_slice(&1_i64.to_le_bytes());
    record.extend_from_slice(&pages[2 * 256..3 * 256]);
    record.extend_from_slice(&0_i64.to_le_bytes());
    assert_eq!(record.len(), 288);
    std::fs::write(store.join("wal"), &record).unwrap();
    let r = run(&exe, &store, &dump, &[]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(
        contents(r.stdout.lines().next().unwrap()),
        [(1, 10), (2, 20)].into_iter().collect()
    );
}
