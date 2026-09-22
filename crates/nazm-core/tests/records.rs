//! Records: what the checker accepts, what it refuses, and by which name.
//!
//! The acceptance question N9 exists to answer is whether a record's behaviour can be
//! *derived* from its fields rather than written out per case. These are the front end's
//! half of the evidence; `crates/nazm-cli/tests/memory.rs` is the back end's, and
//! `crates/nazm-cli/tests/visibility.rs` the module system's.
//!
//! Each test names the rule in `docs/spec.md` it pins.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::{run, Outcome, Value};
use nazm_diag::codes;

fn value(src: &str) -> Value {
    match run(src) {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}"),
    }
}

/// The single diagnostic a program fails with, code and message.
///
/// Asserts there is exactly one, so a test cannot pass on an error it did not mean to
/// provoke — and returns the message, because several of these rules are about *what the
/// diagnostic says* rather than only that one was raised.
fn sole(src: &str) -> (nazm_diag::Code, String) {
    match run(src) {
        Outcome::CheckFailed(ds) => {
            assert_eq!(ds.len(), 1, "expected one error, got {ds:#?}");
            (ds[0].code.clone(), ds[0].message.clone())
        }
        Outcome::RuntimeFailed(d) => (d.code, d.message),
        Outcome::Value(v) => panic!("expected an error, got {v}"),
    }
}

fn code(src: &str) -> nazm_diag::Code {
    sole(src).0
}

// --- construction, projection, mutation ----------------------------------------------

#[test]
fn a_record_is_built_by_naming_every_field_and_read_by_projection() {
    let src = r"
        struct Point { x: Int, y: Int, }
        fn main() -> Int { let p = Point(x: 17, y: 25); p.x + p.y }
    ";
    assert_eq!(value(src), Value::Int(42));
}

#[test]
fn initialisers_may_be_written_in_any_order_and_mean_the_same_record() {
    // Construction is by name, so the order the author wrote them in is presentation.
    let a = r"
        struct Point { x: Int, y: Int, }
        fn main() -> Int { let p = Point(x: 1, y: 2); p.x * 10 + p.y }
    ";
    let b = r"
        struct Point { x: Int, y: Int, }
        fn main() -> Int { let p = Point(y: 2, x: 1); p.x * 10 + p.y }
    ";
    assert_eq!(value(a), Value::Int(12));
    assert_eq!(value(b), Value::Int(12));
}

#[test]
fn moving_two_field_declarations_changes_nothing() {
    let a = r"
        struct Point { x: Int, y: Str, }
        fn main() -> Int { let p = Point(x: 4, y: 'ab'); p.x + str_len(p.y) }
    "
    .replace('\'', "\"");
    let b = r"
        struct Point { y: Str, x: Int, }
        fn main() -> Int { let p = Point(x: 4, y: 'ab'); p.x + str_len(p.y) }
    "
    .replace('\'', "\"");
    assert_eq!(value(&a), Value::Int(6));
    assert_eq!(value(&b), Value::Int(6));
}

#[test]
fn initialisers_evaluate_in_the_order_written() {
    // `docs/spec.md` fixes the order, and the layout has nothing to do with it. The
    // sequence records which initialiser ran first; `b` is declared first and evaluated
    // second.
    let src = r"
        struct Two { a: Int, b: Int, }
        fn note(xs: Ints, n: Int) -> Int { ints_push(xs, n); n }
        fn main() -> Int {
            let xs = ints_new();
            let t = Two(b: note(xs, 1), a: note(xs, 2));
            ints_get(xs, 0) * 10 + ints_get(xs, 1) + t.a - t.a
        }
    ";
    assert_eq!(value(src), Value::Int(12));
}

#[test]
fn a_mutable_binding_may_have_a_field_replaced() {
    let src = r"
        struct Point { x: Int, y: Int, }
        fn main() -> Int { let mut p = Point(x: 1, y: 2); p.x = 40; p.x + p.y }
    ";
    assert_eq!(value(src), Value::Int(42));
}

