//! The language service, driven directly: no protocol, no process, no JSON. N16.
//!
//! What an editor relies on, each stated as a test against the compiler's own answers:
//! open buffers are the source, the module graph is the loader's, definitions follow the
//! resolution's identities, and nothing is ever answered from a version that is not the
//! current one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_diag::codes;
use nazm_service::{Definition, LanguageService, Published, Target};

/// Run `f` on the stack the service documents as its requirement.
fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join")
    })
}

struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, text) in files {
            let p = dir.path().join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, text).unwrap();
        }
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.canonicalize().ok() == b.canonicalize().ok()
}

/// The published set for `path` in one refresh.
fn for_path<'a>(out: &'a [Published], path: &Path) -> &'a Published {
    out.iter()
        .find(|p| same_file(&p.path, path))
        .unwrap_or_else(|| panic!("nothing published for {}: {out:#?}", path.display()))
}

fn codes_of(p: &Published) -> Vec<String> {
    p.diagnostics.iter().map(|d| d.code.to_string()).collect()
}

/// The byte offset of the `n`th occurrence of `needle` in `text`, plus `into` bytes.
fn at(text: &str, needle: &str, n: usize, into: usize) -> u32 {
    let mut from = 0;
    let mut found = None;
    for _ in 0..=n {
        let i = text[from..].find(needle).expect("needle") + from;
        found = Some(i);
        from = i + needle.len();
    }
    u32::try_from(found.unwrap() + into).unwrap()
}

/// Where a definition landed: the file and the defined bytes.
fn landed(d: &Definition) -> (PathBuf, String) {
    match &d.target {
        Target::File(l) => (
            l.path.clone(),
            l.text[l.start as usize..l.end as usize].to_owned(),
        ),
        Target::Prelude { name } => (PathBuf::from(name), String::new()),
    }
}

const LIB: &str = "pub fn helper(n: Int) -> Int { n + 1 }\nfn private_detail() -> Int { 1 }\n";
const MAIN: &str = "use \"lib.nz\";\nfn main() -> Int { helper(41) }\n";

// ---------------------------------------------------------------------------------------
// Buffers are the source.
// ---------------------------------------------------------------------------------------

#[test]
fn an_open_buffer_is_read_instead_of_the_file_on_disk() {
    let p = Project::new(&[("m.nz", "fn main() -> Int { 1 + }\n")]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, "fn main() -> Int { 1 + 2 }\n".into());
        assert!(for_path(&out, &m).diagnostics.is_empty(), "{out:#?}");
        // The disk still holds the broken text, and nothing was written to it.
        assert_eq!(
            std::fs::read_to_string(&m).unwrap(),
            "fn main() -> Int { 1 + }\n"
        );
    });
}

#[test]
fn an_unsaved_dependency_is_what_its_importer_sees() {
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    let disk_lib = p.disk("lib.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&main, 1, MAIN.into());
        assert!(for_path(&out, &main).diagnostics.is_empty(), "{out:#?}");

        // A public signature change, unsaved: `helper` now takes two arguments. The
        // importer is re-checked against the buffer and its call no longer fits.
        let changed = "pub fn helper(n: Int, m: Int) -> Int { n + m }\n";
        let out = s.open(&lib, 1, changed.into());
        let mine = for_path(&out, &main);
        assert!(
            codes_of(mine).contains(&codes::WRONG_ARGUMENT_COUNT.to_string()),
            "the importer should see the unsaved signature: {mine:#?}"
        );

        // A definition query from the importer lands in the *buffer's* text.
        let d = s
            .definition(&main, 1, at(MAIN, "helper(", 0, 2))
            .expect("a definition");
        let (path, bytes) = landed(&d);
        assert!(same_file(&path, &lib));
        assert_eq!(bytes, "helper");
        if let Target::File(l) = &d.target {
            assert_eq!(&*l.text, changed, "the location is in the buffer's text");
        }
        assert_eq!(
            std::fs::read_to_string(&lib).unwrap(),
            disk_lib,
            "nothing written"
        );
    });
}

