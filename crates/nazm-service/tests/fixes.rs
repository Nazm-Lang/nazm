//! Fix plans through the language service. N24.
//!
//! Every plan is checked against the fix the compiler attached to the current diagnostic —
//! its span, replacement, applicability and precondition — and every real fix the compiler
//! has is applied in memory: the text must still parse, the diagnostic it targets must be
//! gone, and an automatic fix must change nothing outside its span.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_diag::Applicability;
use nazm_service::{FixPlan, LanguageService, NoFixes};

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
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

fn offset(text: &str, needle: &str, n: usize) -> u32 {
    let mut from = 0;
    let mut found = 0;
    for _ in 0..=n {
        found = text[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?}"))
            + from;
        from = found + needle.len();
    }
    u32::try_from(found).unwrap()
}

/// The plan's one edit applied to its text.
fn applied(plan: &FixPlan) -> String {
    let e = &plan.file.edits[0];
    let mut text = plan.file.text.to_string();
    text.replace_range(e.start as usize..e.end as usize, &e.replacement);
    text
}

fn parses(text: &str) -> bool {
    nazm_core::__syntax::parse_source(text).1.is_empty()
}

/// A real fix, end to end: the one plan at the diagnostic, what it is, and what applying it
/// does.
struct Case {
    name: &'static str,
    text: &'static str,
    /// Where the diagnostic is, as a needle and its occurrence.
    at: (&'static str, usize),
    code: &'static str,
    applicability: Applicability,
    /// The fix's span, as a needle and occurrence, and its length (0 for an insertion).
    edit_at: (&'static str, usize, u32),
    replacement: &'static str,
}

const CASES: [Case; 5] = [
    Case {
        name: "an assignment to an immutable binding: `mut` at its `let`",
        text: "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n",
        at: ("x = 2", 0),
        code: "N0310",
        applicability: Applicability::Automatic,
        edit_at: ("x = 1", 0, 1),
        replacement: "mut x",
    },
    Case {
        name: "a mutable parameter: `mut` removed",
        text: "fn f(mut n: Int) -> Int { n }\nfn main() -> Int { f(1) }\n",
        at: ("mut", 0),
        code: "N0311",
        applicability: Applicability::Automatic,
        edit_at: ("mut", 0, 3),
        replacement: "",
    },
    Case {
        name: "a loop body with a value: `;` after it, reviewed",
        text:
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; i }\n    i\n}\n",
        at: ("i }", 0),
        code: "N0300",
        applicability: Applicability::NeedsReview,
        edit_at: (" }", 0, 0),
        replacement: ";",
    },
    Case {
        name: "an expression with more after it: `;`, reviewed",
        text: "fn f() -> Int { 1 }\nfn main() -> Int {\n    f()\n    f()\n}\n",
        at: ("f()", 2),
        code: "N0100",
        applicability: Applicability::NeedsReview,
        edit_at: ("\n    f()\n}", 0, 0),
        replacement: ";",
    },
    Case {
        name: "an unterminated string: closed at the end of its line, reviewed",
        text: "fn name() -> Str { \"abc\n}\n",
        at: ("\"abc", 0),
        code: "N0002",
        applicability: Applicability::NeedsReview,
        edit_at: ("\n}", 0, 0),
        replacement: "\"",
    },
];

