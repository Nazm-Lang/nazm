//! The memory report a run prints with `NAZM_MEMORY_REPORT` set, read into numbers: what each
//! class of storage allocated, reclaimed and left live.

// Each test binary that includes this module uses only part of it.
#![allow(dead_code)]

/// What one class of storage did in one run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Class {
    pub(crate) allocated: u64,
    pub(crate) reclaimed: u64,
    pub(crate) live: u64,
}

/// What one run reported: its value, and what it did with each class of storage.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ran {
    pub(crate) value: String,
    pub(crate) sequences: Class,
    pub(crate) strings: Class,
    pub(crate) channels: Class,
}

/// Read `<class> allocated=… reclaimed=… live=…`, three times, in order.
///
/// Positional rather than by-name search, because the three classes use the same three
/// field names — the class word is what tells them apart, and a parser that looked for
/// the first `allocated=` would answer every question with the sequence count.
pub(crate) fn counts(stderr: &str, value: String) -> Ran {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report in: {stderr}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    let class = |name: &str| {
        let at = words
            .iter()
            .position(|w| *w == name)
            .unwrap_or_else(|| panic!("no `{name}` in: {line}"));
        let field = |offset: usize, prefix: &str| {
            words
                .get(at + offset)
                .and_then(|w| w.strip_prefix(prefix))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("no `{prefix}` after `{name}` in: {line}"))
        };
        Class {
            allocated: field(1, "allocated="),
            reclaimed: field(2, "reclaimed="),
            live: field(3, "live="),
        }
    };
    Ran {
        value,
        sequences: class("sequences"),
        strings: class("strings"),
        channels: class("channels"),
    }
}
