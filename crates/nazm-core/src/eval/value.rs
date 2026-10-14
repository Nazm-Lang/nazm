//! The interpreter's values: numbers, strings, byte views, records, enum values, function
//! values and sequences, with how each is compared, printed and reclaimed.

use super::channel::Channel;
use super::counters::{
    ALLOCATED, CLOSURES_ALLOCATED, CLOSURES_RECLAIMED, RECLAIMED, STRINGS_ALLOCATED,
    STRINGS_RECLAIMED,
};
use super::*;

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
    /// A fixed-size array (Gate 2): its elements, inline, by value. Plain data only — numbers,
    /// `Bool`s, records and enums of them — so a clone is the copy the language specifies.
    Array(Vec<Value>),
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
    /// A `Bytes` (Gate 2): a view of a fixed-length mutable buffer, **by reference** — a
    /// write through one view is seen through every view of the buffer. A `Text` needs no
    /// variant: it is a [`Value::Str`] the checker knows is UTF-8.
    Bytes(BytesView),
    /// An `OsHandle` (Gate 2): `generation << 32 | descriptor`, or a failed open's status.
    /// The object it names is in the process's handle table (`eval::os`).
    OsHandle(i64),
    /// A bounded channel of `Int`, by reference.
    ///
    /// The one value designed to be shared between tasks: its operations *are* the
    /// synchronisation, which is why `docs/spec.md` lets it cross a `spawn` while a
    /// sequence may not.
    Chan(Arc<Channel>),
    /// One `Int` cell, by reference (Gate 3): every copy reaches the same cell, and every
    /// operation on it is sequentially consistent.
    Atomic(Arc<std::sync::atomic::AtomicI64>),
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
    /// A `Vec[T]`: a handle to shared, mutable, counted storage, by the sequence law. N11. An
    /// `Ints` and a `Strs` are this too, since R1 (5b.18) made them `Vec[Int]` and `Vec[Str]`.
    ///
    /// Two bindings of the same sequence are the same storage, and a push through one is
    /// visible through the other — `docs/spec.md`'s **owned mutable handle**, which the native
    /// representation, a pointer to a header, behaves as by construction. What is claimed is
    /// reclamation: the storage goes when the last reference to it dies, [`Seq`]'s job here and
    /// a reference count's in a compiled program, compared case by case in
    /// `crates/nazm-cli/tests/memory.rs`. `Arc<Mutex<…>>` because the pipeline runs on a thread
    /// the interpreter owns, so a `Value` has to be `Send`; the lock is never contended.
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
    pub(super) func: nazm_sema::FnId,
    /// What it captured, in order.
    pub(super) carried: Vec<Value>,
}

