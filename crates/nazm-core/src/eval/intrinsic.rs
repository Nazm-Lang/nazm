//! The built-in functions: I/O, strings and sequences, bytes and text, and the numeric ones.

use super::arith::{as_num, from_num, num_failure};
use super::channel::{select, Channel};
use super::value::{seq, BytesView, Seq, Str, Value};
use super::*;

/// `N0416`'s sentence, which the compiled program writes too (`nazm_lir::EXIT_STATUS`).
const EXIT_STATUS_MESSAGE: &str = "this exit status is outside 0 to 255";

/// A `Bytes` or `Text` built-in (Gate 2), with the sentences the compiled program writes;
/// `None` for any other. `ty` is the call's result type, which is a read's `T`.
#[allow(clippy::too_many_lines)]
pub(super) fn bytes_text_intrinsic(
    op: Intrinsic,
    args: &[Value],
    ty: Option<nazm_sema::Type>,
    at: Span,
) -> Option<Result<Value, RuntimeError>> {
    let outside = |message: &str, detail: String| RuntimeError {
        diagnostic: Box::new(
            Diagnostic::error(codes::INDEX_OUT_OF_BOUNDS, at, message).help(detail),
        ),
    };
    let not_utf8 = |message: &str| fail(codes::NOT_UTF8, at, message);
    let bug = || {
        fail(
            codes::TYPE_MISMATCH,
            at,
            format!("`{}` was called with the wrong values", op.name()),
        )
    };
    // `0 <= i` and `i + width <= len`, without overflow.
    let within = |i: i64, width: usize, len: usize| {
        usize::try_from(i).is_ok_and(|i| i.checked_add(width).is_some_and(|end| end <= len))
    };
    let range = |from: i64, to: i64, len: usize| {
        from >= 0 && to >= from && usize::try_from(to).is_ok_and(|t| t <= len)
    };
    let at_boundary = |s: &[u8], i: i64| match usize::try_from(i) {
        Ok(i) if i == s.len() => true,
        Ok(i) => s.get(i).is_some_and(|b| b & 0xC0 != 0x80),
        Err(_) => false,
    };
    let scalar = |c: i64| u32::try_from(c).ok().and_then(char::from_u32);
    let index = |i: i64| usize::try_from(i).unwrap_or(0);
    let count = |n: usize| i64::try_from(n).ok().map(Value::Int);
    Some(match (op, args) {
        (Intrinsic::BytesNew, [Value::Int(n)]) => match usize::try_from(*n) {
            Ok(n) => Ok(Value::Bytes(BytesView::whole(vec![0; n]))),
            Err(_) => Err(outside(
                "a buffer's length is never negative",
                format!("asked for a buffer of {n} bytes"),
            )),
        },
        (Intrinsic::BytesFromStr, [Value::Str(s)]) => {
            Ok(Value::Bytes(BytesView::whole(s.bytes().to_vec())))
        }
        (Intrinsic::BytesToStr, [Value::Bytes(b)]) => Ok(Value::Str(Str::heap(b.read()))),
        (Intrinsic::BytesLen, [Value::Bytes(b)]) => count(b.len).ok_or_else(bug),
        (Intrinsic::TextLen, [Value::Str(s)]) => count(s.len()).ok_or_else(bug),
        (Intrinsic::BytesGet, [Value::Bytes(b), Value::Int(i)]) => {
            if within(*i, 1, b.len) {
                Ok(Value::Num(NumVal::wrapped(
                    NumKind::U8,
                    u64::from(b.read().get(index(*i)).copied().unwrap_or(0)),
                )))
            } else {
                Err(outside(
                    "this index is outside the `Bytes`",
                    format!("asked for byte {i} of a `Bytes` {} long", b.len),
                ))
            }
        }
        (Intrinsic::BytesSet, [Value::Bytes(b), Value::Int(i), v]) => {
            if within(*i, 1, b.len) {
                let byte = as_num(v).map_or(0, |n| n.bits.to_le_bytes()[0]);
                b.write(index(*i), &[byte]);
                Ok(Value::Int(0))
            } else {
                Err(outside(
                    "this index is outside the `Bytes`",
                    format!("asked for byte {i} of a `Bytes` {} long", b.len),
                ))
            }
        }
        (Intrinsic::BytesSlice, [Value::Bytes(b), Value::Int(from), Value::Int(to)]) => {
            if range(*from, *to, b.len) {
                Ok(Value::Bytes(BytesView {
                    buf: Arc::clone(&b.buf),
                    start: b.start + index(*from),
                    len: index(*to) - index(*from),
                }))
            } else {
                Err(outside(
                    "this slice is not inside the `Bytes`",
                    format!("asked for {from}..{to} of a `Bytes` {} long", b.len),
                ))
            }
        }
        (Intrinsic::BytesCopy, [Value::Bytes(d), Value::Bytes(s)]) => {
            if s.len > d.len {
                Err(outside(
                    "the source is longer than the destination",
                    format!("{} bytes into {}", s.len, d.len),
                ))
            } else {
                // Read first: the two may be views of one buffer.
                let bytes = s.read();
                d.write(0, &bytes);
                count(s.len).ok_or_else(bug)
            }
        }
        (Intrinsic::BytesReadLe | Intrinsic::BytesReadBe, [Value::Bytes(b), Value::Int(i)]) => {
            let kind = ty.and_then(nazm_sema::Type::num).unwrap_or(NumKind::I64);
            let w = kind.bytes() as usize;
            if !within(*i, w, b.len) {
                return Some(Err(outside(
                    "this read is outside the `Bytes`",
                    format!("asked for {w} bytes at {i} of a `Bytes` {} long", b.len),
                )));
            }
            let all = b.read();
            let mut raw = [0u8; 8];
            let at = index(*i);
            for j in 0..w {
                let byte = all.get(at + j).copied().unwrap_or(0);
                let k = if op == Intrinsic::BytesReadLe {
                    j
                } else {
                    w - 1 - j
                };
                if let Some(slot) = raw.get_mut(k) {
                    *slot = byte;
                }
            }
            Ok(from_num(NumVal::wrapped(kind, u64::from_le_bytes(raw))))
        }
        (
            Intrinsic::BytesWriteLe | Intrinsic::BytesWriteBe,
            [Value::Bytes(b), Value::Int(i), v],
        ) => {
            let Some(n) = as_num(v) else {
                return Some(Err(bug()));
            };
            let w = n.kind.bytes() as usize;
            if !within(*i, w, b.len) {
                return Some(Err(outside(
                    "this write is outside the `Bytes`",
                    format!("asked for {w} bytes at {i} of a `Bytes` {} long", b.len),
                )));
            }
            let le = n.bits.to_le_bytes();
            let mut out = vec![0u8; w];
            for (j, slot) in out.iter_mut().enumerate() {
                let k = if op == Intrinsic::BytesWriteLe {
                    j
                } else {
                    w - 1 - j
                };
                *slot = le.get(k).copied().unwrap_or(0);
            }
            b.write(index(*i), &out);
            Ok(Value::Int(0))
        }
        (Intrinsic::Utf8Valid, [Value::Str(s)]) => {
            Ok(Value::Bool(std::str::from_utf8(s.bytes()).is_ok()))
        }
        (Intrinsic::TextFromStr, [Value::Str(s)]) => {
            if std::str::from_utf8(s.bytes()).is_ok() {
                Ok(Value::Str(s.clone()))
            } else {
                Err(not_utf8("this is not UTF-8"))
            }
        }
        (Intrinsic::TextAssume | Intrinsic::TextAsStr, [Value::Str(s)]) => {
            Ok(Value::Str(s.clone()))
        }
        (Intrinsic::TextConcat, [Value::Str(a), Value::Str(b)]) => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a.bytes());
            out.extend_from_slice(b.bytes());
            Ok(Value::Str(Str::heap(out)))
        }
        (Intrinsic::TextIsBoundary, [Value::Str(s), Value::Int(i)]) => {
            Ok(Value::Bool(at_boundary(s.bytes(), *i)))
        }
        (Intrinsic::TextSlice, [Value::Str(s), Value::Int(from), Value::Int(to)]) => {
            if !range(*from, *to, s.len()) {
                Err(outside(
                    "this slice is not inside the `Text`",
                    format!("asked for {from}..{to} of a `Text` {} bytes long", s.len()),
                ))
            } else if !(at_boundary(s.bytes(), *from) && at_boundary(s.bytes(), *to)) {
                Err(not_utf8("this is not UTF-8"))
            } else {
                Ok(Value::Str(s.slice(index(*from), index(*to))))
            }
        }
        (Intrinsic::TextScalarAt | Intrinsic::TextNext, [Value::Str(s), Value::Int(at_i)]) => {
            if !within(*at_i, 1, s.len()) {
                return Some(Err(outside(
                    "this index is outside the `Text`",
                    format!("asked for byte {at_i} of a `Text` {} bytes long", s.len()),
                )));
            }
            if !at_boundary(s.bytes(), *at_i) {
                return Some(Err(not_utf8("this is not UTF-8")));
            }
            let rest = s.bytes().get(index(*at_i)..).unwrap_or(&[]);
            let c = (1..=rest.len().min(4))
                .find_map(|k| std::str::from_utf8(rest.get(..k)?).ok())
                .and_then(|t| t.chars().next());
            match (op, c) {
                (Intrinsic::TextScalarAt, Some(c)) => Ok(Value::Int(i64::from(u32::from(c)))),
                (_, Some(c)) => Ok(Value::Int(*at_i + c.len_utf8() as i64)),
                (_, None) => Err(bug()),
            }
        }
        (Intrinsic::TextScalarCount, [Value::Str(s)]) => {
            count(s.bytes().iter().filter(|&&b| b & 0xC0 != 0x80).count()).ok_or_else(bug)
        }
        (Intrinsic::ScalarValid, [Value::Int(c)]) => Ok(Value::Bool(scalar(*c).is_some())),
        (Intrinsic::ScalarText, [Value::Int(c)]) => match scalar(*c) {
            Some(ch) => Ok(Value::Str(Str::heap(ch.to_string().into_bytes()))),
            None => Err(not_utf8("this is not a Unicode scalar value")),
        },
        _ => return None,
    })
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