#[test]
fn closing_a_buffer_gives_the_file_back_to_the_disk() {
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let out = s.open(&lib, 1, "pub fn other() -> Int { 1 }\n".into());
        assert!(
            !for_path(&out, &main).diagnostics.is_empty(),
            "with `helper` gone from the buffer, the call cannot resolve"
        );
        let out = s.close(&lib);
        assert!(
            for_path(&out, &main).diagnostics.is_empty(),
            "closed, the disk's `helper` is back: {out:#?}"
        );
        // The closed document's own set is cleared.
        let cleared = for_path(&out, &lib);
        assert_eq!(cleared.version, None);
        assert!(cleared.diagnostics.is_empty());
        assert_eq!(s.open_documents(), 1);
    });
}

#[test]
fn a_private_body_change_in_a_dependency_does_not_reach_its_importer() {
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        // The private helper's body now has a type error. It is the library's error, in
        // the library's file; the importer's diagnostics are exactly what they were.
        let broken =
            "pub fn helper(n: Int) -> Int { n + 1 }\nfn private_detail() -> Int { \"x\" }\n";
        let out = s.open(&lib, 1, broken.into());
        assert!(for_path(&out, &main).diagnostics.is_empty(), "{out:#?}");
        assert!(
            !for_path(&out, &lib).diagnostics.is_empty(),
            "the library itself reports it"
        );
    });
}

#[test]
fn an_unsaved_use_edit_changes_which_module_is_loaded() {
    let p = Project::new(&[
        ("a.nz", "pub fn pick() -> Int { 1 }\n"),
        ("b.nz", "pub fn pick() -> Int { 2 }\n"),
        ("main.nz", "use \"a.nz\";\nfn main() -> Int { pick() }\n"),
    ]);
    let (a, b, main) = (p.path("a.nz"), p.path("b.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let v1 = "use \"a.nz\";\nfn main() -> Int { pick() }\n";
        s.open(&main, 1, v1.into());
        let d = s.definition(&main, 1, at(v1, "pick", 0, 1)).unwrap();
        assert!(same_file(&landed(&d).0, &a));

        let v2 = "use \"b.nz\";\nfn main() -> Int { pick() }\n";
        s.change(&main, 2, v2.into());
        let d = s.definition(&main, 2, at(v2, "pick", 0, 1)).unwrap();
        assert!(
            same_file(&landed(&d).0, &b),
            "the edited `use` is the graph now"
        );
    });
}

#[test]
fn an_import_that_names_nothing_is_reported_and_an_unsaved_one_resolves() {
    let p = Project::new(&[("main.nz", "use \"new.nz\";\nfn main() -> Int { fresh() }\n")]);
    let (main, fresh) = (p.path("main.nz"), p.path("new.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let text = p.disk("main.nz");
        let out = s.open(&main, 1, text.clone());
        assert!(
            codes_of(for_path(&out, &main)).contains(&codes::MODULE_NOT_FOUND.to_string()),
            "{out:#?}"
        );
        // A buffer for a file that exists only in the editor is a module too.
        let out = s.open(&fresh, 1, "pub fn fresh() -> Int { 3 }\n".into());
        assert!(for_path(&out, &main).diagnostics.is_empty(), "{out:#?}");
        let d = s.definition(&main, 1, at(&text, "fresh", 0, 0)).unwrap();
        assert!(landed(&d).0.ends_with("new.nz"));
    });
}

// ---------------------------------------------------------------------------------------
// Current means current.
// ---------------------------------------------------------------------------------------

#[test]
fn a_question_about_another_version_has_no_answer() {
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let offset = at(MAIN, "helper(", 0, 0);
        assert!(s.definition(&main, 1, offset).is_some());
        s.change(&main, 2, MAIN.into());
        assert!(
            s.definition(&main, 1, offset).is_none(),
            "version 1 is gone"
        );
        assert!(s.hover(&main, 1, offset).is_none());
        assert!(s.definition(&main, 2, offset).is_some());
        assert!(
            s.definition(&main, 3, offset).is_none(),
            "version 3 does not exist"
        );
    });
}

