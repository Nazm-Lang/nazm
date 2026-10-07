//! The Core IR evaluator — `nazm run`'s interpreter.
//!
//! Runs only Core IR, and Core IR exists only for a program that checked, so the shapes it
//! handles are the ones the checker guaranteed and the lowering stated. What remains at runtime
//! is what a type system cannot rule out: integer overflow and division by zero, an index
//! outside a string or sequence, a failing file operation. Each stops the program with a
//! diagnostic at the operator or call, and each behaves identically in every build — a bug
//! that reproduces only in one build configuration is the worst kind to be handed.
//!
//! # What it reads
//!
//! Core IR, and nothing else (`docs/architecture.md` §7.41). Not the syntax tree, and not the
//! checker's resolution: a local is a slot, a call names its callee, a field is an index, a
//! `break` names its loop, and `?` has already become the `match` it means. Until N39 this
//! walked the tree, found locals by searching frames for a name, and asked the resolution at
//! every call and field what the source had meant — a second lowering, done again on every
//! step, beside the native backend's.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use nazm_cir as cir;
use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

use cir::{Block, ExprKind, Stmt};
use nazm_sema::enums::EnumId;
use nazm_sema::intrinsic::Intrinsic;
use nazm_sema::numeric::{self, NumError, NumVal};
use nazm_sema::record::RecordId;
use nazm_sema::NumKind;

/// A runtime value.
///
/// `PartialEq` is written out rather than derived, because a sequence has no derivable
/// answer: `Mutex` does not compare, and comparing contents would be a language decision
/// nobody has made. Sequences therefore compare by **identity** — the same storage or
/// not — and the checker refuses `==` on them anyway (`N0304`), so the answer is never
/// observable from a Nazm program. It exists so the rest of `Value` can still be
/// compared.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    /// A number of any other numeric type (Gate 2): its kind and canonical bits, operated on
    /// only through [`nazm_sema::numeric`], the semantics both interpreters share.
    Num(NumVal),
    Bool(bool),
    /// **Bytes, not text.** `docs/spec.md` specifies `Str` as a byte sequence, and
    /// `str_slice` is allowed to split a multi-byte character — so the representation
    /// cannot be a Rust `String`, which refuses to hold the result.
    ///
    /// This was a real defect, not a hypothetical: with a `String` here,
    /// `str_byte(str_slice("é", 0, 1), 0)` produced 239 (the first byte of the
    /// replacement character `from_utf8_lossy` substituted) where the specification
    /// requires 195. The host's text type was acting as an unstated specification,
    /// which is exactly what the spec section warns against.
    ///
    /// A [`Str`] rather than a `Vec<u8>` since N8: a view over a backing allocation,
    /// which is what the native representation is, so the two agree about *when* string
    /// storage is reclaimed and not only about what the bytes are.
    Str(Str),
    /// A growable sequence of `Int`, **by reference**.
    ///
    /// `Rc<RefCell<…>>` is the reference semantics, stated rather than incidental: two
    /// bindings of the same sequence are the same storage, and a push through one is
    /// visible through the other. `docs/spec.md` says so, and the native representation
    /// — a pointer to a header — behaves identically by construction.
    ///
    /// Aliasing is not prevented and is not meant to be: `docs/spec.md`'s memory
    /// constitution calls this an **owned mutable handle**, two bindings on one are two
    /// references to one storage, and a push through either is visible through both.
    ///
    /// What *is* claimed, since N7, is reclamation: the storage goes when the last
    /// reference to it dies. That is [`Seq`]'s job here and a reference count on the
    /// header's job in a compiled program, and the two are compared case by case in
    /// `crates/nazm-cli/tests/memory.rs`. No ownership *safety* is claimed beyond it —
    /// nothing tracks a lifetime, because in this language nothing needs to: a handle can
    /// be stored only in a local binding, so there is nowhere for a dangling one to hide.
    /// `Arc<Mutex<…>>` rather than `Rc<RefCell<…>>` because the whole pipeline runs on
    /// a thread the interpreter owns (`on_interpreter_stack`), so a `Value` has to be
    /// `Send`. The lock is never contended — evaluation is single-threaded — and it is
    /// not a concurrency mechanism.
    Ints(Arc<Seq<Vec<i64>>>),
    /// The same, for `Str`.
    ///
    /// Each element is a `Str` **value**, which since N8 means each element holds its own
    /// reference to whatever backs it. That is `docs/spec.md`'s rule 3 applied to a
    /// container: what is stored survives the binding it came from, and what is read out
    /// survives being overwritten.
    Strs(Arc<Seq<Vec<Str>>>),
    /// A bounded channel of `Int`, by reference.
    ///
    /// The one value designed to be shared between tasks: its operations *are* the
    /// synchronisation, which is why `docs/spec.md` lets it cross a `spawn` while a
    /// sequence may not.
    Chan(Arc<Channel>),
    /// A user-defined record: an **inline aggregate**, by value.
    ///
    /// Not an `Arc`, and that is the semantic decision rather than a representation
    /// choice. `docs/spec.md` gives a record no object identity: assignment produces
    /// another record value whose fields are copied or shared according to **each field
    /// type's own law**, and a shared cell would have made every copy alias.
    ///
    /// Cloning a [`Rec`] clones its `Vec<Value>`, which clones each field — and every one
    /// of those clones is already the language's rule for that type: an `Int` copies, a
    /// `Str` shares its backing, a sequence handle and a channel take another reference.
    /// So composition is not implemented here; it *is* the field types' behaviour, which
    /// is exactly the property N9 exists to test.
    Record(Rec),
    /// A user-defined enum: a discriminant and **only the active variant's payload**.
    ///
    /// The representation is the semantics. There is no slot for an inactive variant and
    /// nothing to clone for one, so "copy only the active payload" and "release only the
    /// active payload" are not rules this has to follow — they are the only things it can
    /// do. `docs/spec.md` says an inactive variant owns nothing, and a value that carried
    /// storage for one would have made that a promise rather than a fact.
    Enum(EnumVal),
    /// A `Vec[T]`: a handle to shared, mutable, counted storage, by the sequence law. N11.
    ///
    /// One representation for every `T`, because the interpreter is **parametric**: what
    /// an element owns is what its own [`Value`] owns, so copying one in and out by
    /// `clone` applies `T`'s law without this code knowing what `T` is.
    Vec(Arc<Seq<Vec<Value>>>),
    /// A capability (N37): authority the checker has already verified. Nothing reads it —
    /// no built-in takes one, and it has no equality — so it carries only its kind, for a
    /// diagnostic to name. The only ones are those [`Interpreter::run_main`] hands `main`.
    Cap(nazm_sema::CapabilityKind),
    /// A function value (N50): a closure, with the copies it captured, or a named function,
    /// with none. Shared, because a copy of a function value is another reference to the same
    /// immutable thing — what the native closure object's count says.
    Func(Arc<FuncVal>),
}

/// What a function value calls, and what it carries.
#[derive(Debug)]
pub struct FuncVal {
    func: nazm_sema::FnId,
    /// What it captured, in order.
    carried: Vec<Value>,
}

impl FuncVal {
    /// A new function value — one object, counted as the native closure object is.
    fn new(func: nazm_sema::FnId, carried: Vec<Value>) -> Arc<Self> {
        CLOSURES_ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Arc::new(Self { func, carried })
    }
}

impl Drop for FuncVal {
    fn drop(&mut self) {
        CLOSURES_RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A record value: which record it is, and its fields in **declaration order**.
///
/// Declaration order rather than the canonical layout order, because this is the
/// interpreter and a field index here answers the same question
/// [`FieldRef`](nazm_sema::FieldRef) does. Where the fields physically sit is the native
/// backend's decision and nothing here knows it — which is the layout boundary,
/// respected by the reference implementation as well.
#[derive(Debug, Clone)]
pub struct Rec {
    def: RecordId,
    fields: Vec<Value>,
}

impl Value {
    /// The type a value of an implementable type has (N78): what a trait call dispatches on.
    /// `None` for a value no impl can be for.
    fn runtime_type(&self) -> Option<nazm_sema::Type> {
        Some(match self {
            Self::Int(_) => nazm_sema::Type::Int,
            Self::Bool(_) => nazm_sema::Type::Bool,
            Self::Str(_) => nazm_sema::Type::Str,
            Self::Record(r) => nazm_sema::Type::Record(r.def),
            Self::Enum(e) => nazm_sema::Type::Enum(e.def),
            _ => return None,
        })
    }
}

impl Rec {
    #[must_use]
    pub fn new(def: RecordId, fields: Vec<Value>) -> Self {
        Self { def, fields }
    }

    #[must_use]
    pub fn field(&self, at: u32) -> Option<&Value> {
        self.fields.get(at as usize)
    }

    fn field_mut(&mut self, at: u32) -> Option<&mut Value> {
        self.fields.get_mut(at as usize)
    }
}

/// An enum value: which enum, which variant, and that variant's payload in **declaration
/// order**.
///
/// Declaration order rather than the canonical tag or layout order, for the reason [`Rec`]
/// keeps declaration order: this is the interpreter, and an index here answers the same
/// question [`PayloadRef`](nazm_sema::PayloadRef) does. The numeric discriminant a backend
/// assigns is not here and is not needed — the variant is named by its position in the
/// definition, which is what resolution decided.
#[derive(Debug, Clone)]
pub struct EnumVal {
    def: EnumId,
    variant: u32,
    fields: Vec<Value>,
}

impl EnumVal {
    #[must_use]
    pub fn new(def: EnumId, variant: u32, fields: Vec<Value>) -> Self {
        Self {
            def,
            variant,
            fields,
        }
    }

    #[must_use]
    pub fn variant(&self) -> u32 {
        self.variant
    }

    #[must_use]
    pub fn field(&self, at: u32) -> Option<&Value> {
        self.fields.get(at as usize)
    }
}

/// A bounded queue with a close flag.
///
/// One mutex and one condition variable. Both senders and receivers wait on the same
/// variable and every change notifies all of them — which is not the fastest arrangement
/// and is the obviously correct one, and `docs/spec.md` promises no fairness anyway.
#[derive(Debug)]
pub struct Channel {
    state: Mutex<ChannelState>,
    changed: std::sync::Condvar,
    capacity: usize,
}

#[derive(Debug, Default)]
struct ChannelState {
    /// What is queued: `Int`s for a `Chan`, any value for a `Chan[T]` (N53). One channel for
    /// both, since a value carries no static type here and the semantics are one.
    queue: std::collections::VecDeque<Value>,
    closed: bool,
}

impl Channel {
    fn new(capacity: usize) -> Self {
        CHANNELS_ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            state: Mutex::new(ChannelState::default()),
            changed: std::sync::Condvar::new(),
            capacity,
        }
    }

    /// Queue a value, or report that the channel is closed — in which case **the value
    /// was not sent**, which is what `docs/spec.md` promises.
    fn send(&self, value: Value) -> Option<bool> {
        let mut state = self.state.lock().ok()?;
        while !state.closed && state.queue.len() >= self.capacity {
            state = self.changed.wait(state).ok()?;
        }
        if state.closed {
            return Some(false);
        }
        state.queue.push_back(value);
        drop(state);
        self.changed.notify_all();
        select_event();
        Some(true)
    }

    /// One look without waiting (N54): a value taken, the channel closed and drained, or
    /// neither.
    fn try_take(&self) -> Option<Ready> {
        let mut state = self.state.lock().ok()?;
        let ready = match state.queue.pop_front() {
            Some(v) => Ready::Value(v),
            None if state.closed => Ready::Closed,
            None => Ready::Empty,
        };
        drop(state);
        if matches!(ready, Ready::Value(_)) {
            self.changed.notify_all();
        }
        Some(ready)
    }

    /// Take a value, or report that the channel is closed **and drained**. Values queued
    /// before a close are still delivered: closing ends a channel, it does not discard
    /// what is in it.
    fn recv(&self) -> Option<Option<Value>> {
        let mut state = self.state.lock().ok()?;
        while state.queue.is_empty() && !state.closed {
            state = self.changed.wait(state).ok()?;
        }
        let taken = state.queue.pop_front();
        drop(state);
        self.changed.notify_all();
        Some(taken)
    }

    /// Idempotent, and wakes everyone: a blocked sender and a blocked receiver both have
    /// to find out, and neither can be left waiting on a channel nobody will touch again.
    fn close(&self) -> Option<()> {
        let mut state = self.state.lock().ok()?;
        state.closed = true;
        drop(state);
        self.changed.notify_all();
        select_event();
        Some(())
    }
}

/// What one look at a channel found (N54).
enum Ready {
    Value(Value),
    Closed,
    Empty,
}

/// The select event count (N54, §7.56): every send and close advances it, and a select waits
/// only while it has not moved since the select last looked — under its own lock, so no
/// wake-up between a look and a wait is lost. One for every channel: a select cannot wait on
/// several condition variables.
static SELECT_EVENTS: Mutex<u64> = Mutex::new(0);
static SELECT_WOKEN: std::sync::Condvar = std::sync::Condvar::new();

fn select_event() {
    if let Ok(mut g) = SELECT_EVENTS.lock() {
        *g = g.wrapping_add(1);
    }
    SELECT_WOKEN.notify_all();
}

/// What a select found: the lowest ready index, with the value it took if any; `-1` for no
/// channels; `-2` when the deadline passed first.
fn select(
    chans: &[Arc<Channel>],
    deadline: Option<std::time::Instant>,
) -> Option<(i64, Option<Value>)> {
    if chans.is_empty() {
        return Some((-1, None));
    }
    SCHED_SELECTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    loop {
        let seen = *SELECT_EVENTS.lock().ok()?;
        for (i, c) in chans.iter().enumerate() {
            let at = i64::try_from(i).unwrap_or(i64::MAX);
            match c.try_take()? {
                Ready::Value(v) => return Some((at, Some(v))),
                Ready::Closed => return Some((at, None)),
                Ready::Empty => {}
            }
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            SCHED_TIMEOUTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some((-2, None));
        }
        let mut g = SELECT_EVENTS.lock().ok()?;
        while *g == seen {
            match deadline {
                Some(d) => {
                    let now = std::time::Instant::now();
                    if now >= d {
                        break;
                    }
                    g = SELECT_WOKEN.wait_timeout(g, d - now).ok()?.0;
                }
                None => g = SELECT_WOKEN.wait(g).ok()?,
            }
        }
    }
}

/// Counted for the same reason [`Seq`] is: the host reclaims the mutex, the condition
/// variable and the queue when the last `Arc` goes, and *"the host does the right thing"*
/// is not evidence that the **language** reclaims a channel. The compiled program counts
/// the same event at the same moment, and `crates/nazm-cli/tests/memory.rs` compares them.
impl Drop for Channel {
    fn drop(&mut self) {
        CHANNELS_RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Ints(a), Self::Ints(b)) => Arc::ptr_eq(a, b),
            (Self::Strs(a), Self::Strs(b)) => Arc::ptr_eq(a, b),
            (Self::Chan(a), Self::Chan(b)) => Arc::ptr_eq(a, b),
            (Self::Vec(a), Self::Vec(b)) => Arc::ptr_eq(a, b),
            (Self::Cap(a), Self::Cap(b)) => a == b,
            // Field by field. Not what `==` means in a program — that is [`equal`], which
            // follows the language's law — and defined so that the rest of `Value` can
            // still be compared.
            (Self::Record(a), Self::Record(b)) => a.def == b.def && a.fields == b.fields,
            // Same enum, same active variant, equal payloads. Also the host's, not `==`.
            (Self::Enum(a), Self::Enum(b)) => {
                a.def == b.def && a.variant == b.variant && a.fields == b.fields
            }
            _ => false,
        }
    }
}