#[test]
fn an_immutable_binding_may_not() {
    let src = r"
        struct Point { x: Int, y: Int, }
        fn main() -> Int { let p = Point(x: 1, y: 2); p.x = 40; p.x }
    ";
    let (c, message) = sole(src);
    assert_eq!(c, codes::ASSIGN_TO_IMMUTABLE);
    // The message names the field, because the binding alone would not say what was
    // being attempted.
    assert!(message.contains("`x`"), "{message}");
}

#[test]
fn a_parameter_is_an_immutable_binding_like_any_other() {
    let src = r"
        struct Point { x: Int, y: Int, }
        fn move_it(p: Point) -> Int { p.x = 2; p.x }
        fn main() -> Int { move_it(Point(x: 1, y: 2)) }
    ";
    assert_eq!(code(src), codes::ASSIGN_TO_IMMUTABLE);
}

#[test]
fn a_nested_field_is_replaced_through_the_path_and_checked_at_the_root() {
    let src = r"
        struct Inner { n: Int, }
        struct Outer { inner: Inner, }
        fn main() -> Int { let mut o = Outer(inner: Inner(n: 1)); o.inner.n = 41; o.inner.n + 1 }
    ";
    assert_eq!(value(src), Value::Int(42));

    let immutable = r"
        struct Inner { n: Int, }
        struct Outer { inner: Inner, }
        fn main() -> Int { let o = Outer(inner: Inner(n: 1)); o.inner.n = 41; o.inner.n }
    ";
    assert_eq!(code(immutable), codes::ASSIGN_TO_IMMUTABLE);
}

#[test]
fn mutating_a_field_and_mutating_what_it_refers_to_are_different_things() {
    // The distinction already existed for sequence parameters; a record inherits it. `w`
    // is immutable, so `w.values` cannot be *replaced* — and pushing through it is not a
    // replacement, it is a mutation of the sequence the field names.
    let src = r"
        struct Work { values: Ints, }
        fn fill(w: Work) -> Int { ints_push(w.values, 7); ints_get(w.values, 0) }
        fn main() -> Int { fill(Work(values: ints_new())) }
    ";
    assert_eq!(value(src), Value::Int(7));

    let replaced = r"
        struct Work { values: Ints, }
        fn swap(w: Work) -> Int { w.values = ints_new(); 0 }
        fn main() -> Int { swap(Work(values: ints_new())) }
    ";
    assert_eq!(code(replaced), codes::ASSIGN_TO_IMMUTABLE);
}

// --- value semantics ------------------------------------------------------------------

#[test]
fn a_copy_gives_independent_scalars_and_shares_the_handles() {
    // `docs/spec.md`'s table, run: `count` is two `Int`s, `values` is one sequence.
    let src = r"
        struct Box { count: Int, values: Ints, }
        fn main() -> Int {
            let mut a = Box(count: 1, values: ints_new());
            let b = a;
            a.count = 7;
            ints_push(a.values, 5);
            a.count * 100 + b.count * 10 + ints_get(b.values, 0)
        }
    ";
    assert_eq!(value(src), Value::Int(715));
}

#[test]
fn a_nested_record_copies_by_value_too() {
    // Replacing a field of the inner record through one copy is invisible through the
    // other, because the inner record is a value inside the outer one rather than a
    // handle to a shared thing.
    let src = r"
        struct Inner { n: Int, }
        struct Outer { inner: Inner, }
        fn main() -> Int {
            let mut a = Outer(inner: Inner(n: 1));
            let b = a;
            a.inner.n = 9;
            a.inner.n * 10 + b.inner.n
        }
    ";
    assert_eq!(value(src), Value::Int(91));
}

#[test]
fn a_field_read_from_a_temporary_is_a_value_of_its_own() {
    let src = r"
        struct Holder { name: Str, }
        fn make() -> Holder { Holder(name: str_concat('ab', 'cd')) }
        fn main() -> Int { let s = make().name; str_len(s) }
    "
    .replace('\'', "\"");
    assert_eq!(value(&src), Value::Int(4));
}