#[test]
fn every_real_fix_is_planned_from_its_diagnostic_and_repairs_it() {
    on_the_service_stack(|| {
        for case in &CASES {
            let p = Project::new(&[("main.nz", case.text)]);
            let main = p.path("main.nz");
            let mut s = LanguageService::new();
            let published = s.open(&main, 1, case.text.to_owned());
            // The target diagnostic, and whatever follows from the same mistake (a dropped
            // `mut` parameter leaves its uses unresolved), which carries no fix.
            let d = published[0]
                .diagnostics
                .iter()
                .find(|d| d.code.to_string() == case.code)
                .unwrap_or_else(|| panic!("{}: {:?}", case.name, published[0].diagnostics));
            assert!(published[0]
                .diagnostics
                .iter()
                .all(|o| o == d || o.fixes.is_empty()));
            let cursor = offset(case.text, case.at.0, case.at.1);
            assert!(
                d.primary.span.start <= cursor && cursor <= d.primary.span.end,
                "{}: the cursor is on the diagnostic",
                case.name
            );
            let plans = s.fix_plans(&main, 1, cursor, cursor).unwrap();
            assert_eq!(plans.len(), 1, "{}", case.name);
            let plan = &plans[0];
            // The plan is the compiler's fix, exactly.
            let fix = &d.fixes[0];
            assert_eq!(plan.diagnostic, *d, "{}", case.name);
            assert_eq!(plan.applicability, case.applicability, "{}", case.name);
            assert_eq!(plan.applicability, fix.applicability);
            assert_eq!(plan.precondition, fix.precondition);
            assert!(!plan.precondition.is_empty());
            assert_eq!(plan.description, fix.description);
            assert_eq!(plan.alternatives, 1);
            assert_eq!(plan.file.version, Some(1));
            assert_eq!(&*plan.file.text, case.text);
            let e = &plan.file.edits[0];
            let start = offset(case.text, case.edit_at.0, case.edit_at.1);
            assert_eq!(
                (e.start, e.end),
                (start, start + case.edit_at.2),
                "{}: the edit is the fix's span",
                case.name
            );
            assert_eq!((e.start, e.end), (fix.span.start, fix.span.end));
            assert_eq!(e.replacement, case.replacement);
            assert_eq!(e.expected, &case.text[e.start as usize..e.end as usize]);
            assert!(s.fix_plan_is_current(plan));

            // Applied: it parses, the diagnostic is gone, and an automatic fix touched
            // nothing else.
            let after = applied(plan);
            assert!(parses(&after), "{}: {after}", case.name);
            if case.applicability == Applicability::Automatic {
                assert_eq!(after[..e.start as usize], case.text[..e.start as usize]);
                assert_eq!(
                    after[e.start as usize + e.replacement.len()..],
                    case.text[e.end as usize..]
                );
            }
            let published = s.change(&main, 2, after.clone());
            assert!(
                published[0]
                    .diagnostics
                    .iter()
                    .all(|d| d.code.to_string() != case.code),
                "{}: {:?}",
                case.name,
                published[0].diagnostics
            );
            assert_eq!(
                s.fix_plans(&main, 2, 0, u32::try_from(after.len()).unwrap())
                    .unwrap(),
                []
            );
            assert!(!s.fix_plan_is_current(plan), "{}: the old plan", case.name);
        }
    });
}

#[test]
fn the_edit_is_where_the_fix_is_and_relevance_is_where_the_diagnostic_is() {
    // `N0310` is about the assignment; its fix edits the `let`. A cursor on the `let` is
    // not on the diagnostic, and gets nothing.
    let case = &CASES[0];
    let p = Project::new(&[("main.nz", case.text)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, case.text.to_owned());
        let on_let = offset(case.text, "x = 1", 0);
        assert_eq!(s.fix_plans(&main, 1, on_let, on_let).unwrap(), []);
        let on_assignment = offset(case.text, "x = 2", 0);
        let plans = s.fix_plans(&main, 1, on_assignment, on_assignment).unwrap();
        assert_eq!(plans[0].file.edits[0].start, on_let);
        assert_ne!(
            plans[0].diagnostic.primary.span.start,
            plans[0].file.edits[0].start
        );
    });
}

#[test]
fn a_request_gets_the_fixes_of_the_diagnostics_it_touches_and_no_others() {
    const TEXT: &str =
        "fn main() -> Int {\n    let x = 1;\n    let y = 2;\n    x = 3;\n    y = 4;\n    x + y\n}\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, TEXT.to_owned());
        let edited = |plans: Vec<FixPlan>| -> Vec<u32> {
            plans.iter().map(|p| p.file.edits[0].start).collect()
        };
        let (x_let, y_let) = (offset(TEXT, "x = 1", 0), offset(TEXT, "y = 2", 0));
        let (x, y) = (offset(TEXT, "x = 3", 0), offset(TEXT, "y = 4", 0));
        // A cursor inside, at the start and at the end of each diagnostic.
        assert_eq!(edited(s.fix_plans(&main, 1, x, x).unwrap()), [x_let]);
        assert_eq!(
            edited(s.fix_plans(&main, 1, x + 1, x + 1).unwrap()),
            [x_let]
        );
        assert_eq!(edited(s.fix_plans(&main, 1, y, y).unwrap()), [y_let]);
        // A range over both, and one that only reaches the end of the first.
        assert_eq!(
            edited(s.fix_plans(&main, 1, x, y + 1).unwrap()),
            [x_let, y_let]
        );
        assert_eq!(
            edited(s.fix_plans(&main, 1, x - 3, x).unwrap()),
            Vec::<u32>::new()
        );
        // Elsewhere in the file: nothing, although the file has fixes.
        let tail = offset(TEXT, "x + y", 0);
        assert_eq!(
            edited(s.fix_plans(&main, 1, tail, tail).unwrap()),
            Vec::<u32>::new()
        );
    });
}