impl Eq for Value {}

/// Take a sequence's contents, or report the poisoning that can only follow a panic.
///
/// The lock is never contended, so this only fails if a previous evaluation panicked
/// while holding it — in which case the interpreter is already unsound and saying so is
/// better than continuing.
fn seq<T>(lock: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
    lock.lock().ok()
}

/// What a `Str` points into, and whether reclaiming it means anything.
///
/// N8's central problem, from `docs/architecture.md` §7.8: a `Str` may name a literal, a
/// process argument, a buffer the program allocated, or the interior of any of those, and
/// `{ pointer, length }` cannot tell them apart. So the *backing* is named separately from
/// the view, here and in the compiled program, and the two implementations classify the
/// same four origins the same way.
#[derive(Debug)]
pub enum Backing {
    /// Storage the program did not allocate and cannot reclaim: a literal, or a process
    /// argument. Uncounted on purpose — `docs/spec.md` reclaims what a program allocated,
    /// and counting storage nobody allocated as a leak would make every program leak.
    Immortal(Vec<u8>),
    /// Storage the program allocated. Counted, and released when the last view dies.
    Heap(Vec<u8>),
}

impl Backing {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Immortal(b) | Self::Heap(b) => b,
        }
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        if matches!(self, Self::Heap(_)) {
            STRINGS_RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// A `Str`: a view of `len` bytes starting at `start` inside a backing allocation.
///
/// This is the interpreter's half of the representation `docs/architecture.md` §7.8
/// settles. The native value is `{ pointer, length, owner }` and this is
/// `{ backing, start, length }` — the same three facts, because the same rule has to hold
/// in both: **the backing stays valid for every live view of it**, and a slice is a view,
/// not a copy.
///
/// Cloning shares the backing and copies nothing, which is rule 1: assignment never
/// observably copies a `Str`.
#[derive(Debug, Clone)]
pub struct Str {
    backing: Arc<Backing>,
    start: usize,
    len: usize,
}

impl Str {
    /// A literal, an argument, or anything else the program did not allocate.
    #[must_use]
    pub fn immortal(bytes: Vec<u8>) -> Self {
        Self::whole(Backing::Immortal(bytes))
    }

    /// Storage the program allocated: a concatenation, a join, a file's contents.
    #[must_use]
    pub fn heap(bytes: Vec<u8>) -> Self {
        STRINGS_ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::whole(Backing::Heap(bytes))
    }

    fn whole(backing: Backing) -> Self {
        let len = backing.bytes().len();
        Self {
            backing: Arc::new(backing),
            start: 0,
            len,
        }
    }

    /// The bytes this view names.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.backing
            .bytes()
            .get(self.start..self.start + self.len)
            .unwrap_or(&[])
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `str_slice`: a new view of the **same** backing, which is what keeps the backing
    /// alive for exactly as long as the slice is. `from` and `to` are already checked.
    #[must_use]
    pub fn slice(&self, from: usize, to: usize) -> Self {
        Self {
            backing: Arc::clone(&self.backing),
            start: self.start + from,
            len: to.saturating_sub(from),
        }
    }
}

impl PartialEq for Str {
    /// Bytes, never identity. `docs/spec.md`'s *Strings are bytes* makes two strings with
    /// the same bytes equal however they were built, and whether they share a backing is
    /// exactly the thing an implementation may decide for itself.
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}

impl Eq for Str {}

/// Sequence storage, and the two counters that prove it is reclaimed.
///
/// # Why this type exists at all
///
/// The storage could be a bare `Mutex<T>`: the host reclaims it when the last `Arc` to it
/// is dropped, which is exactly what `docs/spec.md`'s memory constitution requires. But
/// *"the host does the right thing"* is not evidence, and `docs/architecture.md` §7.7 says
/// why — a reference implementation has to model the language rather than inherit its
/// behaviour from whatever Rust happens to do. So the allocation and the reclamation are
/// counted here, the same two numbers a compiled program reports, and a test compares
/// them. If the interpreter ever stopped reclaiming, the count would say so instead of the
/// two implementations silently disagreeing.
#[derive(Debug, Default)]
pub struct Seq<T>(Mutex<T>);

impl<T> Seq<T> {
    fn new(value: T) -> Self {
        ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(Mutex::new(value))
    }
}

impl<T> Drop for Seq<T> {
    fn drop(&mut self) {
        RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl<T> std::ops::Deref for Seq<T> {
    type Target = Mutex<T>;

    fn deref(&self) -> &Mutex<T> {
        &self.0
    }
}

static ALLOCATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RECLAIMED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static STRINGS_ALLOCATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static STRINGS_RECLAIMED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CHANNELS_ALLOCATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CHANNELS_RECLAIMED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLOSURES_ALLOCATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLOSURES_RECLAIMED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SCHED_SPAWNED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SCHED_JOINED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SCHED_SELECTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SCHED_TIMEOUTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the scheduler did (N54): tasks started and joined, selects, and selects that timed
/// out. The same four counts a compiled program reports under `NAZM_SCHED_REPORT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedCounts {
    pub spawned: u64,
    pub joined: u64,
    pub selects: u64,
    pub timeouts: u64,
}

/// This process's scheduler counts.
#[must_use]
pub fn sched_counts() -> SchedCounts {
    let n = |c: &std::sync::atomic::AtomicU64| c.load(std::sync::atomic::Ordering::Relaxed);
    SchedCounts {
        spawned: n(&SCHED_SPAWNED),
        joined: n(&SCHED_JOINED),
        selects: n(&SCHED_SELECTS),
        timeouts: n(&SCHED_TIMEOUTS),
    }
}

/// What one class of storage did: created, reclaimed.
///
/// Three classes rather than one total, because they answer different questions and N8
/// makes them independently reclaimable. A slice is **not** an allocation: it takes a
/// reference to a backing that already exists, and counting it as one would report a leak
/// where nothing was allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub sequences: (u64, u64),
    pub strings: (u64, u64),
    pub channels: (u64, u64),
    /// Function values (N50): one object a closure or a named function's value makes.
    pub closures: (u64, u64),
}

/// What this process allocated and reclaimed, by class.
///
/// For the driver's `NAZM_MEMORY_REPORT` line and for the tests that compare the two
/// implementations. Not reachable from a Nazm program, for the reason the compiled
/// counters are not: the specification states *when* storage is reclaimed and deliberately
/// not *how*.
#[must_use]
pub fn memory_counts() -> Counts {
    let n = |c: &std::sync::atomic::AtomicU64| c.load(std::sync::atomic::Ordering::Relaxed);
    Counts {
        sequences: (n(&ALLOCATED), n(&RECLAIMED)),
        strings: (n(&STRINGS_ALLOCATED), n(&STRINGS_RECLAIMED)),
        channels: (n(&CHANNELS_ALLOCATED), n(&CHANNELS_RECLAIMED)),
        closures: (n(&CLOSURES_ALLOCATED), n(&CLOSURES_RECLAIMED)),
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Num(v) => f.write_str(&numeric::to_str(*v)),
            Self::Bool(v) => write!(f, "{v}"),
            // Rendering, not semantics: a `Str` that is not valid UTF-8 has no text
            // form, and showing the replacement character is a display decision. No
            // operation on `Str` goes through here.
            Self::Str(v) => write!(f, "{}", String::from_utf8_lossy(v.bytes())),
            // A sequence is never a program's result — `main` returns `Int` — so this
            // is for diagnostics only, and says what it is rather than dumping it.
            Self::Ints(v) => match v.lock() {
                Ok(v) => write!(f, "Ints({} items)", v.len()),
                Err(_) => f.write_str("Ints(unreadable)"),
            },
            Self::Strs(v) => match v.lock() {
                Ok(v) => write!(f, "Strs({} items)", v.len()),
                Err(_) => f.write_str("Strs(unreadable)"),
            },
            Self::Chan(_) => f.write_str("Chan"),
            Self::Cap(k) => f.write_str(k.type_name()),
            Self::Vec(v) => match v.lock() {
                Ok(v) => write!(f, "Vec({} items)", v.len()),
                Err(_) => f.write_str("Vec(unreadable)"),
            },
            // A record is never a program's result — `main` returns `Int` — so this is
            // for diagnostics, and says what it is rather than dumping it.
            Self::Record(r) => write!(f, "record({} fields)", r.fields.len()),
            Self::Enum(e) => write!(f, "variant {}({} fields)", e.variant, e.fields.len()),
            Self::Func(v) => write!(f, "function value({} carried)", v.carried.len()),
        }
    }
}

