//! Tree-walking evaluator.
//!
//! Runs only after checking has passed, so the shapes it handles are the ones the checker
//! guaranteed. What remains at runtime is the two failures a type system cannot rule out:
//! integer overflow and division by zero. Both stop the program with a diagnostic that
//! points at the operator, and both behave identically in every build — a bug that
//! reproduces only in one build configuration is the worst kind to be handed.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use nazm_syntax::{Assign, BinOp, Block, Expr, Function, MatchArm, Place, Program, Stmt, While};

use nazm_sema::enums::EnumId;
use nazm_sema::intrinsic::Intrinsic;
use nazm_sema::module::ModuleId;
use nazm_sema::record::RecordId;
use nazm_sema::resolve::{Callee, FnId, Resolution};

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
    queue: std::collections::VecDeque<i64>,
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
    fn send(&self, value: i64) -> Option<bool> {
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
        Some(true)
    }

    /// Take a value, or report that the channel is closed **and drained**. Values queued
    /// before a close are still delivered: closing ends a channel, it does not discard
    /// what is in it.
    fn recv(&self) -> Option<Option<i64>> {
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
        Some(())
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
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Ints(a), Self::Ints(b)) => Arc::ptr_eq(a, b),
            (Self::Strs(a), Self::Strs(b)) => Arc::ptr_eq(a, b),
            (Self::Chan(a), Self::Chan(b)) => Arc::ptr_eq(a, b),
            (Self::Vec(a), Self::Vec(b)) => Arc::ptr_eq(a, b),
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
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
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
            Self::Vec(v) => match v.lock() {
                Ok(v) => write!(f, "Vec({} items)", v.len()),
                Err(_) => f.write_str("Vec(unreadable)"),
            },
            // A record is never a program's result — `main` returns `Int` — so this is
            // for diagnostics, and says what it is rather than dumping it.
            Self::Record(r) => write!(f, "record({} fields)", r.fields.len()),
            Self::Enum(e) => write!(f, "variant {}({} fields)", e.variant, e.fields.len()),
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

/// One lexical scope's bindings: a short association list, not a map.
///
/// # Why not a `HashMap`
///
/// It was one. `cargo xtask bench` on `bench/programs/arith.nz` — a loop doing nothing
/// but arithmetic on five locals — spent **29% of the interpreter's time** hashing
/// variable names and comparing them: 43 samples in `RandomState::hash_one::<String>`,
/// 23 in SipHash's `write`, and 15 in `memcmp`, out of 277 on the evaluation thread.
///
/// A frame holds a handful of bindings, and the checker refuses two of one name in the
/// same scope, so a linear scan compares a few short strings and hashes nothing. The
/// measurement is recorded in `docs/performance.md`; the point of repeating it here is
/// that the obvious refactor back to a map would undo a measured result, and whoever
/// proposes it should have to argue with a number rather than with a preference.
#[derive(Debug, Default)]
struct Frame {
    slots: Vec<(String, Value)>,
}

impl Frame {
    fn get(&self, name: &str) -> Option<&Value> {
        self.slots.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.slots
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// Bind a name, replacing any existing binding of it.
    ///
    /// Replacing rather than appending keeps this exactly what the `HashMap` did. Two
    /// `let`s of one name in one scope is refused by the checker, so the case is
    /// unreachable — and a second entry shadowing the first would be a silent
    /// difference in behaviour if it ever became reachable, which is the kind found
    /// years later.
    fn insert(&mut self, name: String, value: Value) {
        match self.get_mut(&name) {
            Some(slot) => *slot = value,
            None => self.slots.push((name, value)),
        }
    }
}

type Env = Vec<Frame>;

/// How a statement finished.
///
/// A statement normally falls through to the next one; `break` and `continue` do not, and
/// the loop that has to act on them may be several blocks up. Returning the signal rather
/// than unwinding keeps the control flow visible in the types — every caller has to say
/// what it does with a `Break` it did not expect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow {
    /// Fell off the end. Carries the tail's value when the block had one.
    Fell(Option<Value>),
    Break,
    Continue,
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
    ($slf:ident, $e:expr, $env:expr) => {
        match $slf.eval($e, $env)? {
            Flow::Fell(Some(v)) => v,
            Flow::Fell(None) => return Err($slf.no_value($e)),
            left => return Ok(left),
        }
    };
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
/// Chiefly the passes over the tree — parsing, checking, evaluating and dropping it —
/// which recurse with its shape rather than with call depth. That requirement belongs to
/// the parser, which sets the nesting limit, so the number is taken from there rather
/// than restated here where the two could drift apart.
pub const STACK_BASE_BYTES: usize = nazm_syntax::PARSE_STACK_BYTES;

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
    program: Arc<Program>,
    /// What resolution decided. Shared with tasks for the same reason as the tree.
    ///
    /// The evaluator used to carry a `HashMap<String, usize>` of its own, built over
    /// every function in the program, and to apply the built-in-first rule itself at each
    /// call. That was a third implementation of name resolution after the checker's and
    /// the backend's, and once a name could mean different things in different modules it
    /// stopped being merely duplicated and started being **wrong**: one map cannot hold
    /// two modules' private `helper`. Calls are now looked up by the span of the callee,
    /// which the checker already resolved.
    res: Arc<Resolution>,
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
    /// The program's own arguments, **excluding** the program name — `nazm run f.nz --
    /// a b` gives `["a", "b"]`. Excluding it is what makes `arg(i)` mean the same thing
    /// interpreted and compiled: a native `argv[0]` is a path nobody should depend on.
    args: Vec<String>,
}