#[test]
fn a_plan_is_current_only_at_its_generation_version_and_bytes() {
    let case = &CASES[0];
    let p = Project::new(&[("main.nz", case.text)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, case.text.to_owned());
        let at = offset(case.text, "x = 2", 0);
        let plan = s.fix_plans(&main, 1, at, at).unwrap().remove(0);
        assert!(s.fix_plan_is_current(&plan));
        // The bytes it replaces are not what the plan expects: not current, though nothing
        // else changed.
        let mut wrong = plan.clone();
        wrong.file.edits[0].expected = "y".to_owned();
        assert!(!s.fix_plan_is_current(&wrong));
        // Another version of the same document: not current.
        let mut other = plan.clone();
        other.file.version = Some(2);
        assert!(!s.fix_plan_is_current(&other));
        // A plan with no version cannot be current, whatever the file says.
        let mut unversioned = plan.clone();
        unversioned.file.version = None;
        assert!(!s.fix_plan_is_current(&unversioned));

        // Another document opened: this one is untouched, at the same version with the same
        // text, but the generation has moved, and a plan belongs to one generation.
        let other = main.with_file_name("other.nz");
        s.open(&other, 1, "fn other() -> Int { 1 }\n".to_owned());
        assert!(
            !s.fix_plan_is_current(&plan),
            "a plan of another generation"
        );
        let plan = s.fix_plans(&main, 1, at, at).unwrap().remove(0);
        assert!(s.fix_plan_is_current(&plan));
        s.close(&other);
        assert!(!s.fix_plan_is_current(&plan));
        let plan = s.fix_plans(&main, 1, at, at).unwrap().remove(0);

        // A change outside the edit: the same fix is still there, but the old plan is of
        // the old generation and version, and is not current; a fresh one is.
        let moved = case.text.replace("x\n}", "x + 0\n}");
        s.change(&main, 2, moved.clone());
        assert!(!s.fix_plan_is_current(&plan));
        assert_eq!(s.fix_plans(&main, 1, at, at), Err(NoFixes::NotCurrent));
        let fresh = s.fix_plans(&main, 2, at, at).unwrap().remove(0);
        assert_eq!(fresh.file.edits[0].start, plan.file.edits[0].start);
        assert!(s.fix_plan_is_current(&fresh));
        // A change inside the edit's bytes: no plan for the old bytes, and a fresh one for
        // the new.
        let renamed = moved.replace("x", "xy");
        s.change(&main, 3, renamed.clone());
        assert!(!s.fix_plan_is_current(&fresh));
        let now = s
            .fix_plans(&main, 3, 0, u32::try_from(renamed.len()).unwrap())
            .unwrap();
        assert_eq!(now[0].file.edits[0].expected, "xy");
    });
}

#[test]
fn fixes_follow_the_buffers_and_never_come_back_from_an_earlier_analysis() {
    const BROKEN: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n";
    const CLEAN: &str = "fn main() -> Int {\n    let mut x = 1;\n    x = 2;\n    x\n}\n";
    const OTHER: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2\n}\n";
    let p = Project::new(&[("main.nz", CLEAN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let all = |s: &LanguageService, v: i32, text: &str| {
            s.fix_plans(&main, v, 0, u32::try_from(text.len()).unwrap())
                .unwrap()
        };
        // The disk is clean; the unsaved buffer is not, and it is what is planned from.
        s.open(&main, 1, BROKEN.to_owned());
        assert_eq!(all(&s, 1, BROKEN).len(), 1);
        s.change(&main, 2, CLEAN.to_owned());
        assert_eq!(all(&s, 2, CLEAN), []);
        // A different mistake that does not parse: no fix from the earlier analysis.
        s.change(&main, 3, OTHER.to_owned());
        let other = all(&s, 3, OTHER);
        assert!(other.iter().all(|p| p.file.version == Some(3)));
        assert!(other
            .iter()
            .all(|p| p.diagnostic.code.to_string() != "N0310"));
        s.change(&main, 4, BROKEN.to_owned());
        assert_eq!(all(&s, 4, BROKEN)[0].file.version, Some(4));
    });
}