/// A runtime failure. Carries a diagnostic so it renders exactly like a compile error.
///
/// `clippy::result_large_err` is allowed on the evaluator below rather than boxing this.
/// Boxing would add an allocation to a path taken once per program, at most — an
/// evaluation error ends the run — and in exchange every construction site would gain a
/// wrapper that says nothing about what went wrong.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    /// Boxed deliberately. Evaluating one Nazm call nests a dozen host frames, each
    /// carrying a `Result<_, RuntimeError>`, so the error's size is multiplied by the
    /// recursion depth. Keeping it behind a pointer is what lets the depth bound trip
    /// before the stack runs out.
    pub diagnostic: Box<Diagnostic>,
}

impl From<Diagnostic> for RuntimeError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostic: Box::new(diagnostic),
        }
    }
}

/// One call's locals, by [`cir::LocalId`]: `None` for a slot not yet bound, or bound in a
/// region that has since been left.
///
/// # Why a slot vector and not names
///
/// Until N39 a frame was an association list of `(name, value)` pairs, one per lexical
/// scope, searched innermost-first at every read — itself the fix for a `HashMap` that spent
/// 29% of `bench/programs/arith.nz` hashing names (`docs/performance.md`). Resolution had
/// already decided which binding each name meant; the search repeated that decision at run
/// time. Core IR carries the answer, so a read is an index.
///
/// A region's exit clears the slots it owns ([`Block::owns`]), which is exactly when a
/// scope's frame used to be dropped — so a string backing or sequence a binding held is
/// released at the same moment it always was, and the counters in `tests/memory.rs` see no
/// difference.
type Frame = Vec<Option<Value>>;

/// How a statement finished.
///
/// A statement normally falls through to the next one; `break` and `continue` do not, and
/// the loop that has to act on them may be several blocks up. Returning the signal rather
/// than unwinding keeps the control flow visible in the types — every caller has to say
/// what it does with a `Break` it did not expect. Since N39 a `break` and a `continue` name
/// the loop they target, and only that loop consumes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow {
    /// Fell off the end. Carries the tail's value when the block had one.
    Fell(Option<Value>),
    Break(cir::LoopId),
    Continue(cir::LoopId),
    Return(Value),
}

impl Flow {
    fn is_fall_through(&self) -> bool {
        matches!(self, Self::Fell(_))
    }
}

/// Take the value of a sub-expression, or propagate the control flow that happened instead.
///
/// `return`, `break` and `continue` are statements, but a *block* is an expression — so
/// they can sit inside anything used as a value: `let x = if c { return 1; } else { 2 };`,
/// a call argument, an operand, a condition. When that happens the enclosing expression
/// has no value and never will, and the only correct thing to do is stop evaluating it
/// and carry the flow outward to whatever handles it.
///
/// Treating that case as an error was this evaluator's bug: programs that type-check —
/// the checker lets a diverging branch take the other branch's type — failed at run time
/// with "control flow escaped", a message about the interpreter rather than the program.
macro_rules! take {
    ($slf:ident, $f:expr, $e:expr, $env:expr) => {
        match $slf.eval($f, $e, $env)? {
            Flow::Fell(Some(v)) => v,
            Flow::Fell(None) => return Err(no_value($e)),
            left => return Ok(left),
        }
    };
}

/// A runtime error with one diagnostic.
fn fail(code: nazm_diag::Code, at: Span, message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        diagnostic: Box::new(Diagnostic::error(code, at, message)),
    }
}

/// An expression that was asked for a value and produced none.
///
/// Checking has already established that one arrives, so this is a checker bug rather
/// than a program's mistake — reported, not panicked on.
fn no_value(e: &cir::Expr) -> RuntimeError {
    RuntimeError {
        diagnostic: Box::new(
            Diagnostic::error(
                codes::TYPE_MISMATCH,
                e.at,
                "this produces no value, but one is needed here",
            )
            .help("a block's value is its final expression — one with no `;` after it"),
        ),
    }
}

/// Ten million iterations: high enough that no reasonable program reaches it, low enough
/// that a runaway one reports in seconds rather than consuming the machine.
pub const DEFAULT_MAX_ITERATIONS: u64 = 10_000_000;

/// Host stack reserved for each level of Nazm call depth.
///
/// Measured rather than guessed. On a debug build the deepest observed cost was about
/// 17 KB per nested call, for frames made deliberately heavy — nested calls and
/// arithmetic in the recursive position. This is that, doubled.
pub const STACK_BYTES_PER_CALL: usize = 32 * 1024;

/// Reserved on top of the per-call budget, for everything not proportional to call depth.
///
/// Chiefly the passes that run on this thread before the program does — parsing,
/// checking, lowering to Core IR and dropping the tree — which recurse with the program's
/// shape rather than with call depth. That requirement belongs to the parser, which sets
/// the nesting limit, so the pipeline takes the number from there ([`crate::TREE_STACK_BYTES`])
/// rather than restating it here where the two could drift apart.
pub const STACK_BASE_BYTES: usize = crate::TREE_STACK_BYTES;

/// The largest depth budget this interpreter will accept.
///
/// Not a safety boundary — the stack grows with the budget, so the guarantee holds above
/// the default too. It is the point past which the reservation is larger than any machine
/// will grant, and refusing with a number beats failing to spawn with an errno.
pub const MAX_SUPPORTED_DEPTH: usize = 1_000_000;

/// The stack a given depth budget needs.
///
/// This is what makes the bound a guarantee rather than a default that happens to fit: a
/// caller who raises `--max-depth` raises the reservation with it, so the diagnostic is
/// still reached before the stack is. A machine that cannot provide the reservation says
/// so ([`codes::INTERPRETER_UNAVAILABLE`]) instead of crashing part-way in.
#[must_use]
pub fn stack_for(max_depth: usize) -> usize {
    STACK_BASE_BYTES.saturating_add(max_depth.saturating_mul(STACK_BYTES_PER_CALL))
}