// --- what is refused, and by which name -----------------------------------------------

#[test]
fn a_missing_field_is_refused_and_named() {
    let src = r"
        struct Point { x: Int, y: Int, }
        fn main() -> Int { let p = Point(x: 1); p.x }
    ";
    let (c, message) = sole(src);
    assert_eq!(c, codes::MISSING_FIELD);
    assert!(message.contains("`y`"), "{message}");
}

#[test]
fn an_unknown_field_is_refused_in_a_construction_and_in_a_projection() {
    let built = r"
        struct Point { x: Int, }
        fn main() -> Int { let p = Point(x: 1, z: 2); p.x }
    ";
    assert_eq!(code(built), codes::UNKNOWN_FIELD);

    let read = r"
        struct Point { x: Int, }
        fn main() -> Int { let p = Point(x: 1); p.z }
    ";
    assert_eq!(code(read), codes::UNKNOWN_FIELD);
}

#[test]
fn a_field_is_looked_for_on_the_type_it_is_read_from_and_nowhere_else() {
    // There is no global field namespace: `y` is a field of `B`, and that does not make
    // it a field of `A`.
    let src = r"
        struct A { x: Int, }
        struct B { y: Int, }
        fn main() -> Int { let a = A(x: 1); a.y }
    ";
    let (c, message) = sole(src);
    assert_eq!(c, codes::UNKNOWN_FIELD);
    assert!(message.contains("`A`"), "{message}");
}

#[test]
fn a_field_given_a_value_twice_is_refused() {
    let src = r"
        struct Point { x: Int, }
        fn main() -> Int { let p = Point(x: 1, x: 2); p.x }
    ";
    assert_eq!(code(src), codes::DUPLICATE_FIELD);
}

#[test]
fn two_fields_of_one_name_are_refused_at_the_declaration() {
    let src = r"
        struct Point { x: Int, x: Str, }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(src), codes::DUPLICATE_FIELD);
}

#[test]
fn an_initialiser_of_the_wrong_type_is_refused() {
    let src = r"
        struct Point { x: Int, }
        fn main() -> Int { let p = Point(x: 'no'); p.x }
    "
    .replace('\'', "\"");
    assert_eq!(code(&src), codes::TYPE_MISMATCH);
}

#[test]
fn a_replacement_of_the_wrong_type_is_refused() {
    let src = r"
        struct Point { x: Int, }
        fn main() -> Int { let mut p = Point(x: 1); p.x = 'no'; p.x }
    "
    .replace('\'', "\"");
    assert_eq!(code(&src), codes::TYPE_MISMATCH);
}

#[test]
fn projecting_something_that_is_not_a_record_is_refused() {
    let src = r"
        fn main() -> Int { let n = 1; n.x }
    ";
    assert_eq!(code(src), codes::INVALID_PROJECTION);
}

#[test]
fn two_records_of_one_name_in_one_module_are_refused() {
    let src = r"
        struct Point { x: Int, }
        struct Point { y: Int, }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(src), codes::DUPLICATE_TYPE);
}

#[test]
fn a_built_in_type_name_may_not_be_redefined() {
    let src = r"
        struct Int { x: Int, }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(src), codes::BUILTIN_TYPE_REDEFINED);
}

#[test]
fn a_record_with_no_fields_is_refused_by_name() {
    // `docs/spec.md` records why: `Marker()` is exactly how a call is written, and types
    // and functions are separate namespaces, so neither reading could be preferred.
    let src = r"
        struct Marker { }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(src), codes::UNSUPPORTED_CONSTRUCT);
}

#[test]
fn a_record_that_contains_itself_is_refused_as_a_layout() {
    let direct = r"
        struct Node { next: Node, }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(direct), codes::RECURSIVE_RECORD);

    let mutual = r"
        struct A { b: B, }
        struct B { a: A, }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(mutual), codes::RECURSIVE_RECORD);
}