#[test]
fn an_unsaved_dependency_is_what_the_fixes_are_computed_against_and_close_restores_it() {
    // No fix the compiler attaches today depends on another file, so what is pinned is that
    // the plans are those of the current diagnostics, computed with the unsaved `lib.nz`:
    // with `step` gone from it, `main.nz` gains an unknown-function diagnostic — which has
    // no fix, and gets none — beside the loop's, whose fix stays. Closing `lib.nz` restores
    // the disk's diagnostics.
    const LIB: &str = "pub fn step(n: Int) -> Int { n + 1 }\n";
    const LIB_EDITED: &str = "pub fn stride(n: Int) -> Int { n + 1 }\n";
    const MAIN: &str =
        "use \"lib.nz\";\nfn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; step(i) }\n    i\n}\n";
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let fixes = |s: &LanguageService| -> Vec<String> {
            s.fix_plans(&main, 1, 0, u32::try_from(MAIN.len()).unwrap())
                .unwrap()
                .iter()
                .map(|p| p.diagnostic.code.to_string())
                .collect()
        };
        let codes = |published: &[nazm_service::Published]| -> Vec<String> {
            let mut c: Vec<String> = published
                .iter()
                .filter(|p| p.path == main)
                .flat_map(|p| p.diagnostics.iter().map(|d| d.code.to_string()))
                .collect();
            c.sort();
            c
        };
        let opened = s.open(&main, 1, MAIN.to_owned());
        assert_eq!(codes(&opened), ["N0300"]);
        assert_eq!(fixes(&s), ["N0300"]);
        let edited = s.open(&lib, 1, LIB_EDITED.to_owned());
        let now = codes(&edited);
        assert_eq!(now.len(), 2, "{now:?}");
        assert!(now.contains(&"N0300".to_owned()));
        assert_eq!(fixes(&s), ["N0300"], "the unknown function has no fix");
        let step = offset(MAIN, "step(i)", 0);
        let at_step: Vec<String> = s
            .fix_plans(&main, 1, step, step)
            .unwrap()
            .iter()
            .map(|p| p.diagnostic.code.to_string())
            .collect();
        assert!(!at_step.iter().any(|c| c != "N0300"), "{at_step:?}");
        let closed = s.close(&lib);
        assert_eq!(codes(&closed), ["N0300"]);
        assert_eq!(fixes(&s), ["N0300"]);
    });
}

#[test]
fn a_thousand_edits_leave_only_the_current_fix() {
    let case = &CASES[0];
    const CLEAN: &str = "fn main() -> Int {\n    let mut x = 1;\n    x = 2;\n    x\n}\n";
    let p = Project::new(&[("main.nz", case.text)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 0, CLEAN.to_owned());
        let baseline = (s.retained(), s.indexed(), s.scoped(), s.calls());
        let mut previous: Option<FixPlan> = None;
        for v in 1..=1000 {
            let text = if v % 2 == 0 { CLEAN } else { case.text };
            s.change(&main, v, text.to_owned());
            let plans = s
                .fix_plans(&main, v, 0, u32::try_from(text.len()).unwrap())
                .unwrap();
            if let Some(old) = &previous {
                assert!(!s.fix_plan_is_current(old), "version {v}");
            }
            assert_eq!(s.fix_plans(&main, v - 1, 0, 1), Err(NoFixes::NotCurrent));
            if v % 2 == 0 {
                assert_eq!(plans, [], "version {v}");
                assert_eq!(
                    (s.retained(), s.indexed(), s.scoped(), s.calls()),
                    baseline,
                    "version {v}"
                );
            } else {
                assert_eq!(plans.len(), 1, "version {v}");
                assert_eq!(plans[0].file.version, Some(v));
                previous = plans.into_iter().next();
            }
        }
    });
}