/// Run `f` on a thread with the stack `max_depth` calls require.
///
/// Scoped, so `f` may borrow the program. Panics inside `f` are resumed on the caller's
/// thread rather than swallowed.
///
/// Returns the spawn error rather than falling back to the caller's stack: running inline
/// would be quieter and would silently drop the guarantee [`DEFAULT_MAX_DEPTH`] rests on.
///
/// # Errors
///
/// If the thread cannot be spawned — resource exhaustion, not anything about the program.
pub fn on_interpreter_stack<T: Send>(
    max_depth: usize,
    f: impl FnOnce() -> T + Send,
) -> std::io::Result<T> {
    std::thread::scope(|scope| {
        Ok(std::thread::Builder::new()
            .name("nazm-eval".to_string())
            .stack_size(stack_for(max_depth))
            .spawn_scoped(scope, f)?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
    })
}

/// The default call-depth bound.
///
/// Deep enough for any recursion a person writes deliberately, shallow enough that the
/// stack is nowhere near exhausted when it trips — which is the whole point: the limit has
/// to be reached *before* the process dies, or there is no diagnostic to read.
///
/// The margin is measured, not assumed. `the_default_depth_is_reached_before_the_stack_is`
/// in `tests/language.rs` recurses to this bound and requires a diagnostic; a per-level
/// cost that grew past the headroom would fail it rather than turning into an unexplained
/// abort in someone's terminal.
pub const DEFAULT_MAX_DEPTH: usize = 512;

/// What the interpreter will spend on a program.
///
/// Neither of these is a language rule, and `docs/spec.md` says so: Nazm bounds neither
/// iteration nor recursion. They are this interpreter's guards, and they exist for the
/// same reason — a program that consumes the machine and says nothing is worse than one
/// that stops and reports where it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_iterations: u64,
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

/// Join anything still running when an interpreter is dropped off the normal path.
///
/// `Stmt::Scope` joins its own tasks; this covers the case it cannot — a panic unwinding
/// through a scope, where the handles would otherwise be dropped and the threads detached.
/// `std::thread::scope` gets this for free; starting tasks eagerly means writing it down.
impl Drop for Interpreter {
    fn drop(&mut self) {
        for level in std::mem::take(&mut self.pending) {
            for handle in level {
                drop(handle.join());
            }
        }
    }
}

#[derive(Debug)]
pub struct Interpreter {
    /// The program, shared with every task.
    ///
    /// Owned through an `Arc` rather than borrowed, because a task's closure has to be
    /// `'static`: it outlives the stack frame that started it by construction, and
    /// nothing borrowed from the caller can be handed to it. The alternative — running
    /// the whole scope body inside `std::thread::scope` — means threading the scope
    /// handle through every method of the interpreter, including the ones that have
    /// nothing to do with concurrency.
    ///
    /// Core IR only. The syntax tree was cloned in here until N39, and the resolution with
    /// it; neither is needed, so neither is kept, and a run holds one representation of the
    /// program rather than three.
    core: Arc<cir::Program>,
    /// Tasks started by the innermost `scope`, innermost last.
    ///
    /// A stack, because a scope may contain another and the inner one joins first.
    ///
    /// A task starts **when its `spawn` runs**, not at the closing brace. Collecting
    /// them and starting them all at the end reads as an optimisation and is a
    /// deadlock: a body that fills a bounded channel before its readers exist blocks
    /// forever, and that is the shape of every worker pool. The first concurrent
    /// program written against this interpreter hung on exactly that.
    pending: Vec<Vec<JoinHandle<Result<(), RuntimeError>>>>,
    /// Bounded so an accidental infinite recursion reports rather than aborting the
    /// process with a stack overflow, which produces no diagnostic at all.
    depth: usize,
    max_depth: usize,
    /// Loop iterations remaining.
    ///
    /// An interpreter guard, not a language rule — `docs/spec.md` is explicit that Nazm
    /// does not bound iteration. With no `break` and no I/O, a non-terminating program
    /// produces nothing whatsoever, and a tool that hangs is worse than one that reports.
    iterations_left: u64,
    /// Calls made, for a meter (N69): every function body entered, `main` excepted.
    calls: u64,
    /// The program's own arguments, **excluding** the program name — `nazm run f.nz --
    /// a b` gives `["a", "b"]`. Excluding it is what makes `arg(i)` mean the same thing
    /// interpreted and compiled: a native `argv[0]` is a path nobody should depend on.
    args: Vec<String>,
}

/// A shape the lowering never produces, reported rather than assumed away.
fn unreachable_flow<T>(at: Span) -> Result<T, RuntimeError> {
    Err(fail(
        codes::COMPILER_DEFECT,
        at,
        "the interpreter met an expression it cannot evaluate",
    ))
}

/// A fresh frame for `f`, with its parameters bound to `args`.
fn frame(f: &cir::Function, args: Vec<Value>) -> Frame {
    let mut frame: Frame = vec![None; f.locals.len()];
    for (slot, v) in frame.iter_mut().zip(args) {
        *slot = Some(v);
    }
    frame
}

impl Interpreter {
    /// An interpreter for a checked program's Core IR, which it takes.
    #[must_use]
    pub fn new(core: cir::Program) -> Self {
        Self {
            core: Arc::new(core),
            pending: Vec::new(),
            depth: 0,
            max_depth: DEFAULT_MAX_DEPTH,
            iterations_left: DEFAULT_MAX_ITERATIONS,
            calls: 0,
            args: Vec::new(),
        }
    }

    /// The arguments `arg(i)` will see.
    #[must_use]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Change both budgets. `nazm run --max-iterations` and `--max-depth` set these.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.iterations_left = limits.max_iterations;
        self.max_depth = limits.max_depth;
        self
    }

    /// What remains of the iteration budget (N69: the simulator's meter is what one transaction
    /// spent of a fresh one).
    #[must_use]
    pub fn iterations_left(&self) -> u64 {
        self.iterations_left
    }

    /// How many calls have been made (N69).
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Give both budgets afresh (N67): a reference evaluated element by element holds each
    /// element to the budget a single run would have, not the whole sequence to one.
    pub fn reset_limits(&mut self, limits: Limits) {
        self.iterations_left = limits.max_iterations;
        self.max_depth = limits.max_depth;
    }

    /// Evaluate `main` and return its value.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        let core = Arc::clone(&self.core);
        // The **root** module's `main`, which the lowering recorded as the entry. An
        // imported module may have one of its own and it is not an entry point;
        // `docs/spec.md` says so and this is where that is true at runtime.
        let Some(main) = core.entry.and_then(|e| core.function(e)) else {
            return Err(fail(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "this program has no `main` function",
            ));
        };
        // The root of authority (N37): each capability parameter `main` declares is handed
        // one, and nothing else in a program can make one. Checking has refused a `main`
        // with a parameter of any other type.
        let roots: Vec<Value> = main
            .param_locals()
            .iter()
            .map(|l| match l.ty {
                nazm_sema::Type::Cap(k) => Value::Cap(k),
                _ => Value::Int(0),
            })
            .collect();
        self.call_body(main, frame(main, roots))
    }

    /// Evaluate the parameterless function `f` and return its value (N65, comptime): a function
    /// of no parameters holds no capability, so whatever it reaches, it reaches by computing.
    /// The caller has held it to that; the budgets of [`Self::with_limits`] still bound it.
    pub fn run_constant(&mut self, f: nazm_sema::FnId) -> Result<Value, RuntimeError> {
        let core = Arc::clone(&self.core);
        let Some(func) = core.function(f).filter(|g| g.params == 0) else {
            return Err(fail(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "no parameterless function to evaluate",
            ));
        };
        self.call_body(func, frame(func, Vec::new()))
    }

    /// Call `f` with `args` (N67: the reference an accelerator's results are held to). The caller
    /// passes values of the parameters' types; a function holding a capability is not called.
    pub fn call(&mut self, f: nazm_sema::FnId, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let core = Arc::clone(&self.core);
        let Some(func) = core
            .function(f)
            .filter(|g| g.params as usize == args.len() && g.authority().is_empty())
        else {
            return Err(fail(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "no function of that arity to call",
            ));
        };
        self.call_body(func, frame(func, args))
    }

    /// Run a function body and produce its value.
    ///
    /// `return` and falling off the end with a tail are the two ways a function produces
    /// one; checking guarantees every path does one or the other, so the remaining cases
    /// are diagnostics rather than panics.
    fn call_body(&mut self, f: &cir::Function, mut env: Frame) -> Result<Value, RuntimeError> {
        match self.block(f, &f.body, &mut env)? {
            Flow::Return(v) | Flow::Fell(Some(v)) => Ok(v),
            Flow::Fell(None) => Err(fail(
                codes::TYPE_MISMATCH,
                f.body.at,
                format!("`{}` finished without producing a value", f.name),
            )),
            Flow::Break(_) | Flow::Continue(_) => Err(fail(
                codes::NOT_IN_A_LOOP,
                f.body.at,
                format!("loop control escaped from `{}`", f.name),
            )),
        }
    }

    /// Run a region, then release what it owns.
    ///
    /// The release is the language's: a binding's value is dropped when the region it was
    /// bound in is left, by falling through, by any transfer, or by a failure — the moment a
    /// scope's frame used to be popped.
    fn block(
        &mut self,
        f: &cir::Function,
        b: &Block,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let out = self.block_inner(f, b, env);
        for l in &b.owns {
            if let Some(slot) = env.get_mut(l.index()) {
                *slot = None;
            }
        }
        out
    }

    fn block_inner(
        &mut self,
        f: &cir::Function,
        b: &Block,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        for stmt in &b.stmts {
            let flow = self.stmt(f, stmt, env)?;
            if !flow.is_fall_through() {
                // Leaves this block and travels outward to whatever handles it.
                return Ok(flow);
            }
        }
        match &b.tail {
            Some(tail) => self.eval(f, tail, env),
            None => Ok(Flow::Fell(None)),
        }
    }

    /// `match`, in either position.
    ///
    /// The scrutinee is evaluated **once**, into a value this expression owns, and the arm
    /// runs with the payload bound into locals its region owns. `docs/spec.md` specifies
    /// value evaluation rather than a borrow of the caller's storage, so `match x { … }`
    /// leaves `x` untouched and `match make() { … }` has a clear owner for the temporary.
    ///
    /// The bindings are **clones of the payload values**, which is the same operation a
    /// projection out of a record performs and the same rule: what comes out holds its own
    /// reference to whatever backs it, so an arm may return it and it survives the
    /// scrutinee. The scrutinee is dropped when this function returns, after the arm's
    /// value has been established — which is the ordering the whole milestone turns on.
    ///
    /// A `?` is one of these since N39 — its `Err` arm an ordinary `return` — so its error
    /// path travels out through loops, scopes and blocks exactly as a written `return` does,
    /// and a scope it leaves joins its tasks on the way.
    fn matching(
        &mut self,
        f: &cir::Function,
        m: &cir::Match,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let value = take!(self, f, &m.scrutinee, env);
        let Value::Enum(subject) = value else {
            return Err(fail(
                codes::MATCH_ON_NON_ENUM,
                m.scrutinee.at,
                "this is not an enum",
            ));
        };
        // The variant selects the arm. Checking has established that exactly one arm names
        // each variant, so the first match is the only match and there is no fallthrough to
        // express — and the arm names its variant by the same index the value carries.
        let Some(arm) = m.arms.iter().find(|a| a.variant == subject.variant) else {
            // Unreachable for a checked program: `N0343` refuses a `match` that leaves a
            // variant uncovered. Reported rather than assumed away, and deliberately not
            // as a language-visible trap — it says the compiler is wrong, not the program.
            return Err(RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::NON_EXHAUSTIVE_MATCH,
                        m.at,
                        "no arm covers this value's variant",
                    )
                    .help("this is a bug in the compiler: exhaustiveness is checked"),
                ),
            });
        };
        for b in &arm.bindings {
            let Some(v) = subject.field(b.field) else {
                return Err(fail(
                    codes::UNKNOWN_FIELD,
                    b.at,
                    "this variant has no such payload field",
                ));
            };
            if let Some(slot) = env.get_mut(b.local.index()) {
                *slot = Some(v.clone());
            }
        }
        // `subject` is dropped here, after the arm's value has been established — which is
        // what keeps a returned payload valid. A `_` field is in `subject` like any other
        // and goes with it; declining to name a value is not declining to own it.
        self.block(f, &arm.body, env)
    }

    fn stmt(
        &mut self,
        f: &cir::Function,
        stmt: &Stmt,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        // What a statement's own value is: nothing. A flow still travels outward — an `if`
        // in statement position may still `return` or `break`.
        let discard = |flow: Flow| match flow {
            Flow::Fell(_) => Flow::Fell(None),
            other => other,
        };
        match stmt {
            Stmt::Let { local, value } => {
                let v = take!(self, f, value, env);
                if let Some(slot) = env.get_mut(local.index()) {
                    *slot = Some(v);
                }
                Ok(Flow::Fell(None))
            }
            Stmt::Assign {
                local,
                path,
                value,
                at,
            } => self.assign(f, *local, path, value, *at, env),
            Stmt::Eval(e) => Ok(discard(self.eval(f, e, env)?)),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                let c = take!(self, f, cond, env);
                let flow = if as_bool(&c, cond)? {
                    self.block(f, then, env)?
                } else if let Some(b) = otherwise {
                    self.block(f, b, env)?
                } else {
                    Flow::Fell(None)
                };
                Ok(discard(flow))
            }
            Stmt::Match(m) => Ok(discard(self.matching(f, m, env)?)),
            // A loop consumes its own `break` and `continue`, but a `return` inside one
            // leaves the function and must travel through — as must a `break` naming an
            // enclosing loop.
            Stmt::Loop { id, cond, body, at } => self.while_loop(f, *id, cond, body, *at, env),
            Stmt::Block(b) => Ok(discard(self.block(f, b, env)?)),
            // Through `take!`: `return (if c { return 1; } else { 2 });` is legal, and
            // the inner `return` is the one that wins.
            Stmt::Return { value, .. } => Ok(Flow::Return(take!(self, f, value, env))),
            Stmt::Break { target, .. } => Ok(Flow::Break(*target)),
            Stmt::Continue { target, .. } => Ok(Flow::Continue(*target)),
            Stmt::Scope { body, .. } => {
                // Tasks run while the body does. The closing brace joins every one of
                // them, and joins them *before* propagating a failure of the body —
                // returning early would leave tasks running past the scope that promised
                // to contain them, which is the one guarantee this construct exists for.
                self.pending.push(Vec::new());
                let flow = self.block(f, body, env);
                let joined = Self::join_all(self.pending.pop().unwrap_or_default());
                // The body's failure wins: a task that failed because the body stopped
                // feeding it is a consequence, not the cause.
                let flow = flow?;
                joined?;
                // A `scope` produces nothing; a `break` or `return` inside one still
                // travels outward, after the join.
                Ok(discard(flow))
            }
            Stmt::Spawn {
                callee, args, at, ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(take!(self, f, a, env));
                }
                // Checked before starting anything: a task with no scope to join it is
                // the one case where starting it would be unrecoverable. Unreachable —
                // the checker reports `N0320`.
                if self.pending.is_empty() {
                    return Err(fail(
                        codes::SPAWN_OUTSIDE_SCOPE,
                        *at,
                        "`spawn` reached the interpreter with no scope to join it",
                    ));
                }
                let task = self.start_task(*callee, values, *at)?;
                if let Some(scope) = self.pending.last_mut() {
                    scope.push(task);
                }
                Ok(Flow::Fell(None))
            }
        }
    }

    /// Evaluate a built-in.
    ///
    /// Arity and argument types were established by checking, so a mismatch here is a
    /// compiler bug and is reported as one rather than assumed away. The failures that
    /// remain are the specified ones: an index outside a string (`N0405`) and an
    /// overflowing length, which cannot happen for a string that fits in memory but is
    /// handled rather than truncated.
    fn intrinsic(
        &mut self,
        i: Intrinsic,
        args: &[Value],
        span: nazm_span::Span,
    ) -> Result<Value, RuntimeError> {
        let bug = |what: &str| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    span,
                    format!("`{}` was called with {what}", i.name()),
                )
                .help("this is a bug in the interpreter; the checker should have caught it"),
            ),
        };
        // The *message* is the same sentence the compiled program writes, because the
        // two implementations reporting the same failure differently is a difference a
        // reader has to reconcile for no reason. What the interpreter knows and the
        // backend cannot — the actual index and length — goes in the help, where it
        // costs nothing and is not part of the contract.
        let out_of_bounds = |message: &str, detail: String| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::INDEX_OUT_OF_BOUNDS, span, message).help(format!(
                    "{detail}. `Str` is indexed in bytes, from 0; `str_len` gives the \
                     length in bytes"
                )),
            ),
        };

        /// A length as an `Int`, or `None` on a host where it would not fit.
        fn index_fits(n: usize) -> Option<Value> {
            i64::try_from(n).ok().map(Value::Int)
        }

        // The path and the host's reason go in the help, not the message, so the two
        // implementations report the same sentence for the same failure — the native
        // side knows neither at compile time.
        let io_failed = |message: &str, path: &str, e: &std::io::Error| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::IO_FAILED, span, message).help(format!("`{path}`: {e}")),
            ),
        };

        if let Some(v) = numeric_intrinsic(i, args, span) {
            return v;
        }

        match (i, args) {
            (Intrinsic::StrLen, [Value::Str(s)]) => {
                // A length that does not fit in `Int` cannot arise from a string that
                // fits in memory on any 64-bit host, but saying so is cheaper than
                // discovering it.
                i64::try_from(s.len()).map_or_else(
                    |_| Err(bug("a string longer than `Int` can measure")),
                    |n| Ok(Value::Int(n)),
                )
            }
            (Intrinsic::StrByte, [Value::Str(s), Value::Int(at)]) => {
                let index = usize::try_from(*at).ok().filter(|n| *n < s.len());
                let Some(index) = index else {
                    return Err(out_of_bounds(
                        "this byte index is outside the `Str`",
                        format!("asked for byte {at} of a `Str` {} bytes long", s.len()),
                    ));
                };
                s.bytes().get(index).map_or_else(
                    || Err(bug("an index that passed its own bounds check")),
                    |b| Ok(Value::Int(i64::from(*b))),
                )
            }
            (Intrinsic::StrSlice, [Value::Str(s), Value::Int(from), Value::Int(to)]) => {
                let ok =
                    *from >= 0 && *to >= *from && usize::try_from(*to).is_ok_and(|t| t <= s.len());
                if !ok {
                    return Err(out_of_bounds(
                        "this slice is not inside the `Str`",
                        format!("asked for {from}..{to} of a `Str` {} bytes long", s.len()),
                    ));
                }
                let (a, b) = (
                    usize::try_from(*from).unwrap_or(0),
                    usize::try_from(*to).unwrap_or(0),
                );
                // Bytes, not characters: slicing is allowed to land inside a multi-byte
                // sequence, and `docs/spec.md` says what that produces.
                //
                // A **view**, not a copy, and not an allocation: the result shares the
                // backing it was cut from and keeps it alive for exactly as long as it
                // lives. That is the property N8 exists to establish, and it is the same
                // property `getelementptr` gives the compiled program.
                Ok(Value::Str(s.slice(a, b)))
            }
            (Intrinsic::StrConcat, [Value::Str(a), Value::Str(b)]) => {
                let mut out = Vec::with_capacity(a.len() + b.len());
                out.extend_from_slice(a.bytes());
                out.extend_from_slice(b.bytes());
                Ok(Value::Str(Str::heap(out)))
            }
            (Intrinsic::IntToStr, [Value::Int(n)]) => {
                Ok(Value::Str(Str::heap(n.to_string().into_bytes())))
            }
            (Intrinsic::IntsNew, []) => Ok(Value::Ints(Arc::new(Seq::new(Vec::new())))),
            (Intrinsic::StrsNew, []) => Ok(Value::Strs(Arc::new(Seq::new(Vec::new())))),
            (Intrinsic::IntsLen, [Value::Ints(a)]) => seq(a)
                .and_then(|a| index_fits(a.len()))
                .map_or_else(|| Err(bug("a sequence that could not be read")), Ok),
            (Intrinsic::StrsLen, [Value::Strs(a)]) => seq(a)
                .and_then(|a| index_fits(a.len()))
                .map_or_else(|| Err(bug("a sequence that could not be read")), Ok),
            // The left-to-right checked sum (N66): the loop it stands for, as a function, failing
            // with that loop's addition's diagnostic at the first running sum that overflows.
            (Intrinsic::IntsSum, [Value::Int(start), Value::Ints(a)]) => {
                let Some(a) = seq(a) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let mut sum = *start;
                for x in a.iter() {
                    sum = sum.checked_add(*x).ok_or_else(|| RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::INTEGER_OVERFLOW,
                                span,
                                "this addition overflows `Int`",
                            )
                            .help(
                                "`Int` is a 64-bit signed integer; overflow stops the program rather than wrapping",
                            ),
                        ),
                    })?;
                }
                Ok(Value::Int(sum))
            }
            (Intrinsic::IntsGet, [Value::Ints(a), Value::Int(at)]) => {
                let Some(a) = seq(a) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let Some(v) = usize::try_from(*at).ok().and_then(|i| a.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked for item {at} of an `Ints` holding {}", a.len()),
                    ));
                };
                Ok(Value::Int(*v))
            }
            (Intrinsic::StrsGet, [Value::Strs(a), Value::Int(at)]) => {
                let Some(a) = seq(a) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let Some(v) = usize::try_from(*at).ok().and_then(|i| a.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked for item {at} of a `Strs` holding {}", a.len()),
                    ));
                };
                // The element's own reference, cloned. What comes back keeps the
                // backing alive on its own, which is why overwriting the element
                // afterwards cannot invalidate it (`docs/spec.md`, rule 4).
                Ok(Value::Str(v.clone()))
            }
            (Intrinsic::IntsSet, [Value::Ints(a), Value::Int(at), Value::Int(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                let len = a.len();
                let Some(slot) = usize::try_from(*at).ok().and_then(|i| a.get_mut(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked to write item {at} of an `Ints` holding {len}"),
                    ));
                };
                *slot = *v;
                Ok(Value::Int(*at))
            }
            (Intrinsic::StrsSet, [Value::Strs(a), Value::Int(at), Value::Str(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                let len = a.len();
                let Some(slot) = usize::try_from(*at).ok().and_then(|i| a.get_mut(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked to write item {at} of a `Strs` holding {len}"),
                    ));
                };
                // The new element takes its own reference **before** the old one is
                // dropped, which the assignment does in that order. Replacing an element
                // with a view of the same backing is therefore safe rather than a
                // release followed by a use.
                slot.clone_from(v);
                Ok(Value::Int(*at))
            }
            (Intrinsic::IntsPush, [Value::Ints(a), Value::Int(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.push(*v);
                index_fits(a.len())
                    .map_or_else(|| Err(bug("a sequence longer than `Int` can measure")), Ok)
            }
            (Intrinsic::StrsPush, [Value::Strs(a), Value::Str(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.push(v.clone());
                index_fits(a.len())
                    .map_or_else(|| Err(bug("a sequence longer than `Int` can measure")), Ok)
            }
            (Intrinsic::IntsPop, [Value::Ints(a)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.pop().map_or_else(
                    || {
                        Err(out_of_bounds(
                            "this sequence is empty",
                            "asked to remove the last item of an empty `Ints`".to_owned(),
                        ))
                    },
                    |v| Ok(Value::Int(v)),
                )
            }
            (Intrinsic::ChanNew | Intrinsic::ChanNewOf, [Value::Int(capacity)]) => {
                let Ok(capacity) = usize::try_from(*capacity) else {
                    return Err(out_of_bounds(
                        "a channel needs a capacity of at least 1",
                        format!("asked for a capacity of {capacity}"),
                    ));
                };
                if capacity == 0 {
                    return Err(out_of_bounds(
                        "a channel needs a capacity of at least 1",
                        "asked for a capacity of 0".to_owned(),
                    ));
                }
                Ok(Value::Chan(Arc::new(Channel::new(capacity))))
            }
            (Intrinsic::ChanSend, [Value::Chan(c), v @ Value::Int(_)])
            | (Intrinsic::ChanSendOf, [Value::Chan(c), v]) => {
                // A copy: the sender's binding is untouched (N53).
                c.send(v.clone()).map_or_else(
                    || Err(bug("a channel that could not be locked")),
                    |sent| Ok(Value::Bool(sent)),
                )
            }
            (Intrinsic::ChanRecv, [Value::Chan(c), Value::Ints(out)]) => {
                let Some(taken) = c.recv() else {
                    return Err(bug("a channel that could not be locked"));
                };
                match taken {
                    Some(Value::Int(v)) => {
                        let Some(mut out) = seq(out) else {
                            return Err(bug("a sequence that could not be written"));
                        };
                        out.push(v);
                        Ok(Value::Bool(true))
                    }
                    Some(_) => Err(bug("an `Int` channel held something else")),
                    None => Ok(Value::Bool(false)),
                }
            }
            // Moved out of the channel onto the vector (N53).
            (Intrinsic::ChanRecvOf, [Value::Chan(c), Value::Vec(out)]) => {
                let Some(taken) = c.recv() else {
                    return Err(bug("a channel that could not be locked"));
                };
                match taken {
                    Some(v) => {
                        let Some(mut out) = seq(out) else {
                            return Err(bug("a vector that could not be written"));
                        };
                        out.push(v);
                        Ok(Value::Bool(true))
                    }
                    None => Ok(Value::Bool(false)),
                }
            }
            // N54: the lowest ready channel's value moved onto `out`, or its index alone.
            (Intrinsic::ChanSelectOf, [Value::Vec(cs), Value::Vec(out)])
            | (Intrinsic::ChanSelectUntil, [Value::Vec(cs), Value::Vec(out), Value::Int(_)]) => {
                let deadline = match args {
                    [_, _, Value::Int(ms)] => Some(
                        std::time::Instant::now()
                            + std::time::Duration::from_millis(u64::try_from(*ms).unwrap_or(0)),
                    ),
                    _ => None,
                };
                let chans: Vec<Arc<Channel>> = {
                    let Some(cs) = seq(cs) else {
                        return Err(bug("a vector that could not be read"));
                    };
                    cs.iter()
                        .filter_map(|v| match v {
                            Value::Chan(c) => Some(Arc::clone(c)),
                            _ => None,
                        })
                        .collect()
                };
                let Some((at, taken)) = select(&chans, deadline) else {
                    return Err(bug("a channel that could not be locked"));
                };
                if let Some(v) = taken {
                    let Some(mut out) = seq(out) else {
                        return Err(bug("a vector that could not be written"));
                    };
                    out.push(v);
                }
                Ok(Value::Int(at))
            }
            (Intrinsic::TimeNowMs, []) => {
                static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let base = BASE.get_or_init(std::time::Instant::now);
                Ok(Value::Int(
                    i64::try_from(base.elapsed().as_millis()).unwrap_or(i64::MAX),
                ))
            }
            (Intrinsic::ChanClose | Intrinsic::ChanCloseOf, [Value::Chan(c)]) => {
                c.close().map_or_else(
                    || Err(bug("a channel that could not be locked")),
                    |()| Ok(Value::Int(0)),
                )
            }
            (Intrinsic::StrJoin, [Value::Strs(parts), Value::Str(sep)]) => {
                let Some(parts) = seq(parts) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let total: usize = parts.iter().map(Str::len).sum::<usize>()
                    + sep.len() * parts.len().saturating_sub(1);
                let mut out = Vec::with_capacity(total);
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.extend_from_slice(sep.bytes());
                    }
                    out.extend_from_slice(part.bytes());
                }
                Ok(Value::Str(Str::heap(out)))
            }
            (Intrinsic::StrsPop, [Value::Strs(a)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.pop().map_or_else(
                    || {
                        Err(out_of_bounds(
                            "this sequence is empty",
                            "asked to remove the last item of an empty `Strs`".to_owned(),
                        ))
                    },
                    |v| Ok(Value::Str(v)),
                )
            }

            // --- `Vec[T]` ------------------------------------------------------------
            //
            // The sequence law, with the element's own law for what goes in and comes out:
            // `clone` is a copy by `T`'s law, so a `Str` element takes its own reference to
            // its backing and a `Vec` element takes one to its storage, and dropping an
            // element releases exactly what it owned.
            (Intrinsic::VecNew, []) => Ok(Value::Vec(Arc::new(Seq::new(Vec::new())))),
            (Intrinsic::VecLen, [Value::Vec(a)]) => seq(a)
                .and_then(|a| index_fits(a.len()))
                .map_or_else(|| Err(bug("a sequence that could not be read")), Ok),
            (Intrinsic::VecGet, [Value::Vec(a), Value::Int(at)]) => {
                let Some(a) = seq(a) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let Some(v) = usize::try_from(*at).ok().and_then(|i| a.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked for item {at} of a `Vec` holding {}", a.len()),
                    ));
                };
                // A copy of its own: overwriting the element afterwards cannot touch it.
                Ok(v.clone())
            }
            (Intrinsic::VecSet, [Value::Vec(a), Value::Int(at), v]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                let len = a.len();
                let Some(slot) = usize::try_from(*at).ok().and_then(|i| a.get_mut(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked to write item {at} of a `Vec` holding {len}"),
                    ));
                };
                // The new value's copy is made before the old one is dropped, which is the
                // order the assignment runs in.
                *slot = v.clone();
                Ok(Value::Int(*at))
            }
            (Intrinsic::VecPush, [Value::Vec(a), v]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.push(v.clone());
                index_fits(a.len())
                    .map_or_else(|| Err(bug("a sequence longer than `Int` can measure")), Ok)
            }
            (Intrinsic::VecPop, [Value::Vec(a)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                // A transfer: the element leaves the vector and is this value now.
                a.pop().map_or_else(
                    || {
                        Err(out_of_bounds(
                            "this sequence is empty",
                            "asked to remove the last item of an empty `Vec`".to_owned(),
                        ))
                    },
                    Ok,
                )
            }

            // --- the outside world ---------------------------------------------
            (Intrinsic::ArgsCount, []) => index_fits(self.args.len())
                .map_or_else(|| Err(bug("more arguments than `Int` can count")), Ok),
            (Intrinsic::Arg, [Value::Int(at)]) => {
                let Some(a) = usize::try_from(*at).ok().and_then(|i| self.args.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the arguments",
                        format!("asked for argument {at} of {} given", self.args.len()),
                    ));
                };
                // The process's arguments are the operating system's storage, not
                // this program's: **external**, so nothing counts them and nothing
                // reclaims them. The compiled program says the same thing by giving an
                // `arg` no owner at all.
                Ok(Value::Str(Str::immortal(a.clone().into_bytes())))
            }
            (Intrinsic::ReadFile, [Value::Str(path)]) => {
                let name = String::from_utf8_lossy(path.bytes()).into_owned();
                match std::fs::read(&name) {
                    Ok(bytes) => Ok(Value::Str(Str::heap(bytes))),
                    Err(e) => Err(io_failed("this file could not be read", &name, &e)),
                }
            }
            (Intrinsic::WriteFile, [Value::Str(path), Value::Str(data)]) => {
                let name = String::from_utf8_lossy(path.bytes()).into_owned();
                match std::fs::write(&name, data.bytes()) {
                    Ok(()) => index_fits(data.len())
                        .map_or_else(|| Err(bug("a file longer than `Int` can measure")), Ok),
                    Err(e) => Err(io_failed("this file could not be written", &name, &e)),
                }
            }
            (Intrinsic::FileExists, [Value::Str(path)]) => {
                let name = String::from_utf8_lossy(path.bytes()).into_owned();
                // Openable for reading, which is what the native side tests. "Exists"
                // and "can be opened" differ for a directory or an unreadable file, and
                // the built-in is named for the question a program actually asks.
                Ok(Value::Bool(std::fs::File::open(&name).is_ok()))
            }
            (Intrinsic::Print, [Value::Str(data)]) | (Intrinsic::EPrint, [Value::Str(data)]) => {
                use std::io::Write as _;
                let written = if i == Intrinsic::Print {
                    std::io::stdout()
                        .write_all(data.bytes())
                        .map(|()| data.len())
                } else {
                    std::io::stderr()
                        .write_all(data.bytes())
                        .map(|()| data.len())
                };
                match written {
                    Ok(n) => index_fits(n)
                        .map_or_else(|| Err(bug("more bytes than `Int` can count")), Ok),
                    Err(e) => Err(io_failed("this output could not be written", "-", &e)),
                }
            }
            (Intrinsic::ExitWith, [Value::Int(code)]) => {
                // Ends the process, here as natively. The interpreter runs on a thread
                // it owns, and `exit` from a thread ends the whole process — which is
                // the point: `exit_with` means the program is over, not that this
                // evaluation is.
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
                std::process::exit(i32::try_from(*code).unwrap_or(1))
            }
            // Declassification changes provenance and nothing at run time (N52).
            (Intrinsic::StrVouch, [v @ Value::Str(_)]) => Ok(v.clone()),
            // A contract clause (N87): the sentence the compiled program writes, at the clause.
            (Intrinsic::ContractCheck, [Value::Bool(holds), Value::Int(which)]) => {
                if *holds {
                    return Ok(Value::Int(0));
                }
                let (code, message) = if *which == 0 {
                    (
                        codes::PRECONDITION_FAILED,
                        "this precondition does not hold",
                    )
                } else {
                    (
                        codes::POSTCONDITION_FAILED,
                        "this postcondition does not hold",
                    )
                };
                Err(RuntimeError {
                    diagnostic: Box::new(Diagnostic::error(code, span, message)),
                })
            }
            (Intrinsic::StrFromByte, [Value::Int(b)]) => {
                let Ok(byte) = u8::try_from(*b) else {
                    return Err(out_of_bounds(
                        "this is not a byte value",
                        format!("asked for byte {b}, which is outside 0..=255"),
                    ));
                };
                Ok(Value::Str(Str::heap(vec![byte])))
            }
            _ => Err(bug("arguments of the wrong shape")),
        }
    }

    /// Start one task and hand back the handle its scope will join.
    ///
    /// One OS thread per task, and `docs/spec.md` says so rather than implying anything
    /// else. Each task gets its **own** interpreter over the same program: the state
    /// shared between tasks is exactly the channels they were handed, which is what the
    /// capture rule is for.
    ///
    /// Each task gets its own iteration and depth budgets rather than a share of the
    /// parent's. Splitting them would make a program's own limit depend on how many
    /// tasks happened to be running, which is not something a person can reason about.
    fn start_task(
        &self,
        callee: nazm_sema::FnId,
        args: Vec<Value>,
        span: Span,
    ) -> Result<JoinHandle<Result<(), RuntimeError>>, RuntimeError> {
        let Some(name) = self.core.function(callee).map(|f| f.name.clone()) else {
            return Err(fail(
                codes::UNDEFINED_FUNCTION,
                span,
                "this task's function is not in the program",
            ));
        };
        let core = Arc::clone(&self.core);
        let max_depth = self.max_depth;
        let iterations_left = self.iterations_left;
        let task_name = name.clone();

        std::thread::Builder::new()
            .name(name.clone())
            // The same reservation the main evaluation thread gets, for the same reason:
            // the depth limit has to be reached before the stack is, or a deep recursion
            // inside a task crashes the process with no diagnostic at all.
            .stack_size(stack_for(max_depth))
            .spawn(move || {
                let mut task = Self {
                    core: Arc::clone(&core),
                    pending: Vec::new(),
                    depth: 0,
                    max_depth,
                    iterations_left,
                    calls: 0,
                    // A task has no arguments of its own: `arg(i)` is the program's
                    // command line, and a task is not a program.
                    args: Vec::new(),
                };
                // Looked up in the program this task shares, which is the one the callee
                // was resolved in. Reported rather than indexed, because a panic in a task
                // loses every other task's diagnostic along with its own.
                let Some(f) = core.function(callee) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::INTERPRETER_UNAVAILABLE,
                                Span::synthetic(),
                                format!("a task was started for `{task_name}`, which is not there"),
                            )
                            .help("this is a bug in the interpreter"),
                        ),
                    });
                };
                task.call_body(f, frame(f, args)).map(|_| ())
            })
            .inspect(|_| {
                SCHED_SPAWNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .map_err(|e| RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::INTERPRETER_UNAVAILABLE,
                        span,
                        format!("could not start a task for `{name}`: {e}"),
                    )
                    .help(
                        "each task is an operating-system thread; this is a limit of the \
                         machine, not of the program",
                    ),
                ),
            })
    }

    /// Wait for every task of one scope.
    ///
    /// Every handle is joined even after one has failed. Returning at the first failure
    /// would leave the rest running, and a scope that does not contain its tasks is not
    /// a scope.
    ///
    /// A task that fails ends the whole program, because that is what the specification
    /// promises — tasks do not have private failure modes. The first failure is reported
    /// and the rest are treated as consequences of it.
    fn join_all(tasks: Vec<JoinHandle<Result<(), RuntimeError>>>) -> Result<(), RuntimeError> {
        SCHED_JOINED.fetch_add(
            u64::try_from(tasks.len()).unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
        let mut first: Option<RuntimeError> = None;
        for handle in tasks {
            let outcome = match handle.join() {
                Ok(outcome) => outcome,
                // A panic is an interpreter bug, not a program error, but it still has to
                // come out as a diagnostic: resuming the unwind here would lose the other
                // tasks still waiting to be joined.
                Err(_) => Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::INTERPRETER_UNAVAILABLE,
                            nazm_span::Span::synthetic(),
                            "a task ended abnormally",
                        )
                        .help("this is a bug in the interpreter"),
                    ),
                }),
            };
            if let Err(e) = outcome {
                first.get_or_insert(e);
            }
        }
        match first {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// `local.path = value;`.
    ///
    /// **The new value first, always.** `docs/spec.md` requires a replacement to be secured
    /// before the old one is let go, so that `p.name = p.name` and a failing right-hand side
    /// are both safe. Evaluating here rather than after the walk is what makes that true: by
    /// the time anything is overwritten, `v` already holds its own reference to whatever
    /// backs it. The old value at the end of the path is dropped by the store, which is where
    /// a `Str` backing, a sequence or a channel loses the reference this record held — the
    /// same event, at the same moment, as the compiled program's release.
    ///
    /// Iterative: a path is a list, and walking it with a loop keeps the depth of a
    /// projection out of the host's stack.
    fn assign(
        &mut self,
        f: &cir::Function,
        local: cir::LocalId,
        path: &[cir::Step],
        value: &cir::Expr,
        at: Span,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let v = take!(self, f, value, env);
        let Some(Some(mut slot)) = env.get_mut(local.index()).map(Option::as_mut) else {
            return Err(fail(
                codes::UNDEFINED_VARIABLE,
                at,
                format!(
                    "`{}` is not defined",
                    f.local(local).map_or("a local", |l| l.name.as_str())
                ),
            ));
        };
        for step in path {
            // Unreachable if checking ran: the checker resolved this path and refused
            // anything it could not. A diagnostic rather than a panic, so a checker bug
            // surfaces as a report.
            let Value::Record(r) = slot else {
                return Err(no_such_place(at));
            };
            let Some(next) = r.field_mut(step.field) else {
                return Err(no_such_place(at));
            };
            slot = next;
        }
        *slot = v;
        Ok(Flow::Fell(None))
    }

    fn while_loop(
        &mut self,
        f: &cir::Function,
        id: cir::LoopId,
        cond: &cir::Expr,
        body: &Block,
        at: Span,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        loop {
            // `take!`, because a condition is an expression like any other: `while (if c
            // { return 1; } else { true })` leaves the function from the condition — and a
            // `break` there names the enclosing loop, which is where it travels.
            let c = take!(self, f, cond, env);
            if !as_bool(&c, cond)? {
                return Ok(Flow::Fell(None));
            }

            let Some(left) = self.iterations_left.checked_sub(1) else {
                return Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::ITERATION_LIMIT,
                            at,
                            "this program exceeded the interpreter's iteration limit",
                        )
                        .help(
                            "Nazm does not bound iteration; `nazm run` does, so a \
                         non-terminating program reports instead of hanging. Raise it \
                         with `--max-iterations`, or check the loop's condition ever \
                         becomes false",
                        ),
                    ),
                });
            };
            self.iterations_left = left;

            match self.block(f, body, env)? {
                Flow::Fell(_) => {}
                Flow::Continue(t) if t == id => {}
                Flow::Break(t) if t == id => return Ok(Flow::Fell(None)),
                // A `return` inside a loop leaves the function, not just the loop; a
                // transfer naming an enclosing loop leaves this one on its way there.
                other => return Ok(other),
            }
        }
    }

    /// Evaluate an expression, keeping both its value and any control flow it produces.
    ///
    /// The two cannot be separated, which is why this returns a [`Flow`] rather than a
    /// [`Value`]. `if c { 1 } else { continue; }` in tail position both *is* the block's
    /// value on one path and leaves the loop on the other; `let x = if c { return 1; }
    /// else { 2 };` is the same shape one level in. Every sub-expression here takes its
    /// operands through `take!`, so a flow raised anywhere inside travels straight out
    /// instead of being mistaken for a value that never arrives.
    fn eval(
        &mut self,
        f: &cir::Function,
        e: &cir::Expr,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let value = match &e.kind {
            ExprKind::Const(c) => match f.constant(*c) {
                Some(cir::Const::Int(v)) => Value::Int(*v),
                Some(cir::Const::Bool(v)) => Value::Bool(*v),
                // A literal is **immortal**: the compiled program puts it in a constant and
                // the interpreter must not pretend it allocated one, or the two would
                // disagree about what a program with no allocation did.
                Some(cir::Const::Str(v)) => Value::Str(Str::immortal(v.clone())),
                Some(cir::Const::Num { kind, bits }) => from_num(NumVal {
                    kind: *kind,
                    bits: *bits,
                }),
                None => {
                    return Err(fail(
                        codes::TYPE_MISMATCH,
                        e.at,
                        "this literal is not in its function's constants",
                    ))
                }
            },

            ExprKind::Local(l) => match env.get(l.index()).and_then(Option::as_ref) {
                Some(v) => v.clone(),
                // Unreachable if checking ran. A diagnostic rather than a panic, so a
                // checker bug surfaces as a report.
                None => {
                    return Err(fail(
                        codes::UNDEFINED_VARIABLE,
                        e.at,
                        format!(
                            "`{}` is not defined",
                            f.local(*l).map_or("a local", |l| l.name.as_str())
                        ),
                    ))
                }
            },

            ExprKind::Neg(operand) => match take!(self, f, operand, env) {
                // `-i64::MIN` has no representable result, so negation overflows exactly
                // once — and it is the value people reach for when testing edges.
                Value::Int(v) => v.checked_neg().map(Value::Int).ok_or_else(|| {
                    fail(
                        codes::INTEGER_OVERFLOW,
                        e.at,
                        "this negation overflows `Int`",
                    )
                })?,
                Value::Num(v) => numeric::neg(v)
                    .map(from_num)
                    .map_err(|err| num_failure(err, e.at, "negation", v.kind))?,
                other => {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        format!("`-` applies to `Int`, but this is `{other}`"),
                    ))
                }
            },

            ExprKind::Arith { op, lhs, rhs } => {
                // Left to right, as the spec fixes — and `take!` preserves that: a flow
                // raised by the left operand leaves before the right one is evaluated.
                let l = take!(self, f, lhs, env);
                let r = take!(self, f, rhs, env);
                arith(*op, &l, &r, e.at)?
            }

            ExprKind::Compare { op, lhs, rhs, .. } => {
                let l = take!(self, f, lhs, env);
                let r = take!(self, f, rhs, env);
                compare(*op, &l, &r, e.at)?
            }

            // Gate 2's numeric forms, through the shared semantics.
            ExprKind::Bits { op, lhs, rhs } => {
                let l = take!(self, f, lhs, env);
                let r = take!(self, f, rhs, env);
                let (Some(a), Some(b)) = (as_num(&l), as_num(&r)) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a bit operation on a non-integer",
                    ));
                };
                numeric::bit(op.numeric(), a, b)
                    .map(from_num)
                    .map_err(|err| num_failure(err, e.at, "shift", a.kind))?
            }
            ExprKind::Complement(x) => {
                let v = take!(self, f, x, env);
                let Some(a) = as_num(&v) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a complement of a non-integer",
                    ));
                };
                from_num(numeric::complement(a))
            }
            ExprKind::Convert { mode, operand } => {
                let v = take!(self, f, operand, env);
                let (Some(a), Some(to)) = (as_num(&v), e.ty.and_then(nazm_sema::Type::num)) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a conversion of a non-number",
                    ));
                };
                numeric::convert(to, *mode, a)
                    .map(from_num)
                    .map_err(|err| num_failure(err, e.at, "conversion", to))?
            }
            ExprKind::Fits { to, operand } => {
                let v = take!(self, f, operand, env);
                let (Some(a), Some(to)) = (as_num(&v), to.num()) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a conversion of a non-number",
                    ));
                };
                Value::Bool(numeric::fits(to, a))
            }

            ExprKind::Record {
                arity,
                fields,
                record,
            } => {
                // **In the order written**, which `docs/spec.md` fixes as the evaluation
                // order and which physical layout has nothing to do with. A failure part
                // way through drops what has been built so far — the `Vec` goes with the
                // error — and that drop is what releases a string backing or a sequence
                // reference an earlier initialiser had taken. The counters observe it, so
                // the cleanup is evidence rather than an assumption about the host.
                let values = self.initialise(f, *arity, fields, e.at, env)?;
                match values {
                    Ok(values) => Value::Record(Rec::new(*record, values)),
                    Err(flow) => return Ok(flow),
                }
            }

            ExprKind::Variant {
                enumeration,
                variant,
                arity,
                fields,
            } => {
                // In the order written, like a record construction, and the same drop on
                // the way out. The discriminant is only written once every payload field
                // exists, so no partially built value is ever a value of a variant.
                let values = self.initialise(f, *arity, fields, e.at, env)?;
                match values {
                    Ok(values) => Value::Enum(EnumVal::new(*enumeration, *variant, values)),
                    Err(flow) => return Ok(flow),
                }
            }

            ExprKind::Match(m) => return self.matching(f, m, env),

            ExprKind::Field { base, field, .. } => {
                let base = take!(self, f, base, env);
                // The field's value is **cloned out**, which is the whole of rule 3 for a
                // projection: what comes out holds its own reference to whatever backs
                // it, so it survives the record it came from — including when that record
                // was a temporary that dies on this very line.
                let Value::Record(r) = &base else {
                    return Err(fail(
                        codes::INVALID_PROJECTION,
                        e.at,
                        "this is not a record",
                    ));
                };
                let Some(v) = r.field(*field) else {
                    return Err(fail(
                        codes::UNKNOWN_FIELD,
                        e.at,
                        "this record has no such field",
                    ));
                };
                v.clone()
            }

            ExprKind::Builtin { op, args, .. } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(take!(self, f, a, env));
                }
                self.intrinsic(*op, &values, e.at)?
            }

            ExprKind::Closure {
                func,
                captures: taken,
            } => {
                let mut values = Vec::with_capacity(taken.len());
                for l in taken {
                    match env.get(l.index()).cloned().flatten() {
                        Some(v) => values.push(v),
                        None => {
                            return Err(fail(
                                codes::UNDEFINED_VARIABLE,
                                e.at,
                                "a closure captured a binding that holds nothing",
                            ))
                        }
                    }
                }
                Value::Func(FuncVal::new(*func, values))
            }
            ExprKind::FnValue(func) => Value::Func(FuncVal::new(*func, Vec::new())),

            ExprKind::Call { .. } | ExprKind::CallIndirect { .. } | ExprKind::CallTrait { .. } => {
                let (mut target, args) = match &e.kind {
                    ExprKind::Call { callee, args, .. } => (Ok(*callee), args),
                    // Decided below, from the receiver's value (N78).
                    ExprKind::CallTrait { args, .. } => {
                        (Ok(nazm_sema::FnId::from_index(usize::MAX)), args)
                    }
                    ExprKind::CallIndirect { callee, args, .. } => {
                        let v = take!(self, f, callee, env);
                        match v {
                            Value::Func(fv) => (Err(fv), args),
                            _ => {
                                return Err(fail(
                                    codes::TYPE_MISMATCH,
                                    e.at,
                                    "a call through something that is not a function value",
                                ))
                            }
                        }
                    }
                    _ => unreachable_flow(e.at)?,
                };
                let mut values = Vec::with_capacity(args.len() + 1);
                for a in args {
                    values.push(take!(self, f, a, env));
                }
                // A call through a bound (N78): this frame's type arguments are not kept, but the
                // receiver's value knows its type, and that is the type the bound was
                // instantiated at. Resolved in the one impl table, Core IR's.
                if let ExprKind::CallTrait {
                    trait_id, method, ..
                } = &e.kind
                {
                    let callee = values
                        .first()
                        .and_then(Value::runtime_type)
                        .and_then(|t| self.core.method_fn(*trait_id, t, *method));
                    match callee {
                        Some(c) => target = Ok(c),
                        None => {
                            return Err(fail(
                                codes::COMPILER_DEFECT,
                                e.at,
                                "a trait call whose receiver's type has no impl in the program",
                            ))
                        }
                    }
                }
                let callee = match &target {
                    Ok(c) => *c,
                    Err(fv) => fv.func,
                };
                let core = Arc::clone(&self.core);
                let Some(g) = core.function(callee) else {
                    return Err(fail(
                        codes::UNDEFINED_FUNCTION,
                        e.at,
                        "this call's function is not in the program",
                    ));
                };

                self.depth += 1;
                self.calls += 1;
                if self.depth > self.max_depth {
                    self.depth -= 1;
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::CALL_DEPTH_LIMIT,
                                e.at,
                                format!("`{}` recursed deeper than {}", g.name, self.max_depth),
                            )
                            .help(
                                "the interpreter bounds call depth so runaway recursion \
                             reports instead of exhausting the stack, which would crash \
                             without a diagnostic. Raise it with `--max-depth`, or check \
                             the recursion has a base case",
                            ),
                        ),
                    });
                }

                // A fresh frame: a function sees its parameters and nothing from the
                // caller. Lexical scope, not dynamic. A closure's body sees itself first, then
                // its parameters, and its captures where its body reads them (N50).
                let mut fr = match &target {
                    Err(fv) if g.closure => {
                        values.insert(0, Value::Func(Arc::clone(fv)));
                        frame(g, values)
                    }
                    _ => frame(g, values),
                };
                if let Err(fv) = &target {
                    let slots = &g.captures;
                    for (l, v) in slots.iter().zip(&fv.carried) {
                        if let Some(slot) = fr.get_mut(l.index()) {
                            *slot = Some(v.clone());
                        }
                    }
                }
                let out = self.call_body(g, fr);
                self.depth -= 1;
                // A call absorbs its callee's `return`: that is what makes it a call and
                // not a jump. Nothing propagates outward from here but a genuine error.
                out?
            }

            // Both branches are value regions; the one taken is the `if`'s flow.
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = take!(self, f, cond, env);
                return if as_bool(&c, cond)? {
                    self.block(f, then, env)
                } else {
                    self.block(f, otherwise, env)
                };
            }

            ExprKind::Block(b) => return self.block(f, b, env),
        };
        Ok(Flow::Fell(Some(value)))
    }

    /// Evaluate initialisers in the order written and place each at its field's declaration
    /// index. `Err` carries a flow that left part way, which drops what was built.
    fn initialise(
        &mut self,
        f: &cir::Function,
        arity: u32,
        fields: &[cir::Init],
        at: Span,
        env: &mut Frame,
    ) -> Result<Result<Vec<Value>, Flow>, RuntimeError> {
        let mut built: Vec<Option<Value>> = vec![None; arity as usize];
        for i in fields {
            let v = match self.eval(f, &i.value, env)? {
                Flow::Fell(Some(v)) => v,
                Flow::Fell(None) => return Err(no_value(&i.value)),
                left => return Ok(Err(left)),
            };
            if let Some(slot) = built.get_mut(i.field as usize) {
                *slot = Some(v);
            }
        }
        // Checking guarantees every field was given exactly one value, so an absent one is
        // a checker bug rather than a program's mistake.
        let mut values = Vec::with_capacity(built.len());
        for v in built {
            match v {
                Some(v) => values.push(v),
                None => return Err(fail(codes::MISSING_FIELD, at, "a field was given no value")),
            }
        }
        Ok(Ok(values))
    }
}