impl Interpreter {
    #[must_use]
    ///
    /// The tree is cloned once, here, so that tasks can share it. One copy per run of a
    /// program is not a cost worth designing around; one copy per task would be.
    pub fn new(program: &Program, res: &Resolution) -> Self {
        Self {
            program: Arc::new(program.clone()),
            res: Arc::new(res.clone()),
            pending: Vec::new(),
            depth: 0,
            max_depth: DEFAULT_MAX_DEPTH,
            iterations_left: DEFAULT_MAX_ITERATIONS,
            args: Vec::new(),
        }
    }

    /// Which declaration the call whose callee was written at `at` refers to.
    ///
    /// An index into `Program::functions`, because the syntax tree is still the only
    /// place a body lives. Two hops — span to definition, definition to declaration —
    /// and neither of them is a name.
    fn decl_at(&self, at: nazm_span::Span) -> Option<usize> {
        match self.res.callee(at)? {
            Callee::Function(id) => Some(self.res.function(id)?.decl as usize),
            Callee::Builtin(_) => None,
        }
    }

    /// The declaration of a definition, by id.
    fn declaration<'p>(&self, program: &'p Program, id: FnId) -> Option<&'p Function> {
        program.functions.get(self.res.function(id)?.decl as usize)
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

    /// Evaluate `main` and return its value.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        // Cloned before the lookup so that the body being run borrows the clone and not
        // `self`, which `call_body` needs mutably.
        let program = Arc::clone(&self.program);
        // The **root** module's `main`, not whichever `main` the program happens to
        // contain. An imported module may have one of its own and it is not an entry
        // point; `docs/spec.md` says so and this is where that is true at runtime.
        let main = self
            .res
            .definition(ModuleId::ROOT, "main")
            .and_then(|id| self.declaration(&program, id))
            .ok_or_else(|| RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::MISSING_MAIN,
                    nazm_span::Span::synthetic(),
                    "this program has no `main` function",
                )),
            })?;
        let mut env: Env = vec![Frame::default()];
        self.call_body(Self::body_of(main)?, &mut env, "main")
    }

    /// Run a function body and produce its value.
    ///
    /// `return` and falling off the end with a tail are the two ways a function produces
    /// one; checking guarantees every path does one or the other, so the remaining cases
    /// are diagnostics rather than panics.
    fn call_body(
        &mut self,
        body: &Block,
        env: &mut Env,
        name: &str,
    ) -> Result<Value, RuntimeError> {
        match self.block_value(body, env)? {
            Flow::Return(v) | Flow::Fell(Some(v)) => Ok(v),
            Flow::Fell(None) => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    body.span,
                    format!("`{name}` finished without producing a value"),
                )),
            }),
            Flow::Break | Flow::Continue => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::NOT_IN_A_LOOP,
                    body.span,
                    format!("loop control escaped from `{name}`"),
                )),
            }),
        }
    }

    /// Run a block in its own scope.
    /// `match scrutinee { … }`.
    ///
    /// The scrutinee is evaluated **once**, into a value this expression owns, and the arm
    /// runs with the payload bound into a scope of its own. `docs/spec.md` specifies value
    /// evaluation rather than a borrow of the caller's storage, so `match x { … }` leaves
    /// `x` untouched and `match make() { … }` has a clear owner for the temporary.
    ///
    /// The bindings are **clones of the payload values**, which is the same operation a
    /// projection out of a record performs and the same rule: what comes out holds its own
    /// reference to whatever backs it, so an arm may return it and it survives the
    /// scrutinee. The scrutinee is dropped when this function returns, after the arm's
    /// value has been established — which is the ordering the whole milestone turns on.
    /// `operand?`: the operand once, then either its success value or a `return`.
    ///
    /// No host mechanism is involved — not a panic, not an unwind. The `Err` path is the
    /// same [`Flow::Return`] a written `return` produces, so it travels out through loops,
    /// scopes and blocks exactly as one does, and a scope it leaves joins its tasks on the
    /// way. What it returns is the enclosing function's own `Result`, which may be a
    /// different instance from the operand's: only the error value is carried across.
    fn propagate(&mut self, operand: &Expr, at: Span, env: &mut Env) -> Result<Flow, RuntimeError> {
        let value = take!(self, operand, env);
        let (Value::Enum(subject), Some(p)) = (value, self.res.propagation(at)) else {
            // Unreachable for a checked program, which records every `?` it accepts.
            return Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::PROPAGATE_NON_RESULT,
                    at,
                    "this `?` was not resolved to a `Result`",
                )),
            });
        };
        let payload = |v: &EnumVal| {
            v.field(0).cloned().ok_or_else(|| RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    at,
                    "this `Result` variant has no payload",
                )),
            })
        };
        if subject.variant() == p.ok {
            return Ok(Flow::Fell(Some(payload(&subject)?)));
        }
        let error = payload(&subject)?;
        Ok(Flow::Return(Value::Enum(EnumVal::new(
            p.into,
            p.into_err,
            vec![error],
        ))))
    }

    fn match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
        env: &mut Env,
    ) -> Result<Flow, RuntimeError> {
        let value = take!(self, scrutinee, env);
        let Value::Enum(subject) = value else {
            return Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::MATCH_ON_NON_ENUM,
                    scrutinee.span(),
                    "this is not an enum",
                )),
            });
        };
        // The discriminant selects the arm. Checking has established that exactly one arm
        // names each variant, so the first match is the only match and there is no
        // fallthrough to express.
        //
        // Compared by **definition**, since N11: the interpreter is parametric, so an arm in
        // a generic body names `Maybe[T]` while the value it meets is a `Maybe[Int]`, and
        // both are the one enum `Maybe` whose variants every application shares.
        let types = self.res.types();
        let subject_def = types.definition_of(nazm_sema::Type::Enum(subject.def));
        let Some(arm) = arms.iter().find(|a| {
            self.res.variant(a.variant_span).is_some_and(|v| {
                types.definition_of(nazm_sema::Type::Enum(v.enumeration)) == subject_def
                    && v.variant == subject.variant
            })
        }) else {
            // Unreachable for a checked program: `N0343` refuses a `match` that leaves a
            // variant uncovered. Reported rather than assumed away, and deliberately not
            // as a language-visible trap — it says the compiler is wrong, not the program.
            return Err(RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::NON_EXHAUSTIVE_MATCH,
                        span,
                        "no arm covers this value's variant",
                    )
                    .help("this is a bug in the compiler: exhaustiveness is checked"),
                ),
            });
        };
        let mut frame = Frame::default();
        for f in &arm.fields {
            let Some(name) = &f.binding else { continue };
            let Some(at) = self.res.payload(f.name_span) else {
                continue;
            };
            let Some(v) = subject.field(at.field) else {
                return Err(RuntimeError {
                    diagnostic: Box::new(Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        "this variant has no such payload field",
                    )),
                });
            };
            frame.insert(name.clone(), v.clone());
        }
        env.push(frame);
        let out = self.eval(&arm.body, env);
        env.pop();
        // `subject` is dropped here, after the arm's value has been established — which is
        // what keeps a returned payload valid. A `_` field is in `subject` like any other
        // and goes with it; declining to name a value is not declining to own it.
        out
    }

    fn block(&mut self, b: &Block, env: &mut Env) -> Result<Flow, RuntimeError> {
        env.push(Frame::default());
        let out = self.block_inner(b, env);
        env.pop();
        out
    }

    fn block_inner(&mut self, b: &Block, env: &mut Env) -> Result<Flow, RuntimeError> {
        for stmt in &b.statements {
            let flow = self.stmt(stmt, env)?;
            if !flow.is_fall_through() {
                // Leaves this block and travels outward to whatever handles it.
                return Ok(flow);
            }
        }
        match &b.tail {
            Some(tail) => self.eval(tail, env),
            None => Ok(Flow::Fell(None)),
        }
    }

    /// Run a block where a value is required.
    ///
    /// Used for `if` branches in expression position and for a function body. A block
    /// that diverges has no value and the caller decides what that means.
    fn block_value(&mut self, b: &Block, env: &mut Env) -> Result<Flow, RuntimeError> {
        self.block(b, env)
    }

    fn stmt(&mut self, stmt: &Stmt, env: &mut Env) -> Result<Flow, RuntimeError> {
        match stmt {
            Stmt::Let(l) => {
                let v = take!(self, &l.value, env);
                if let Some(frame) = env.last_mut() {
                    frame.insert(l.name.clone(), v);
                }
                Ok(Flow::Fell(None))
            }
            Stmt::Assign(a) => self.assign(a, env),
            // A loop consumes its own `break` and `continue`, but a `return` inside one
            // leaves the function and must travel through.
            Stmt::While(w) => self.while_loop(w, env),
            // Through `take!`: `return (if c { return 1; } else { 2 });` is legal, and
            // the inner `return` is the one that wins.
            Stmt::Return(r) => Ok(Flow::Return(take!(self, &r.value, env))),
            Stmt::Break { .. } => Ok(Flow::Break),
            Stmt::Continue { .. } => Ok(Flow::Continue),
            Stmt::Scope(sc) => {
                // Tasks run while the body does. The closing brace joins every one of
                // them, and joins them *before* propagating a failure of the body —
                // returning early would leave tasks running past the scope that promised
                // to contain them, which is the one guarantee this construct exists for.
                self.pending.push(Vec::new());
                let flow = self.block(&sc.body, env);
                let joined = Self::join_all(self.pending.pop().unwrap_or_default());
                // The body's failure wins: a task that failed because the body stopped
                // feeding it is a consequence, not the cause.
                let flow = flow?;
                joined?;
                Ok(match flow {
                    // A `scope` produces nothing; a `break` or `return` inside one still
                    // travels outward, after the join.
                    Flow::Fell(_) => Flow::Fell(None),
                    other => other,
                })
            }
            Stmt::Spawn(sp) => {
                let mut values = Vec::with_capacity(sp.args.len());
                for a in &sp.args {
                    values.push(take!(self, a, env));
                }
                // Checked before starting anything: a task with no scope to join it is
                // the one case where starting it would be unrecoverable. Unreachable —
                // the checker reports `N0320`.
                if self.pending.is_empty() {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::SPAWN_OUTSIDE_SCOPE,
                            sp.span,
                            "`spawn` reached the interpreter with no scope to join it",
                        )),
                    });
                }
                let task = self.start_task(&sp.callee, sp.callee_span, values, sp.span)?;
                if let Some(scope) = self.pending.last_mut() {
                    scope.push(task);
                }
                Ok(Flow::Fell(None))
            }
            Stmt::Expr(e) => {
                // The value is dropped; the flow is not. An `if` in statement position
                // may still `return` or `break`, and that has to travel outward.
                Ok(match self.eval(e, env)? {
                    Flow::Fell(_) => Flow::Fell(None),
                    other => other,
                })
            }
        }
    }

    /// A condition's value, once something has established it produced one.
    ///
    /// Not a `Bool` here means checking passed something it should not have, so this
    /// reports rather than panics: a checker bug should be readable.
    fn as_bool(&self, v: &Value, cond: &Expr) -> Result<bool, RuntimeError> {
        match v {
            Value::Bool(b) => Ok(*b),
            other => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::CONDITION_NOT_BOOL,
                    cond.span(),
                    format!("a condition must be `Bool`, but this is `{other}`"),
                )),
            }),
        }
    }

    /// An expression that was asked for a value and produced none.
    ///
    /// Checking has already established that one arrives, so this is a checker bug rather
    /// than a program's mistake — reported, not panicked on.
    fn no_value(&self, e: &Expr) -> RuntimeError {
        RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    e.span(),
                    "this produces no value, but one is needed here",
                )
                .help("a block's value is its final expression — one with no `;` after it"),
            ),
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
            (Intrinsic::ChanNew, [Value::Int(capacity)]) => {
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
            (Intrinsic::ChanSend, [Value::Chan(c), Value::Int(value)]) => {
                c.send(*value).map_or_else(
                    || Err(bug("a channel that could not be locked")),
                    |sent| Ok(Value::Bool(sent)),
                )
            }
            (Intrinsic::ChanRecv, [Value::Chan(c), Value::Ints(out)]) => {
                let Some(taken) = c.recv() else {
                    return Err(bug("a channel that could not be locked"));
                };
                match taken {
                    Some(v) => {
                        let Some(mut out) = seq(out) else {
                            return Err(bug("a sequence that could not be written"));
                        };
                        out.push(v);
                        Ok(Value::Bool(true))
                    }
                    None => Ok(Value::Bool(false)),
                }
            }
            (Intrinsic::ChanClose, [Value::Chan(c)]) => c.close().map_or_else(
                || Err(bug("a channel that could not be locked")),
                |()| Ok(Value::Int(0)),
            ),
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
        callee: &str,
        at: nazm_span::Span,
        args: Vec<Value>,
        span: nazm_span::Span,
    ) -> Result<JoinHandle<Result<(), RuntimeError>>, RuntimeError> {
        let index = self.decl_at(at).ok_or_else(|| RuntimeError {
            diagnostic: Box::new(Diagnostic::error(
                codes::UNDEFINED_FUNCTION,
                span,
                format!("`{callee}` is not a function"),
            )),
        })?;
        let program = Arc::clone(&self.program);
        let res = Arc::clone(&self.res);
        let max_depth = self.max_depth;
        let iterations_left = self.iterations_left;
        let name = callee.to_string();

        std::thread::Builder::new()
            .name(name.clone())
            // The same reservation the main evaluation thread gets, for the same reason:
            // the depth limit has to be reached before the stack is, or a deep recursion
            // inside a task crashes the process with no diagnostic at all.
            .stack_size(stack_for(max_depth))
            .spawn(move || {
                let mut task = Self {
                    program: Arc::clone(&program),
                    res,
                    pending: Vec::new(),
                    depth: 0,
                    max_depth,
                    iterations_left,
                    // A task has no arguments of its own: `arg(i)` is the program's
                    // command line, and a task is not a program.
                    args: Vec::new(),
                };
                // The index was taken from the table built over this program, so it is
                // always in range. Reported rather than indexed, because a panic in a
                // task loses every other task's diagnostic along with its own.
                let Some(f) = program.functions.get(index) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::INTERPRETER_UNAVAILABLE,
                                nazm_span::Span::synthetic(),
                                format!("a task was started for `{name}`, which is not there"),
                            )
                            .help("this is a bug in the interpreter"),
                        ),
                    });
                };
                let mut env: Env = vec![Frame::default()];
                if let Some(frame) = env.last_mut() {
                    for (p, v) in f.params.iter().zip(args) {
                        frame.insert(p.name.clone(), v);
                    }
                }
                Self::body_of(f).and_then(|body| task.call_body(body, &mut env, &name).map(|_| ()))
            })
            .map_err(|e| RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::INTERPRETER_UNAVAILABLE,
                        span,
                        format!("could not start a task for `{callee}`: {e}"),
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

    /// Assign to the innermost binding of that name.
    ///
    /// Innermost, so a loop body assigning to a variable declared outside it updates that
    /// one rather than creating a shadow that vanishes each iteration — which would make
    /// every accumulator silently do nothing.
    fn assign(&mut self, a: &Assign, env: &mut Env) -> Result<Flow, RuntimeError> {
        // **The new value first, always.** `docs/spec.md` requires a replacement to be
        // secured before the old one is let go, so that `p.name = p.name` and a failing
        // right-hand side are both safe. Evaluating here rather than after the walk is
        // what makes that true: by the time anything is overwritten, `v` already holds
        // its own reference to whatever backs it.
        let v = take!(self, &a.value, env);
        let place = &a.place;
        // Cloned handle rather than a borrow: `env` is borrowed mutably below, and the
        // resolution is behind the same `self`.
        let res = Arc::clone(&self.res);
        for frame in env.iter_mut().rev() {
            if let Some(slot) = frame.get_mut(&place.name) {
                return Self::replace(&res, slot, place, &v).map(|()| Flow::Fell(None));
            }
        }
        Err(RuntimeError {
            diagnostic: Box::new(Diagnostic::error(
                codes::UNDEFINED_VARIABLE,
                place.name_span,
                format!("`{}` is not defined", place.name),
            )),
        })
    }

    /// Write `v` at the end of `place`'s projection path, starting from the binding.
    ///
    /// The old value at that position is dropped by the store, which is where a `Str`
    /// backing, a sequence or a channel loses the reference this record held — the same
    /// event, at the same moment, as the compiled program's release.
    ///
    /// Iterative: a path is a list, and walking it with a loop keeps the depth of a
    /// projection out of the host's stack.
    ///
    /// Which field each step names is **read from the resolution**, not looked up by
    /// name here. The checker already decided it, and deciding it again would be the
    /// second resolver N1 removed.
    fn replace(
        res: &Resolution,
        mut slot: &mut Value,
        place: &Place,
        v: &Value,
    ) -> Result<(), RuntimeError> {
        for step in &place.path {
            // Unreachable if checking ran: the checker resolved this path and refused
            // anything it could not. A diagnostic rather than a panic, so a checker bug
            // surfaces as a report.
            let (Value::Record(r), Some(at)) = (&mut *slot, res.field(step.name_span)) else {
                return Err(Self::no_such_place(place));
            };
            let Some(next) = r.field_mut(at.field) else {
                return Err(Self::no_such_place(place));
            };
            slot = next;
        }
        *slot = v.clone();
        Ok(())
    }

    fn no_such_place(place: &Place) -> RuntimeError {
        RuntimeError {
            diagnostic: Box::new(Diagnostic::error(
                codes::UNDEFINED_VARIABLE,
                place.span,
                "this is not a place that can be written to",
            )),
        }
    }

    fn while_loop(&mut self, w: &While, env: &mut Env) -> Result<Flow, RuntimeError> {
        loop {
            // `take!`, because a condition is an expression like any other: `while (if c
            // { return 1; } else { true })` leaves the function from the condition.
            let c = take!(self, &w.cond, env);
            if !self.as_bool(&c, &w.cond)? {
                return Ok(Flow::Fell(None));
            }

            let Some(left) = self.iterations_left.checked_sub(1) else {
                return Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::ITERATION_LIMIT,
                            w.span,
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

            match self.block(&w.body, env)? {
                Flow::Fell(_) | Flow::Continue => {}
                Flow::Break => return Ok(Flow::Fell(None)),
                // A `return` inside a loop leaves the function, not just the loop.
                r @ Flow::Return(_) => return Ok(r),
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
    fn eval(&mut self, e: &Expr, env: &mut Env) -> Result<Flow, RuntimeError> {
        let value = match e {
            Expr::Int { value, .. } => Value::Int(*value),
            Expr::Bool { value, .. } => Value::Bool(*value),
            // A literal's bytes are the source's bytes, which are valid UTF-8 because
            // the source is. Nothing downstream may assume that of every `Str`.
            // A literal is **immortal**: the compiled program puts it in a constant and
            // the interpreter must not pretend it allocated one, or the two would
            // disagree about what a program with no allocation did.
            Expr::Str { value, .. } => Value::Str(Str::immortal(value.clone().into_bytes())),

            Expr::Var { name, span } => env
                .iter()
                .rev()
                .find_map(|f| f.get(name).cloned())
                .ok_or_else(|| RuntimeError {
                    // Unreachable if checking ran. A diagnostic rather than a panic, so a
                    // checker bug surfaces as a report.
                    diagnostic: Box::new(Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    )),
                })?,

            Expr::Neg {
                operand, op_span, ..
            } => match take!(self, operand, env) {
                // `-i64::MIN` has no representable result, so negation overflows exactly
                // once — and it is the value people reach for when testing edges.
                Value::Int(v) => v
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::INTEGER_OVERFLOW,
                            *op_span,
                            "this negation overflows `Int`",
                        )),
                    })?,
                other => {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::INVALID_OPERANDS,
                            *op_span,
                            format!("`-` applies to `Int`, but this is `{other}`"),
                        )),
                    })
                }
            },

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => {
                // Left to right, as the spec fixes — and `take!` preserves that: a flow
                // raised by the left operand leaves before the right one is evaluated.
                let l = take!(self, lhs, env);
                let r = take!(self, rhs, env);
                self.binary(*op, &l, &r, *op_span)?
            }

            Expr::Construct {
                ty_span, fields, ..
            } => {
                // **In the order written**, which `docs/spec.md` fixes as the evaluation
                // order and which physical layout has nothing to do with. A failure part
                // way through drops what has been built so far — the `Vec` goes with the
                // error — and that drop is what releases a string backing or a sequence
                // reference an earlier initialiser had taken. The counters observe it, so
                // the cleanup is evidence rather than an assumption about the host.
                let Some(def) = self.res.construction(*ty_span) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::UNKNOWN_TYPE,
                            *ty_span,
                            "this record was not resolved",
                        )),
                    });
                };
                let arity = self.res.records().get(def).map_or(0, |d| d.fields.len());
                let mut built: Vec<Option<Value>> = vec![None; arity];
                for f in fields {
                    let v = take!(self, &f.value, env);
                    // Placed by **declaration** index, taken by name, so the order the
                    // author wrote the initialisers in cannot change the record's shape.
                    let at = self
                        .res
                        .records()
                        .get(def)
                        .and_then(|d| d.field(&f.name).map(|(i, _)| i as usize));
                    if let Some(slot) = at.and_then(|i| built.get_mut(i)) {
                        *slot = Some(v);
                    }
                }
                // Checking guarantees every field was given exactly one value, so an
                // absent one is a checker bug rather than a program's mistake.
                let mut values = Vec::with_capacity(arity);
                for (i, v) in built.into_iter().enumerate() {
                    match v {
                        Some(v) => values.push(v),
                        None => {
                            let name = self
                                .res
                                .records()
                                .get(def)
                                .and_then(|d| d.fields.get(i))
                                .map_or_else(|| "a field".to_owned(), |f| format!("`{}`", f.name));
                            return Err(RuntimeError {
                                diagnostic: Box::new(Diagnostic::error(
                                    codes::MISSING_FIELD,
                                    *ty_span,
                                    format!("{name} was given no value"),
                                )),
                            });
                        }
                    }
                }
                Value::Record(Rec::new(def, values))
            }

            Expr::Variant {
                ty_span,
                variant_span,
                fields,
                ..
            } => {
                // In the order written, like a record construction, and the same drop on
                // the way out: a failure part way through takes the `Vec` with it, which
                // releases whatever the earlier initialisers had taken. The discriminant
                // is only written once every payload field exists, so no partially built
                // value is ever a value of a variant.
                let Some(to) = self.res.variant(*variant_span) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::UNKNOWN_VARIANT,
                            *variant_span,
                            "this variant was not resolved",
                        )),
                    });
                };
                let arity = self
                    .res
                    .enums()
                    .get(to.enumeration)
                    .and_then(|d| d.variants.get(to.variant as usize))
                    .map_or(0, |v| v.fields.len());
                let mut built: Vec<Option<Value>> = vec![None; arity];
                for f in fields {
                    let v = take!(self, &f.value, env);
                    let at = self.res.payload(f.name_span).map(|p| p.field as usize);
                    if let Some(slot) = at.and_then(|i| built.get_mut(i)) {
                        *slot = Some(v);
                    }
                }
                let mut values = Vec::with_capacity(arity);
                for v in built {
                    match v {
                        Some(v) => values.push(v),
                        None => {
                            return Err(RuntimeError {
                                diagnostic: Box::new(Diagnostic::error(
                                    codes::MISSING_FIELD,
                                    *ty_span,
                                    "a payload field was given no value",
                                )),
                            })
                        }
                    }
                }
                Value::Enum(EnumVal::new(to.enumeration, to.variant, values))
            }

            Expr::Match {
                scrutinee,
                arms,
                span,
            } => {
                return self.match_expr(scrutinee, arms, *span, env);
            }

            Expr::Propagate {
                operand, op_span, ..
            } => return self.propagate(operand, *op_span, env),

            Expr::Field {
                base, name_span, ..
            } => {
                let base = take!(self, base, env);
                // The field's value is **cloned out**, which is the whole of rule 3 for a
                // projection: what comes out holds its own reference to whatever backs
                // it, so it survives the record it came from — including when that record
                // was a temporary that dies on this very line.
                let (Value::Record(r), Some(at)) = (&base, self.res.field(*name_span)) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::INVALID_PROJECTION,
                            *name_span,
                            "this is not a record",
                        )),
                    });
                };
                let Some(v) = r.field(at.field) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::UNKNOWN_FIELD,
                            *name_span,
                            "this record has no such field",
                        )),
                    });
                };
                v.clone()
            }

            Expr::Call {
                callee,
                callee_span,
                args,
                span,
                ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(take!(self, a, env));
                }
                // What this call means was decided once, by the checker, and is read here
                // rather than decided again. "Built-ins before user functions" used to be
                // applied at this line with its own copy of the rule; it is now one of the
                // two answers `Callee` can hold.
                let resolved = self.res.callee(*callee_span);
                if let Some(Callee::Builtin(i)) = resolved {
                    return Ok(Flow::Fell(Some(self.intrinsic(i, &values, *span)?)));
                }

                let program = Arc::clone(&self.program);
                let f = self
                    .decl_at(*callee_span)
                    .and_then(|i| program.functions.get(i))
                    .ok_or_else(|| RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::UNDEFINED_FUNCTION,
                            *span,
                            format!("`{callee}` is not a function"),
                        )),
                    })?;

                self.depth += 1;
                if self.depth > self.max_depth {
                    self.depth -= 1;
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::CALL_DEPTH_LIMIT,
                                *span,
                                format!("`{callee}` recursed deeper than {}", self.max_depth),
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

                // A fresh environment: a function sees its parameters and nothing from
                // the caller. Lexical scope, not dynamic.
                let mut call_env: Env = vec![Frame::default()];
                if let Some(frame) = call_env.last_mut() {
                    for (p, v) in f.params.iter().zip(values) {
                        frame.insert(p.name.clone(), v);
                    }
                }
                let out = self.call_body(Self::body_of(f)?, &mut call_env, callee);
                self.depth -= 1;
                // A call absorbs its callee's `return`: that is what makes it a call and
                // not a jump. Nothing propagates outward from here but a genuine error.
                out?
            }

            // Both return their flow directly rather than a value. An `if` with no `else`
            // produces nothing, and whether that is allowed is the caller's question —
            // `take!` reports it where a value was needed, and `stmt` drops it where one
            // was not.
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let c = take!(self, cond, env);
                return if self.as_bool(&c, cond)? {
                    self.block(then_block, env)
                } else if let Some(b) = else_block {
                    self.block(b, env)
                } else {
                    Ok(Flow::Fell(None))
                };
            }

            Expr::Block(b) => return self.block(b, env),
        };
        Ok(Flow::Fell(Some(value)))
    }

    /// The body of a function that is about to run.
    ///
    /// A missing body means the parser reported an error, and a program with errors is
    /// never evaluated — so this is unreachable. It reports rather than panicking,
    /// because a bug in that reasoning should still produce something readable.
    fn body_of(f: &Function) -> Result<&Block, RuntimeError> {
        f.body.as_ref().ok_or_else(|| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    f.name_span,
                    format!("`{}` has no body to run", f.name),
                )
                .help("its body did not parse; fix the reported syntax error first"),
            ),
        })
    }

    fn binary(
        &self,
        op: BinOp,
        l: &Value,
        r: &Value,
        op_span: nazm_span::Span,
    ) -> Result<Value, RuntimeError> {
        use Value::{Bool, Int};

        let overflow = |what: &str| {
            RuntimeError {
            diagnostic: Box::new(Diagnostic::error(
                codes::INTEGER_OVERFLOW,
                op_span,
                format!("this {what} overflows `Int`"),
            )
            .help(
                "`Int` is a 64-bit signed integer; overflow stops the program rather than wrapping",
            )),
        }
        };

        let same = if matches!(op, BinOp::Eq | BinOp::NotEq) {
            equal(l, r)
        } else {
            None
        };
        match (op, l, r) {
            (BinOp::Add, Int(a), Int(b)) => a
                .checked_add(*b)
                .map(Int)
                .ok_or_else(|| overflow("addition")),
            (BinOp::Sub, Int(a), Int(b)) => a
                .checked_sub(*b)
                .map(Int)
                .ok_or_else(|| overflow("subtraction")),
            (BinOp::Mul, Int(a), Int(b)) => a
                .checked_mul(*b)
                .map(Int)
                .ok_or_else(|| overflow("multiplication")),

            (BinOp::Div | BinOp::Rem, Int(_), Int(0)) => Err(RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(codes::DIVISION_BY_ZERO, op_span, "division by zero")
                        .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`"),
                ),
            }),
            // `checked_div` also catches i64::MIN / -1, which overflows rather than
            // dividing — the one division that is not a zero check.
            (BinOp::Div, Int(a), Int(b)) => a
                .checked_div(*b)
                .map(Int)
                .ok_or_else(|| overflow("division")),
            // `i64::MIN % -1` is mathematically 0 and 0 is representable, so nothing
            // overflows. `checked_rem` says `None` anyway, because the hardware
            // instruction that computes it faults on the *quotient*. Inheriting that
            // would make the rule "`%` fails on a zero divisor, and also on one exact
            // pair of operands" — a wart an agent would have to memorise. `wrapping_rem`
            // is the correct answer here, and the zero divisor is already handled above.
            (BinOp::Rem, Int(a), Int(b)) => Ok(Int(a.wrapping_rem(*b))),

            (BinOp::Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
            (BinOp::LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
            (BinOp::Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
            (BinOp::GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),

            // One relation, and `!=` is its negation rather than a second walk that could
            // drift from it. A value with no equality falls to the arm below.
            (BinOp::Eq, _, _) if same.is_some() => Ok(Bool(same == Some(true))),
            (BinOp::NotEq, _, _) if same.is_some() => Ok(Bool(same == Some(false))),

            // Unreachable after checking. A diagnostic rather than a panic, for the same
            // reason as the undefined-variable arm above.
            (op, a, b) => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::INVALID_OPERANDS,
                    op_span,
                    format!(
                        "`{}` does not apply to `{}` and `{}`",
                        op.symbol(),
                        type_name(a),
                        type_name(b)
                    ),
                )),
            }),
        }
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
    }
}
