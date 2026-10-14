//! The entry `nazm test` and `nazm bench` run a marked function through (Gate 2, §18–§19).
//!
//! A `@test`, `@fuzz` or `@bench` function is not a program: something has to hold the authority
//! it asks for, call it, and turn what it answers into a process status. That something is a
//! `main` written here and appended to the entry file's text as the loader reads it — never to the
//! file on disk. The file's own `main`, if it has one, is renamed in place to a name of the same
//! length, so every offset before the appended text is unchanged and every diagnostic about the
//! program lands where it did. The harness then runs as any program does: under `nazm run` and as
//! a `nazm build` executable, each with its own process, so a trap fails one test and nothing else.
//!
//! The status is the answer: `0` when the function answered `Ok` (for a fuzz target, for every
//! input), and `2` — a trap's status — with its `Err` printed when it did not, or when it trapped;
//! a program that does not check is `1` under `nazm run` and no executable at all. A
//! benchmark prints `nazm-bench: NANOSECONDS SINK` for its iterations together, `SINK` every answer
//! folded with `^`, so no answer is unused and the work cannot be optimised away.

use std::path::{Path, PathBuf};

use nazm_service::load::{self, Loaded, Sources};

/// What the generated `main` does with the function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Call it once.
    Test,
    /// Call it with this many generated inputs.
    Fuzz(u32),
    /// Call it this many times, timed.
    Bench(u32),
}

/// A marked function and how to run it: the hidden `--harness` argument of `run` and `build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Harness {
    pub(crate) kind: Kind,
    pub(crate) function: String,
}

impl Harness {
    /// `test:NAME`, `fuzz:NAME:CASES` or `bench:NAME:ITERATIONS`.
    pub(crate) fn parse(spec: &str) -> Option<Self> {
        let mut parts = spec.splitn(3, ':');
        let kind = parts.next()?;
        let function = parts.next()?.to_owned();
        let count = parts.next().map(str::parse::<u32>);
        let kind = match (kind, count) {
            ("test", None) => Kind::Test,
            ("fuzz", Some(Ok(n))) => Kind::Fuzz(n),
            ("bench", Some(Ok(n))) => Kind::Bench(n),
            _ => return None,
        };
        let plain = !function.is_empty()
            && function
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_');
        plain.then_some(Self { kind, function })
    }

    /// The argument [`Self::parse`] reads back.
    pub(crate) fn spec(&self) -> String {
        match self.kind {
            Kind::Test => format!("test:{}", self.function),
            Kind::Fuzz(n) => format!("fuzz:{}:{n}", self.function),
            Kind::Bench(n) => format!("bench:{}:{n}", self.function),
        }
    }
}

/// The hidden `--harness` argument, read; a usage error when it is malformed.
pub(crate) fn parsed(spec: Option<&str>) -> Result<Option<Harness>, std::process::ExitCode> {
    match spec {
        None => Ok(None),
        Some(s) => Harness::parse(s).map(Some).ok_or_else(|| {
            eprintln!("nazm: `--harness {s}` is not `test:NAME`, `fuzz:NAME:CASES` or `bench:NAME:ITERATIONS`");
            std::process::ExitCode::from(crate::exit::USAGE)
        }),
    }
}

/// A function a file marks to be run: its name, its attribute, and whether `@test(fails)` said it
/// passes by failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marked {
    pub(crate) function: String,
    pub(crate) attribute: &'static str,
    pub(crate) fails: bool,
}