#[test]
fn a_broken_buffer_never_answers_from_the_clean_one_before_it() {
    let p = Project::new(&[("lib.nz", LIB)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let clean =
            "use \"lib.nz\";\nfn main() -> Int { helper(1) }\nfn other() -> Int { helper(2) }\n";
        s.open(&main, 1, clean.into());
        let in_main = at(clean, "helper(1", 0, 0);
        assert!(s.definition(&main, 1, in_main).is_some());

        // `main`'s body no longer parses. Its call is still in the concrete tree, and the
        // checker never saw it: there is no current answer, so there is no answer — not
        // the answer version 1 had.
        let broken = "use \"lib.nz\";\nfn main() -> Int { let x = ; helper(1) }\nfn other() -> Int { helper(2) }\n";
        let out = s.change(&main, 2, broken.into());
        assert!(!for_path(&out, &main).diagnostics.is_empty());
        assert!(s
            .definition(&main, 2, at(broken, "helper(1", 0, 0))
            .is_none());
        // A function that still parses is still answered, from the current analysis.
        assert!(s
            .definition(&main, 2, at(broken, "helper(2", 0, 0))
            .is_some());
    });
}

#[test]
fn a_fix_publishes_an_empty_set_so_old_errors_are_cleared() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, "fn main() -> Int { true }\n".into());
        assert_eq!(
            codes_of(for_path(&out, &m)),
            [codes::TYPE_MISMATCH.to_string()]
        );
        let out = s.change(&m, 2, "fn main() -> Int { 1 }\n".into());
        let now = for_path(&out, &m);
        assert_eq!(now.version, Some(2));
        assert!(
            now.diagnostics.is_empty(),
            "an empty set is published, not nothing"
        );
    });
}

// ---------------------------------------------------------------------------------------
// Definitions follow identities.
// ---------------------------------------------------------------------------------------

const BODY: &str = "struct Point { x: Int, y: Int, }\n\
enum Shape { Dot, Box(side: Int), }\n\
fn area(s: Shape) -> Int { match s { Shape.Dot() => 0, Shape.Box(side: k) => k * k, } }\n\
fn main() -> Int {\n    let p = Point(x: 1, y: 2);\n    let q = p.x;\n    let b = Shape.Box(side: q);\n    area(b) + p.y\n}\n";

fn definition_of(
    s: &LanguageService,
    path: &Path,
    needle: &str,
    n: usize,
    into: usize,
) -> (String, String) {
    let d = s
        .definition(path, 1, at(BODY, needle, n, into))
        .unwrap_or_else(|| panic!("no definition for {needle:?}"));
    (d.kind.to_owned(), landed(&d).1)
}

#[test]
fn every_category_the_resolution_identifies_is_a_definition() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, BODY.into());
        assert!(for_path(&out, &m).diagnostics.is_empty(), "{out:#?}");
        let span_of = |kind: &str, text: &str| (kind.to_owned(), text.to_owned());
        // A call, by the function it resolved to.
        assert_eq!(
            definition_of(&s, &m, "area(b)", 0, 0),
            span_of("function", "area")
        );
        // A local, by its slot: to the `let` that introduced it...
        assert_eq!(definition_of(&s, &m, "p.x", 0, 0), span_of("local", "p"));
        // ...and a parameter, and a pattern binding.
        assert_eq!(
            definition_of(&s, &m, "match s", 0, 6),
            span_of("local", "s")
        );
        assert_eq!(definition_of(&s, &m, "k * k", 0, 0), span_of("local", "k"));
        // A record construction, by the record.
        assert_eq!(
            definition_of(&s, &m, "Point(x", 0, 0),
            span_of("record", "Point")
        );
        // A variant, by the enum's variant.
        assert_eq!(
            definition_of(&s, &m, "Box(side: q", 0, 0),
            span_of("variant", "Box")
        );
        // A projection, by the record's field.
        assert_eq!(definition_of(&s, &m, "p.y", 0, 2), span_of("field", "y"));
        // A payload field, by the variant's field.
        assert_eq!(
            definition_of(&s, &m, "side: q", 0, 0),
            span_of("payload field", "side")
        );
    });
}