#[test]
fn a_record_declared_after_its_use_still_resolves() {
    // Declaration order is not a dependency. Names are collected for the whole
    // compilation before any field type is resolved, which is what makes this work.
    let src = r"
        struct A { b: B, }
        struct B { n: Int, }
        fn main() -> Int { let a = A(b: B(n: 42)); a.b.n }
    ";
    assert_eq!(value(src), Value::Int(42));
}

#[test]
fn an_unknown_type_in_a_field_is_refused() {
    let src = r"
        struct A { b: Widget, }
        fn main() -> Int { 0 }
    ";
    assert_eq!(code(src), codes::UNKNOWN_TYPE);
}

#[test]
fn equality_is_not_offered_on_a_record() {
    // It would have to decide what a sequence field means — the same storage, or the
    // same contents — and the language has not chosen. Sequences and channels are
    // refused for the same reason; a record joins them rather than getting an exception.
    let src = r"
        struct Point { x: Int, }
        fn main() -> Int { let a = Point(x: 1); let b = Point(x: 1); if a == b { 1 } else { 0 } }
    ";
    assert_eq!(code(src), codes::INVALID_OPERANDS);
}

#[test]
fn two_records_with_the_same_fields_are_two_types() {
    // Nominal, always. There is no conversion, implicit or explicit.
    let src = r"
        struct A { x: Int, }
        struct B { x: Int, }
        fn take(b: B) -> Int { b.x }
        fn main() -> Int { take(A(x: 1)) }
    ";
    assert_eq!(code(src), codes::TYPE_MISMATCH);
}

#[test]
fn a_record_and_a_function_may_share_a_name() {
    // Types and functions are separate namespaces, and where the grammar puts a name is
    // what decides which one is meant.
    let src = r"
        struct Point { x: Int, }
        fn Point(n: Int) -> Int { n }
        fn main() -> Int { let p = Point(x: 1); p.x + Point(41) }
    ";
    assert_eq!(value(src), Value::Int(42));
}

// --- derived task safety ---------------------------------------------------------------

#[test]
fn a_record_of_task_safe_fields_may_cross() {
    let src = r"
        struct Message { id: Int, text: Str, reply: Chan, }
        fn worker(m: Message) -> Int { chan_send(m.reply, m.id + str_len(m.text)); 0 }
        fn main() -> Int {
            let c = chan_new(2);
            let m = Message(id: 40, text: 'ab', reply: c);
            scope { spawn worker(m); }
            chan_close(c);
            let got = ints_new();
            while chan_recv(c, got) { }
            ints_get(got, 0)
        }
    "
    .replace('\'', "\"");
    assert_eq!(value(&src), Value::Int(42));
}

#[test]
fn a_record_containing_a_sequence_may_not_cross_and_the_diagnostic_says_which_field() {
    let src = r"
        struct Work { values: Ints, }
        fn worker(w: Work) -> Int { 0 }
        fn main() -> Int {
            let w = Work(values: ints_new());
            scope { spawn worker(w); }
            0
        }
    ";
    let (c, message) = sole(src);
    assert_eq!(c, codes::ILLEGAL_TASK_CAPTURE);
    assert!(message.contains("values"), "{message}");
}

#[test]
fn wrapping_a_sequence_two_levels_deep_does_not_launder_it() {
    // The rule is derived, recursively, and the path is what the diagnostic reports —
    // `inner.values` rather than "this record cannot cross".
    let src = r"
        struct Inner { values: Ints, }
        struct Outer { inner: Inner, }
        fn worker(o: Outer) -> Int { 0 }
        fn main() -> Int {
            let o = Outer(inner: Inner(values: ints_new()));
            scope { spawn worker(o); }
            0
        }
    ";
    let (c, message) = sole(src);
    assert_eq!(c, codes::ILLEGAL_TASK_CAPTURE);
    assert!(message.contains("inner.values"), "{message}");
}