/// A condition's value, once something has established it produced one.
///
/// Not a `Bool` here means checking passed something it should not have, so this
/// reports rather than panics: a checker bug should be readable.
fn as_bool(v: &Value, cond: &cir::Expr) -> Result<bool, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(fail(
            codes::CONDITION_NOT_BOOL,
            cond.at,
            format!("a condition must be `Bool`, but this is `{other}`"),
        )),
    }
}

fn no_such_place(at: Span) -> RuntimeError {
    fail(
        codes::UNDEFINED_VARIABLE,
        at,
        "this is not a place that can be written to",
    )
}

/// Checked `Int` arithmetic, failing exactly where `docs/spec.md` says it does.
fn arith(op: cir::ArithOp, l: &Value, r: &Value, at: Span) -> Result<Value, RuntimeError> {
    use cir::ArithOp::{Add, Div, Mul, Rem, Sub};
    use Value::Int;

    let overflow = |what: &str| RuntimeError {
        diagnostic: Box::new(
            Diagnostic::error(
                codes::INTEGER_OVERFLOW,
                at,
                format!("this {what} overflows `Int`"),
            )
            .help(
                "`Int` is a 64-bit signed integer; overflow stops the program rather than wrapping",
            ),
        ),
    };
    match (op, l, r) {
        (Add, Int(a), Int(b)) => a
            .checked_add(*b)
            .map(Int)
            .ok_or_else(|| overflow("addition")),
        (Sub, Int(a), Int(b)) => a
            .checked_sub(*b)
            .map(Int)
            .ok_or_else(|| overflow("subtraction")),
        (Mul, Int(a), Int(b)) => a
            .checked_mul(*b)
            .map(Int)
            .ok_or_else(|| overflow("multiplication")),
        (Div | Rem, Int(_), Int(0)) => Err(RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::DIVISION_BY_ZERO, at, "division by zero")
                    .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`"),
            ),
        }),
        // `checked_div` also catches i64::MIN / -1, which overflows rather than dividing —
        // the one division that is not a zero check.
        (Div, Int(a), Int(b)) => a
            .checked_div(*b)
            .map(Int)
            .ok_or_else(|| overflow("division")),
        // `i64::MIN % -1` is mathematically 0 and 0 is representable, so nothing overflows.
        // `checked_rem` says `None` anyway, because the hardware instruction that computes
        // it faults on the *quotient*. Inheriting that would make the rule "`%` fails on a
        // zero divisor, and also on one exact pair of operands" — a wart an agent would have
        // to memorise. `wrapping_rem` is the correct answer here, and the zero divisor is
        // already handled above.
        (Rem, Int(a), Int(b)) => Ok(Int(a.wrapping_rem(*b))),
        (op, Value::Num(a), Value::Num(b)) => {
            let nop = match op {
                Add => numeric::Arith::Add,
                Sub => numeric::Arith::Sub,
                Mul => numeric::Arith::Mul,
                Div => numeric::Arith::Div,
                Rem => numeric::Arith::Rem,
            };
            let what = match op {
                Add => "addition",
                Sub => "subtraction",
                Mul => "multiplication",
                Div | Rem => "division",
            };
            numeric::arith(nop, *a, *b)
                .map(from_num)
                .map_err(|err| num_failure(err, at, what, a.kind))
        }
        // Unreachable after checking. A diagnostic rather than a panic.
        (op, a, b) => Err(fail(
            codes::INVALID_OPERANDS,
            at,
            format!(
                "`{}` does not apply to `{}` and `{}`",
                op.symbol(),
                type_name(a),
                type_name(b)
            ),
        )),
    }
}

/// A value as a number of the shared semantics, `Int` included (Gate 2).
fn as_num(v: &Value) -> Option<NumVal> {
    match v {
        Value::Int(i) => NumVal::int(NumKind::I64, i128::from(*i)),
        Value::Num(n) => Some(*n),
        _ => None,
    }
}

/// A number as a value: an `Int` for that kind, a [`Value::Num`] for every other.
fn from_num(n: NumVal) -> Value {
    if n.kind == NumKind::I64 {
        #[allow(clippy::cast_possible_wrap)]
        Value::Int(n.bits as i64)
    } else {
        Value::Num(n)
    }
}

/// The failure a numeric operation stops the program with — the same codes and sentences the
/// compiled program reports.
fn num_failure(err: NumError, at: Span, what: &str, kind: NumKind) -> RuntimeError {
    let d = match err {
        NumError::Overflow => Diagnostic::error(
            codes::INTEGER_OVERFLOW,
            at,
            format!("this {what} overflows `{}`", kind.name()),
        )
        .help("overflow stops the program rather than wrapping; `wrapping_add` and its kin wrap"),
        NumError::DivideByZero => {
            Diagnostic::error(codes::DIVISION_BY_ZERO, at, "division by zero")
                .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`")
        }
        NumError::ShiftRange => Diagnostic::error(
            codes::SHIFT_OUT_OF_RANGE,
            at,
            format!("this shift amount is outside `0..{}`", kind.bits()),
        )
        .help("a shift by a negative amount, or by the width or more, stops the program"),
    };
    RuntimeError {
        diagnostic: Box::new(d),
    }
}