/// Every top-level function the file at `path` marks `@test`, `@fuzz` or `@bench`, in source
/// order, among the declarations this host builds with its package's default features — one whose
/// `@cfg` does not hold is not run.
pub(crate) fn marked(path: &Path, text: &str) -> Vec<Marked> {
    let (program, _) = nazm_core::__syntax::parse_source(text);
    let config = crate::profile::configuration(path, None, None, &[])
        .unwrap_or_else(|_| nazm_core::sema::BuildConfig::host());
    let program = nazm_core::check::attributes::configure(
        program,
        &nazm_core::sema::ModuleGraph::default().with_config(config),
        &mut Vec::new(),
    );
    let mut out = Vec::new();
    for f in program.functions.iter().filter(|f| f.method_of.is_none()) {
        for a in &f.attrs {
            let attribute = match a.name.as_str() {
                "test" => "test",
                "fuzz" => "fuzz",
                "bench" => "bench",
                _ => continue,
            };
            out.push(Marked {
                function: f.name.clone(),
                attribute,
                fails: attribute == "test" && a.args.as_ref().is_some_and(|x| !x.is_empty()),
            });
        }
    }
    out
}

/// What the entry file is read as: its text with its `main` blanked and the harness appended.
///
/// # Errors
///
/// The function is not a top-level function of the file.
pub(crate) fn text(original: &str, h: &Harness) -> Result<String, String> {
    let (program, _) = nazm_core::__syntax::parse_source(original);
    let target = program
        .functions
        .iter()
        .find(|f| f.method_of.is_none() && f.name == h.function)
        .ok_or_else(|| format!("no top-level function `{}` to run", h.function))?;
    let skip = usize::from(matches!(h.kind, Kind::Fuzz(_)));
    let held: Vec<(String, String)> = target
        .params
        .iter()
        .skip(skip)
        .enumerate()
        .map(|(i, p)| (format!("nz_harness_{i}"), p.ty.name.clone()))
        .collect();

    let mut text = original.to_owned();
    // The file's own `main` is not part of what the harness builds: its text is read as blanks,
    // its lines kept, so every offset after it is where it was. Moving it aside under another
    // name would leave a function no longer `main`, without the `out` `main` is given (R1, 5b.1).
    if let Some(main) = program
        .functions
        .iter()
        .find(|f| f.method_of.is_none() && f.name == "main")
    {
        let start = main
            .attrs
            .iter()
            .map(|a| a.span.start)
            .chain([main.span.start])
            .min()
            .unwrap_or(main.span.start) as usize;
        let end = main.span.end as usize;
        let blank: String = original
            .get(start..end)
            .ok_or("this file's `main` is not where it was parsed")?
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        text.replace_range(start..end, &blank);
    }

    let params: String = held
        .iter()
        .map(|(name, ty)| format!(", {name}: {ty}"))
        .collect();
    let args: String = held.iter().map(|(name, _)| format!(", {name}")).collect();
    let args_alone = args.trim_start_matches(", ");
    let name = &h.function;
    let failed = "Result.Err(error: e) => { print(str_concat(e, \"\\n\")); exit_with(2) }";
    let main = match h.kind {
        Kind::Test => format!(
            "fn main(nz_harness_io: IoCap, nz_harness_out: OutCap{params}) -> Int {{\n    \
             match {name}({args_alone}) {{\n        Result.Ok(value: v) => 0,\n        {failed}\n    }}\n}}\n"
        ),
        Kind::Fuzz(cases) => format!(
            "fn nz_harness_next(s: UInt64) -> UInt64 {{\n    \
             let a = s ^ (s << 13);\n    let b = a ^ (a >> 7);\n    b ^ (b << 17)\n}}\n\n\
             fn main(nz_harness_io: IoCap, nz_harness_out: OutCap{params}) -> Int {{\n    \
             let mut state = UInt64(88172645463325252);\n    let mut case = 0;\n    \
             while case < {cases} {{\n        \
             state = nz_harness_next(state);\n        \
             let len = Int(state % UInt64(65));\n        \
             let input = bytes_new(len);\n        \
             let mut i = 0;\n        \
             while i < len {{\n            \
             state = nz_harness_next(state);\n            \
             bytes_set(input, i, UInt8(state & UInt64(255)));\n            \
             i = i + 1;\n        }}\n        \
             match {name}(input{args}) {{\n            \
             Result.Ok(value: v) => 0,\n            \
             Result.Err(error: e) => {{ print(str_concat(str_concat(str_concat(\"case \", int_to_str(case)), \": \"), str_concat(e, \"\\n\"))); exit_with(2) }}\n        }};\n        \
             case = case + 1;\n    }}\n    0\n}}\n"
        ),
        Kind::Bench(iterations) => format!(
            "fn main(nz_harness_io: IoCap, nz_harness_out: OutCap, nz_harness_time: TimeCap{params}) -> Int {{\n    \
             let start = time_now_ns();\n    let mut i = 0;\n    let mut sink = 0;\n    \
             while i < {iterations} {{\n        sink = sink ^ {name}({args_alone});\n        i = i + 1;\n    }}\n    \
             let took = time_now_ns() - start;\n    \
             print(str_concat(str_concat(str_concat(\"nazm-bench: \", int_to_str(took)), str_concat(\" \", int_to_str(sink))), \"\\n\"));\n    0\n}}\n"
        ),
    };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('\n');
    text.push_str(&main);
    Ok(text)
}