#[test]
fn one_spelling_with_three_meanings_goes_to_three_definitions() {
    // `n` is a function, a parameter, and a `let` that shadows the parameter in an inner
    // block. A search by spelling finds the first `n` for all three; the resolution knows
    // which binding or function each use is.
    let text = "fn n() -> Int { 7 }\nfn f(n: Int) -> Int {\n    let a = n;\n    let b = { let n = a + 1; n };\n    n + b + n()\n}\nfn main() -> Int { f(1) }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, text.into());
        assert!(for_path(&out, &m).diagnostics.is_empty(), "{out:#?}");
        let go = |needle: &str, n: usize| -> (String, u32) {
            let d = s.definition(&m, 1, at(text, needle, n, 0)).unwrap();
            let Target::File(l) = d.target else { panic!() };
            (d.kind.to_owned(), l.start)
        };
        let function = u32::try_from(text.find("fn n").unwrap() + 3).unwrap();
        let parameter = u32::try_from(text.find("(n: Int)").unwrap() + 1).unwrap();
        let inner = u32::try_from(text.find("let n").unwrap() + 4).unwrap();
        assert_eq!(
            go("n;", 0),
            ("local".to_owned(), parameter),
            "`let a = n` is the parameter"
        );
        assert_eq!(
            go("n }", 0),
            ("local".to_owned(), inner),
            "inside the block, the inner `let`"
        );
        assert_eq!(
            go("n + b", 0),
            ("local".to_owned(), parameter),
            "after it, the parameter again"
        );
        assert_eq!(
            go("n()", 1),
            ("function".to_owned(), function),
            "a call is the function"
        );
    });
}

#[test]
fn a_core_prelude_definition_is_named_not_given_a_path() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let text = "fn main() -> Int {\n    let r = Result[Int, Int].Ok(value: 1);\n    match r { Result.Ok(value: v) => v, Result.Err(error: e) => e, }\n}\n";
        let out = s.open(&m, 1, text.into());
        assert!(for_path(&out, &m).diagnostics.is_empty(), "{out:#?}");
        let d = s.definition(&m, 1, at(text, "Ok(value: 1", 0, 0)).unwrap();
        assert_eq!(d.kind, "variant");
        let Target::Prelude { name } = d.target else {
            panic!("the prelude has no path: {d:?}");
        };
        assert!(name.contains("prelude"), "{name}");
    });
}

#[test]
fn hover_reads_the_resolution_and_nothing_else() {
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let h = s.hover(&main, 1, at(MAIN, "helper(", 0, 3)).unwrap();
        // No declared effect set, so none in the signature; what the body requires is
        // the checker's, and said as inferred (N36).
        assert_eq!(h.text, "fn helper(n: Int) -> Int\ninferred effects: {}");
        assert_eq!(&MAIN[h.start as usize..h.end as usize], "helper");

        let local = "fn main() -> Int { let mut n = 2; n = n + 1; n }\n";
        s.change(&main, 2, local.into());
        let h = s.hover(&main, 2, at(local, "n }", 0, 0)).unwrap();
        assert_eq!(h.text, "mut n: Int");
        // A literal and a keyword have no hover.
        assert!(s.hover(&main, 2, at(local, "2;", 0, 0)).is_none());
        assert!(s.hover(&main, 2, at(local, "let", 0, 1)).is_none());
    });
}

// ---------------------------------------------------------------------------------------
// Where a position is.
// ---------------------------------------------------------------------------------------

#[test]
fn a_position_is_on_a_name_or_on_nothing() {
    let p = Project::new(&[("lib.nz", LIB)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let text = "use \"lib.nz\";\nfn main() -> Int { helper( 41 ) } // helper\n";
        s.open(&main, 1, text.into());
        let start = at(text, "helper(", 0, 0);
        let q = |o: u32| s.definition(&main, 1, o).is_some();
        assert!(q(start), "first byte of the name");
        assert!(q(start + 3), "inside the name");
        assert!(q(start + 6), "just after the name, before `(`");
        assert!(
            !q(start + 7),
            "after `(`: punctuation, and not just after a name"
        );
        assert!(!q(start + 8), "whitespace");
        assert!(!q(at(text, "41", 0, 0)), "a literal");
        assert!(!q(at(text, "fn main", 0, 0)), "a keyword");
        assert!(!q(at(text, "// helper", 0, 5)), "the name inside a comment");
        assert!(
            !q(u32::try_from(text.len()).unwrap()),
            "the end of the file"
        );
        assert!(!q(9999), "past the end of the file");
    });
}