/// A comparison. Ordering is `Int`'s; equality is [`equal`], the language's law, and `!=` is
/// its negation rather than a second walk that could drift from it.
fn compare(op: cir::CmpOp, l: &Value, r: &Value, at: Span) -> Result<Value, RuntimeError> {
    use cir::CmpOp::{Eq, Gt, GtEq, Lt, LtEq, NotEq};
    use Value::{Bool, Int};

    let same = if matches!(op, Eq | NotEq) {
        equal(l, r)
    } else {
        None
    };
    let ordering = |o: numeric::Cmp, a: &NumVal, b: &NumVal| Ok(Bool(numeric::compare(o, *a, *b)));
    match (op, l, r) {
        (Lt, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::Lt, a, b),
        (LtEq, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::LtEq, a, b),
        (Gt, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::Gt, a, b),
        (GtEq, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::GtEq, a, b),
        (Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
        (LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
        (Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
        (GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),
        (Eq, _, _) if same.is_some() => Ok(Bool(same == Some(true))),
        (NotEq, _, _) if same.is_some() => Ok(Bool(same == Some(false))),
        // Unreachable after checking. A diagnostic rather than a panic.
        (op, a, b) => Err(fail(
            codes::INVALID_OPERANDS,
            at,
            format!(
                "`{}` does not apply to `{}` and `{}`",
                op.symbol(),
                type_name(a),
                type_name(b)
            ),
        )),
    }
}

/// Built-in equality, by the language's law rather than the host's. N13.
///
/// `Int`, `Bool` and `Str` compare as they always have — a `Str` by its bytes, never by
/// whether two views share a backing. A record compares every field, and an enum its
/// active variant and then **that variant's payload alone**: a value here carries no
/// storage for an inactive variant, so there is nothing else it could read. No definition
/// id is compared, because checking has already established that both sides have one type.
///
/// `None` for a value the language gives no equality — a sequence, a `Vec`, a `Chan` —
/// which checking refuses (`N0304`) before a program runs. [`Value`]'s own `PartialEq`
/// answers those by identity so the rest of `Value` can be compared, and that answer is
/// the host's convenience, not the language's: it is not what `==` means, and this is.
///
/// Recursive, and bounded: a type nests at most [`nazm_sema::MAX_TYPE_DEPTH`] deep.
fn equal(a: &Value, b: &Value) -> Option<bool> {
    let all = |xs: &[Value], ys: &[Value]| -> Option<bool> {
        if xs.len() != ys.len() {
            return None;
        }
        for (x, y) in xs.iter().zip(ys) {
            if !equal(x, y)? {
                return Some(false);
            }
        }
        Some(true)
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x == y),
        // IEEE 754's for a float (Gate 2): a NaN equals nothing.
        (Value::Num(x), Value::Num(y)) => Some(numeric::compare(numeric::Cmp::Eq, *x, *y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x == y),
        (Value::Str(x), Value::Str(y)) => Some(x.bytes() == y.bytes()),
        (Value::Record(x), Value::Record(y)) => all(&x.fields, &y.fields),
        (Value::Enum(x), Value::Enum(y)) => {
            if x.variant != y.variant {
                return Some(false);
            }
            all(&x.fields, &y.fields)
        }
        _ => None,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Num(n) => n.kind.name(),
        Value::Bool(_) => "Bool",
        Value::Str(_) => "Str",
        Value::Ints(_) => "Ints",
        Value::Strs(_) => "Strs",
        Value::Chan(_) => "Chan",
        // Only reached by a built-in handed the wrong type, which checking has already
        // refused — and no built-in takes a record, so no message can name one usefully.
        Value::Record(_) => "a record",
        Value::Enum(_) => "an enum",
        Value::Vec(_) => "a `Vec`",
        Value::Cap(k) => k.type_name(),
        Value::Func(_) => "a function value",
    }
}

/// Gate 2's numeric built-ins, through the shared semantics; `None` for every other built-in.
fn numeric_intrinsic(
    i: Intrinsic,
    args: &[Value],
    at: Span,
) -> Option<Result<Value, RuntimeError>> {
    use numeric::{Arith, Binary, Count, Unary};
    let a = args.first().and_then(as_num);
    let b = args.get(1).and_then(as_num);
    let unary = |f: Unary| a.map(|a| Ok(from_num(numeric::unary(f, a))));
    let binary = |f: Binary| Some(Ok(from_num(numeric::binary(f, a?, b?))));
    let predicate = |p: fn(f64) -> bool| a.map(|a| Ok(Value::Bool(p(a.as_f64()))));
    let count = |c: Count| a.map(|a| Ok(Value::Int(numeric::count(c, a))));
    let str_arg = || match args.first() {
        Some(Value::Str(s)) => Some(String::from_utf8_lossy(s.bytes()).into_owned()),
        _ => None,
    };
    let bits_of = |k: NumKind, bits: u64| Some(Ok(from_num(NumVal { kind: k, bits })));
    Some(match i {
        Intrinsic::WrappingAdd => Ok(from_num(numeric::wrapping(Arith::Add, a?, b?))),
        Intrinsic::WrappingSub => Ok(from_num(numeric::wrapping(Arith::Sub, a?, b?))),
        Intrinsic::WrappingMul => Ok(from_num(numeric::wrapping(Arith::Mul, a?, b?))),
        Intrinsic::SaturatingAdd => Ok(from_num(numeric::saturating(Arith::Add, a?, b?))),
        Intrinsic::SaturatingSub => Ok(from_num(numeric::saturating(Arith::Sub, a?, b?))),
        Intrinsic::SaturatingMul => Ok(from_num(numeric::saturating(Arith::Mul, a?, b?))),
        Intrinsic::CountOnes => return count(Count::Ones),
        Intrinsic::LeadingZeros => return count(Count::LeadingZeros),
        Intrinsic::TrailingZeros => return count(Count::TrailingZeros),
        Intrinsic::RotateLeft | Intrinsic::RotateRight => {
            let a = a?;
            let n = match args.get(1) {
                Some(Value::Int(n)) => *n,
                _ => return None,
            };
            numeric::rotate(i == Intrinsic::RotateLeft, a, n)
                .map(from_num)
                .map_err(|err| num_failure(err, at, "rotation", a.kind))
        }
        Intrinsic::SwapBytes => Ok(from_num(numeric::swap_bytes(a?))),
        Intrinsic::ToBigEndian | Intrinsic::FromBigEndian => {
            Ok(from_num(numeric::endian(true, a?)))
        }
        Intrinsic::ToLittleEndian | Intrinsic::FromLittleEndian => {
            Ok(from_num(numeric::endian(false, a?)))
        }
        Intrinsic::FloatSqrt => return unary(Unary::Sqrt),
        Intrinsic::FloatAbs => return unary(Unary::Abs),
        Intrinsic::FloatFloor => return unary(Unary::Floor),
        Intrinsic::FloatCeil => return unary(Unary::Ceil),
        Intrinsic::FloatTrunc => return unary(Unary::Trunc),
        Intrinsic::FloatRound => return unary(Unary::Round),
        Intrinsic::FloatSin => return unary(Unary::Sin),
        Intrinsic::FloatCos => return unary(Unary::Cos),
        Intrinsic::FloatTan => return unary(Unary::Tan),
        Intrinsic::FloatExp => return unary(Unary::Exp),
        Intrinsic::FloatLn => return unary(Unary::Ln),
        Intrinsic::FloatLog2 => return unary(Unary::Log2),
        Intrinsic::FloatLog10 => return unary(Unary::Log10),
        Intrinsic::FloatPow => return binary(Binary::Pow),
        Intrinsic::FloatAtan2 => return binary(Binary::Atan2),
        Intrinsic::FloatMin => return binary(Binary::Min),
        Intrinsic::FloatMax => return binary(Binary::Max),
        Intrinsic::FloatIsNan => return predicate(f64::is_nan),
        Intrinsic::FloatIsInfinite => return predicate(f64::is_infinite),
        Intrinsic::FloatIsFinite => return predicate(f64::is_finite),
        Intrinsic::NumToStr => Ok(Value::Str(Str::heap(numeric::to_str(a?).into_bytes()))),
        Intrinsic::Float64ToBits => return bits_of(NumKind::U64, a?.bits),
        Intrinsic::Float64FromBits => return bits_of(NumKind::F64, a?.bits),
        Intrinsic::Float32ToBits => return bits_of(NumKind::U32, a?.bits),
        Intrinsic::Float32FromBits => return bits_of(NumKind::F32, a?.bits),
        Intrinsic::FloatParses => Ok(Value::Bool(numeric::parses(&str_arg()?))),
        Intrinsic::FloatParseRaw => Ok(from_num(NumVal::f64(numeric::parse(&str_arg()?)))),
        _ => return None,
    })
}