impl FuncVal {
    /// A new function value — one object, counted as the native closure object is.
    pub(super) fn new(func: nazm_sema::FnId, carried: Vec<Value>) -> Arc<Self> {
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
    pub(super) fields: Vec<Value>,
}

impl Value {
    /// The type a value of an implementable type has (N78): what a trait call dispatches on.
    /// `None` for a value no impl can be for.
    pub(super) fn runtime_type(&self) -> Option<nazm_sema::Type> {
        Some(match self {
            Self::Int(_) => nazm_sema::Type::Int,
            Self::Num(n) => nazm_sema::Type::of_num(n.kind),
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

    /// Which record this is a value of.
    pub(super) const fn def(&self) -> RecordId {
        self.def
    }

    pub(super) fn field_mut(&mut self, at: u32) -> Option<&mut Value> {
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
    pub(super) variant: u32,
    pub(super) fields: Vec<Value>,
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
    /// Which enum this is a value of.
    pub(super) const fn def(&self) -> EnumId {
        self.def
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

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
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
pub(super) fn seq<T>(lock: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
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
    /// Storage an append made (R1b, D2): counted like `Heap`, with `room` bytes the compiled
    /// program's allocation has, so a later append can grow it in place exactly when the
    /// compiled program's would.
    Grown { bytes: Vec<u8>, room: usize },
}

impl Backing {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Immortal(b) | Self::Heap(b) | Self::Grown { bytes: b, .. } => b,
        }
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        if matches!(self, Self::Heap(_) | Self::Grown { .. }) {
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

    /// `str_append` (R1b, D2): this string followed by `tail`, written in place when this
    /// view is the only reference to storage an append made and the result fits its room —
    /// the compiled program's rule, so the two allocate the same strings. Otherwise a copy
    /// into a new allocation of `max(2n, 16)` bytes, which the next append can grow.
    #[must_use]
    pub fn append(mut self, tail: &[u8]) -> Self {
        let n = self.len + tail.len();
        let end = self.start + self.len;
        if let Some(Backing::Grown { bytes, room }) = Arc::get_mut(&mut self.backing) {
            if self.start + n <= *room {
                bytes.truncate(end);
                bytes.extend_from_slice(tail);
                self.len = n;
                return self;
            }
        }
        let room = (2 * n).max(16);
        let mut bytes = Vec::with_capacity(room);
        bytes.extend_from_slice(self.bytes());
        bytes.extend_from_slice(tail);
        STRINGS_ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::whole(Backing::Grown { bytes, room })
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

/// A `Bytes` (Gate 2): `len` bytes starting at `start` of a shared buffer.
///
/// The native value is `Str`'s triple over a buffer `nz.str_alloc` made, so the buffer is
/// counted with the strings — the two implementations' memory reports count the same
/// allocations. Cloning shares the buffer: a `Bytes` is a handle.
#[derive(Debug, Clone)]
pub struct BytesView {
    pub(super) buf: Arc<Buffer>,
    pub(super) start: usize,
    pub(super) len: usize,
}

/// A `Bytes` buffer: fixed in length once made, its bytes written in place.
#[derive(Debug)]
pub struct Buffer(Mutex<Vec<u8>>);

impl Buffer {
    fn new(bytes: Vec<u8>) -> Arc<Self> {
        STRINGS_ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Arc::new(Self(Mutex::new(bytes)))
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        STRINGS_RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl BytesView {
    pub(super) fn whole(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        Self {
            buf: Buffer::new(bytes),
            start: 0,
            len,
        }
    }

    /// A copy of the bytes this view names.
    pub(super) fn read(&self) -> Vec<u8> {
        self.buf
            .0
            .lock()
            .ok()
            .and_then(|b| b.get(self.start..self.start + self.len).map(<[u8]>::to_vec))
            .unwrap_or_default()
    }

    /// Write `bytes` at `at` within this view; the caller has checked the range.
    pub(super) fn write(&self, at: usize, bytes: &[u8]) {
        if let Ok(mut b) = self.buf.0.lock() {
            let from = self.start + at;
            if let Some(dst) = b.get_mut(from..from + bytes.len()) {
                dst.copy_from_slice(bytes);
            }
        }
    }
}

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
///
/// # Release is not recursion (R1a)
///
/// A `Vec[Value]` can hold values that hold sequences, as deep as a program makes them — a
/// `Box` chain a million long is a million sequences inside one another — and the host's own
/// drop would recurse once per level and overflow the stack. So a sequence of values gives its
/// elements to a worklist instead of dropping them in place, and the first release on a thread
/// empties that worklist in a loop: a release nested inside it only adds to it. The stack a
/// release takes is bounded by the types, never by the value (`docs/spec.md`, *Immutable
/// indirection*).
#[derive(Debug, Default)]
pub struct Seq<T: Drain>(Mutex<T>);

/// What a sequence's storage hands over when it is released: its elements that can own
/// sequences, for the worklist; nothing for those that cannot.
pub trait Drain {
    fn drain_into(&mut self, out: &mut Vec<Value>);
}

impl Drain for Vec<i64> {
    fn drain_into(&mut self, _: &mut Vec<Value>) {}
}

impl Drain for Vec<Str> {
    fn drain_into(&mut self, _: &mut Vec<Value>) {}
}

impl Drain for Vec<Value> {
    fn drain_into(&mut self, out: &mut Vec<Value>) {
        out.append(self);
    }
}

std::thread_local! {
    /// The worklist of the release in progress on this thread, if one is.
    static RELEASING: std::cell::RefCell<Option<Vec<Value>>> = const { std::cell::RefCell::new(None) };
}

/// Release `values`: queued behind the release already in progress on this thread, or, if
/// none is, dropped one at a time in a loop until nothing they owned is left.
fn release(mut values: Vec<Value>) {
    if values.is_empty() {
        return;
    }
    let queued = RELEASING.with(|r| match r.try_borrow_mut() {
        Ok(mut slot) => match slot.as_mut() {
            Some(queue) => {
                queue.append(&mut values);
                true
            }
            None => {
                *slot = Some(Vec::new());
                false
            }
        },
        // Borrowed only by this function, never across a drop; a failure here would be a
        // release inside the queue's own bookkeeping, which nothing does.
        Err(_) => false,
    });
    if queued {
        return;
    }
    loop {
        // Each value is dropped outside the borrow, so a sequence it releases queues its own.
        drop(values);
        values = RELEASING.with(|r| {
            r.try_borrow_mut()
                .ok()
                .and_then(|mut slot| slot.as_mut().map(std::mem::take))
                .unwrap_or_default()
        });
        if values.is_empty() {
            break;
        }
    }
    RELEASING.with(|r| {
        if let Ok(mut slot) = r.try_borrow_mut() {
            *slot = None;
        }
    });
}

impl<T: Drain> Seq<T> {
    pub(super) fn new(value: T) -> Self {
        ALLOCATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(Mutex::new(value))
    }
}

impl<T: Drain> Drop for Seq<T> {
    fn drop(&mut self) {
        RECLAIMED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut owned = Vec::new();
        if let Ok(inner) = self.0.get_mut() {
            inner.drain_into(&mut owned);
        }
        release(owned);
    }
}

impl<T: Drain> std::ops::Deref for Seq<T> {
    type Target = Mutex<T>;

    fn deref(&self) -> &Mutex<T> {
        &self.0
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Num(v) => f.write_str(&numeric::to_str(*v)),
            Self::Array(v) => write!(f, "array({} elements)", v.len()),
            Self::Bool(v) => write!(f, "{v}"),
            // Rendering, not semantics: a `Str` that is not valid UTF-8 has no text
            // form, and showing the replacement character is a display decision. No
            // operation on `Str` goes through here.
            Self::Str(v) => write!(f, "{}", String::from_utf8_lossy(v.bytes())),
            // A sequence is never a program's result — `main` returns `Int` — so this
            // is for diagnostics only, and says what it is rather than dumping it.
            Self::Chan(_) => f.write_str("Chan"),
            Self::Bytes(b) => write!(f, "Bytes({} bytes)", b.len),
            Self::OsHandle(_) => f.write_str("OsHandle"),
            Self::Atomic(_) => f.write_str("Atomic"),
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