#[test]
fn a_name_inside_a_malformed_region_has_no_definition() {
    let p = Project::new(&[("lib.nz", LIB)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let text = "use \"lib.nz\";\nfn main() -> Int { let x = helper(1 +; x }\n";
        let out = s.open(&main, 1, text.into());
        assert!(!for_path(&out, &main).diagnostics.is_empty());
        assert!(s.definition(&main, 1, at(text, "helper", 0, 0)).is_none());
    });
}

// ---------------------------------------------------------------------------------------
// The compiler's answers, not the service's.
// ---------------------------------------------------------------------------------------

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

#[test]
fn the_services_diagnostics_are_the_command_lines() {
    // For every multi-module conformance program and every example, the service opened
    // on the root file reports exactly what the loader and checker every command uses
    // report for that file — the same compilation, with no buffer changed.
    let root = workspace();
    let mut roots: Vec<PathBuf> = Vec::new();
    for dir in ["examples", "compiler/conformance/modules"] {
        for e in walk(&root.join(dir)) {
            if e.file_name().is_some_and(|n| n == "main.nz") || dir == "examples" {
                roots.push(e);
            }
        }
    }
    assert!(roots.len() >= 15, "{roots:?}");
    on_the_service_stack(move || {
        for path in roots {
            let text = std::fs::read_to_string(&path).unwrap();
            let mut s = LanguageService::new();
            let out = s.open(&path, 1, text);
            let service: Vec<String> = for_path(&out, &path)
                .diagnostics
                .iter()
                .map(|d| format!("{} {:?}", d.code, d.primary.span))
                .collect();
            let decided = nazm_service::root::decide(&path, None).unwrap();
            let loaded = nazm_service::load::load(&path, decided.path());
            let (map, graph, mut early) = match loaded {
                Ok(l) => (l.map, l.graph, Vec::new()),
                Err((l, d)) => (l.map, l.graph, d),
            };
            let a = nazm_core::analyse(&map, &graph, nazm_core::Entry::Absent);
            early.extend(a.diagnostics);
            let cli: Vec<String> = early
                .iter()
                .filter(|d| d.primary.span.file == nazm_span::FileId::ROOT)
                .map(|d| format!("{} {:?}", d.code, d.primary.span))
                .collect();
            assert_eq!(service, cli, "{}", path.display());
        }
    });
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else if p.extension().is_some_and(|x| x == "nz") {
            out.push(p);
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------------------
// Long-lived means bounded.
// ---------------------------------------------------------------------------------------

#[test]
fn a_thousand_edits_leave_one_current_answer_and_bounded_state() {
    let p = Project::new(&[("lib.nz", LIB)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let good = "use \"lib.nz\";\nfn main() -> Int { helper(41) }\n";
        let bad = "use \"lib.nz\";\nfn main() -> Int { helper(41 }\n";
        let mut s = LanguageService::new();
        s.open(&main, 0, good.into());
        let mut after_warmup = (0, 0, 0);
        for version in 1..=1000 {
            let text = if version % 2 == 0 { good } else { bad };
            let out = s.change(&main, version, text.into());
            let mine = for_path(&out, &main);
            assert_eq!(mine.version, Some(version));
            assert_eq!(
                mine.diagnostics.is_empty(),
                version % 2 == 0,
                "version {version}"
            );
            if version == 10 {
                after_warmup = s.retained();
            }
        }
        // One document and one analysis of it are kept, over the current text only,
        // whatever the history: the same after a thousand edits as after ten. Resident
        // memory across edits is measured in `docs/performance.md`.
        assert_eq!(s.retained(), after_warmup);
        assert_eq!(s.retained().0, 1);
        assert_eq!(s.retained().1, 1);
        // The last version is the answer.
        assert!(s
            .definition(&main, 1000, at(good, "helper", 0, 0))
            .is_some());
        assert!(s.definition(&main, 999, at(good, "helper", 0, 0)).is_none());
    });
}