impl Interpreter {
    /// Evaluate a built-in.
    ///
    /// Arity and argument types were established by checking, so a mismatch here is a
    /// compiler bug and is reported as one rather than assumed away. The failures that
    /// remain are the specified ones: an index outside a string (`N0405`) and an
    /// overflowing length, which cannot happen for a string that fits in memory but is
    /// handled rather than truncated.
    pub(super) fn intrinsic(
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
            // Atomics (Gate 3): sequentially consistent, as `docs/spec.md` promises everywhere.
            (Intrinsic::AtomicNew, [Value::Int(v)]) => Ok(Value::Atomic(Arc::new(
                std::sync::atomic::AtomicI64::new(*v),
            ))),
            (Intrinsic::AtomicLoad, [Value::Atomic(a)]) => {
                Ok(Value::Int(a.load(std::sync::atomic::Ordering::SeqCst)))
            }
            (Intrinsic::AtomicStore, [Value::Atomic(a), Value::Int(v)]) => {
                a.store(*v, std::sync::atomic::Ordering::SeqCst);
                Ok(Value::Int(0))
            }
            (Intrinsic::AtomicSwap, [Value::Atomic(a), Value::Int(v)]) => {
                Ok(Value::Int(a.swap(*v, std::sync::atomic::Ordering::SeqCst)))
            }
            (
                Intrinsic::AtomicCompareSwap,
                [Value::Atomic(a), Value::Int(old), Value::Int(new)],
            ) => {
                use std::sync::atomic::Ordering::SeqCst;
                Ok(Value::Bool(
                    a.compare_exchange(*old, *new, SeqCst, SeqCst).is_ok(),
                ))
            }
            // The cell is left as it was when the sum would leave `Int`.
            (Intrinsic::AtomicAdd, [Value::Atomic(a), Value::Int(d)]) => {
                use std::sync::atomic::Ordering::SeqCst;
                a.fetch_update(SeqCst, SeqCst, |x| x.checked_add(*d))
                    .map(Value::Int)
                    .map_err(|_| RuntimeError {
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
                    })
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
            // Gate 2 (§11): nanoseconds on the same monotonic clock, the wall clock, and a wait.
            (Intrinsic::TimeNowNs, []) => {
                static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let base = BASE.get_or_init(std::time::Instant::now);
                Ok(Value::Int(
                    i64::try_from(base.elapsed().as_nanos()).unwrap_or(i64::MAX),
                ))
            }
            (Intrinsic::TimeWallNs, []) => Ok(Value::Int(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX)),
            )),
            (Intrinsic::TimeSleepMs, [Value::Int(ms)]) => {
                if *ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(ms.unsigned_abs()));
                }
                Ok(Value::Int(0))
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
                // A status is `0..=255`, what every host reports unchanged (v1, Q1-B-05):
                // anything else fails, as natively, rather than reaching the host truncated.
                let Ok(status) = u8::try_from(*code) else {
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(codes::EXIT_STATUS, span, EXIT_STATUS_MESSAGE)
                                .help(format!("`exit_with({code})`: a status is 0 to 255")),
                        ),
                    });
                };
                // Ends the process, here as natively. The interpreter runs on a thread
                // it owns, and `exit` from a thread ends the whole process — which is
                // the point: `exit_with` means the program is over, not that this
                // evaluation is.
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
                std::process::exit(i32::from(status))
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
}