/// The entry file served as [`text`] makes it; every other file as it is on disk.
struct Served {
    entry: PathBuf,
    text: String,
}

impl Sources for Served {
    fn read(&self, path: &Path) -> std::io::Result<String> {
        if same(path, &self.entry) {
            return Ok(self.text.clone());
        }
        load::Disk.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        load::Disk.exists(path)
    }
}

fn same(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// [`load::load`], with the entry read through the harness when there is one.
///
/// # Errors
///
/// As [`load::load`]; a harness whose function the entry does not declare is a load error at
/// the entry, as a missing file is.
pub(crate) fn loaded(
    entry: &Path,
    source_root: &Path,
    harness: Option<&Harness>,
) -> Result<Loaded, (Loaded, Vec<nazm_diag::Diagnostic>)> {
    let Some(h) = harness else {
        return load::load(entry, source_root);
    };
    let original = std::fs::read_to_string(entry).unwrap_or_default();
    match text(&original, h) {
        Ok(text) => load::load_from(
            entry,
            source_root,
            &Served {
                entry: entry.to_path_buf(),
                text,
            },
        ),
        Err(why) => {
            let partial = Loaded {
                map: nazm_span::SourceMap::from_parts(Vec::new()),
                graph: nazm_core::sema::ModuleGraph::default(),
            };
            let d = nazm_diag::Diagnostic::error(
                nazm_diag::codes::ATTRIBUTE,
                nazm_span::Span::synthetic(),
                why,
            )
            .help("`nazm test` and `nazm bench` run a `@test`, `@fuzz` or `@bench` function the file declares");
            Err((partial, vec![d]))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_harness_argument_reads_back() {
        for spec in ["test:t", "fuzz:f_1:100", "bench:b:5"] {
            assert_eq!(
                Harness::parse(spec).map(|h| h.spec()).as_deref(),
                Some(spec)
            );
        }
        for bad in [
            "test",
            "test:",
            "fuzz:f",
            "bench:b:x",
            "test:a b",
            "other:t",
            "test:t:1",
        ] {
            assert_eq!(Harness::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn main_is_blanked_without_moving_an_offset() {
        // An option-C `main` (R1, 5b.1): blanked, not renamed, so its implicit `out` is not lost.
        let src = "fn helper() -> Int { 1 }\nfn main() {\n    println(helper())\n}\n@test\nfn t() -> Result[Int, Str] { Ok(0) }\n";
        let h = Harness::parse("test:t").unwrap();
        let out = text(src, &h).unwrap();
        let (kept, appended) = out.split_at(src.len());
        assert_eq!(&kept[..25], &src[..25]);
        assert!(!kept.contains("main") && !kept.contains("println"), "{out}");
        assert_eq!(kept.len(), src.len());
        assert_eq!(kept.lines().count(), src.lines().count());
        assert!(kept.contains("@test\nfn t()"), "{out}");
        assert!(appended.contains("match t()"), "{out}");
    }
}
