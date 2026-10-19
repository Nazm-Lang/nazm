//! Operators and literals: arithmetic, comparison, arrays and indexing, negation and complement,
//! numeric literals, and conversions.

use super::clauses::is_number_literal;
use super::layout::{array_length, array_refusal};
use super::written::not_generic;
use super::*;

impl Checker<'_> {
    fn check_binary(&mut self, b: &BinaryCheck, expected: Option<Type>) -> Type {
        let BinaryCheck {
            op,
            lt,
            rt,
            op_span,
            lhs_span,
            rhs_span,
        } = *b;
        let ok = match op {
            // Arithmetic is on two numbers of **one** type (Gate 2; `Int` only before it). No
            // string concatenation, and `+` on two `Bool`s is a mistake rather than something
            // to define. Nothing converts: `Int + UInt8` is refused.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                lt == rt && lt.num().is_some()
            }
            // Ordering is derived (L6): numbers, `Bool`, `Str` by its bytes, and records, enums
            // and tuples of ordered components, by the names of their fields and variants
            // (`docs/user/spec.md`, *Ordering and hashing are derived*).
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                lt == rt && self.res.types().ordering(lt).is_ok()
            }
            // The bit operators (Gate 2): two integers of one type, or for a shift an integer
            // and an `Int` amount.
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => lt == rt && lt.is_integer(),
            BinOp::Shl | BinOp::Shr => lt.is_integer() && rt == Type::Int,
            // Equality needs **one** type — two records with identical fields are two
            // types, and nothing converts — and that type must have equality, which is
            // derived rather than listed (`docs/user/spec.md`, *Equality is derived*, N13).
            // `Int`, `Bool` and `Str` have it. A sequence, a `Vec` and a `Chan` do not:
            // equality on a handle would have to mean either "the same storage" or "the
            // same contents", the language has chosen neither, and offering one silently
            // is how a program comes to depend on the wrong answer. A record or an enum has
            // it exactly when everything it contains does, over every variant. A type
            // parameter does not: `==` is not valid for every type, and an unconstrained
            // `T` supports only what every type supports.
            BinOp::Eq | BinOp::NotEq => lt == rt && self.res.types().equality(lt).is_ok(),
            // `&&` and `||` are `Bool` only (N49): no truthiness, so `1 && 2` is a mistake.
            BinOp::And | BinOp::Or => lt == Type::Bool && rt == Type::Bool,
        };

        if !ok {
            // "compare" is only right for a comparison. Saying it of `+` describes the
            // wrong operation and reads as a compiler that does not know what it is
            // looking at.
            let verb = if op.is_comparison() {
                "compare"
            } else {
                "combine"
            };
            let (slt, srt) = (self.show(lt), self.show(rt));
            // For a composite without equality, the component responsible — the first a
            // reader would find, `Outer.inner.values` — so the refusal says why rather than
            // only that.
            let blocked = match op {
                BinOp::Eq | BinOp::NotEq if lt == rt => self.res.types().equality(lt).err(),
                BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq if lt == rt => {
                    self.res.types().ordering(lt).err()
                }
                _ => None,
            };
            let property = if op.is_equality() {
                "equality"
            } else {
                "order"
            };
            // A type parameter without a requirement is the one blocker the author can lift
            // where they stand, by declaring it (N14); say so, naming the parameter.
            let unrequired = blocked.as_ref().and_then(|w| match w.ty {
                Type::Param(_) if op.is_equality() => Some(self.show(w.ty)),
                _ => None,
            });
            let detail = if let Some(why) = blocked.filter(|w| !w.path.is_empty()) {
                format!(
                    "`{}` does not apply to `{slt}`: its `{}` is a `{}`, which has no {property}",
                    op.symbol(),
                    why.path.join("."),
                    self.show(why.ty)
                )
            } else if lt == rt {
                format!("`{}` does not apply to `{slt}`", op.symbol())
            } else {
                format!("`{}` cannot {verb} `{slt}` with `{srt}`", op.symbol())
            };
            let mut refusal = Diagnostic::error(codes::INVALID_OPERANDS, op_span, detail)
                .secondary(lhs_span, format!("this is `{slt}`"))
                .secondary(rhs_span, format!("this is `{srt}`"));
            if let Some(p) = unrequired {
                refusal = refusal.help(format!(
                    "`{p}` is not known to have equality, because its declaration does not \
                     require it; a definition that compares one declares `{p}: {EQUALITY}`"
                ));
            }
            self.errors.push(refusal);

            // The operands were wrong, so this expression has no honest type. Reporting
            // the operator's usual one produces a second error about a mismatch the
            // author never wrote — two diagnostics for one mistake, and the second is
            // fiction. Adopt what the context wanted instead.
            return expected.unwrap_or(if op.is_comparison() || op.is_logical() {
                Type::Bool
            } else if lt.num().is_some() {
                lt
            } else {
                Type::Int
            });
        }

        if op.is_comparison() || op.is_logical() {
            Type::Bool
        } else {
            lt
        }
    }

    /// A constant named as a value (Gate 2): its declared type, its value recorded for lowering.
    #[inline(never)]
    pub(super) fn const_value(&mut self, name: &str, at: Span) -> Type {
        let Some((ty, value)) = self.types.consts.values.get(name).cloned() else {
            return Type::Int;
        };
        self.res.record_const_use(at, value);
        if let Some(decl) = self.types.consts.decls.get(name) {
            self.res.record_const_ref(at, *decl);
        }
        ty
    }

    /// The element type an expected array type gives, if one is expected.
    fn expected_element(&self, expected: Option<Type>) -> Option<Type> {
        match expected {
            Some(Type::Array(a)) => self.res.types().array_info(a).map(|(e, _)| e),
            _ => None,
        }
    }

    /// Refuse an array type written or made in a body, where every definition is known.
    fn validate_array(&mut self, t: Type, at: Span) {
        if let Some(d) = array_refusal(self.res, t, at) {
            self.errors.push(d);
        }
    }

    /// `[a, b, c]` (Gate 2): every element of one type, the first's or the one expected.
    #[inline(never)]
    pub(super) fn check_array_literal(
        &mut self,
        elems: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        // `[a, b, c]` where a `Vec[T]`, an `Ints` or a `Strs` is wanted (R1, 5b.16): that sequence,
        // its elements in order — `[]` an empty one.
        if let Some(seq) = expected.filter(|t| matches!(t, Type::Vec(_) | Type::Ints | Type::Strs))
        {
            let elem = match seq {
                Type::Ints => Type::Int,
                Type::Strs => Type::Str,
                t => self
                    .res
                    .types()
                    .type_args(t)
                    .first()
                    .copied()
                    .unwrap_or(Type::Int),
            };
            for e in elems {
                let t = self.value_type(e, Some(elem));
                if !self.assignable(t, elem) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        e.span(),
                        format!(
                            "this `{}`'s elements are `{}`, and this is `{}`",
                            self.show(seq),
                            self.show(elem),
                            self.show(t)
                        ),
                    ));
                }
            }
            self.res.record_sequence_literal(span, seq);
            return seq;
        }
        let want = self.expected_element(expected);
        // `[a, b, c]` anywhere a fixed array is not expected (R1.5, D6) is a list — a `Vec` of
        // its first element's type — as it is where one is wanted, above; a fixed array is
        // written where its type says `[T; n]`.
        if want.is_none() {
            let Some(first) = elems.first() else {
                self.errors.push(
                    Diagnostic::error(
                        codes::ARRAY_TOO_LARGE,
                        span,
                        "an empty list needs its element type written",
                    )
                    .help("write the type where it is bound: `let v: List[Int] = []`"),
                );
                return expected.unwrap_or(Type::Int);
            };
            let elem = self.value_type(first, None);
            for e in elems.iter().skip(1) {
                let t = self.value_type(e, Some(elem));
                if !self.assignable(t, elem) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        e.span(),
                        format!(
                            "this list's elements are `{}`, and this is `{}`",
                            self.show(elem),
                            self.show(t)
                        ),
                    ));
                }
            }
            let seq = self.res.types_mut().vec_of(elem);
            self.res.record_sequence_literal(span, seq);
            return seq;
        }
        let Some(first) = elems.first() else {
            self.errors.push(
                Diagnostic::error(
                    codes::ARRAY_TOO_LARGE,
                    span,
                    "an array has at least one element",
                )
                .help("an empty collection is a `Vec`: `vec_new[Int]()`"),
            );
            return expected.unwrap_or(Type::Int);
        };
        let elem = self.value_type(first, want);
        for e in elems.iter().skip(1) {
            let t = self.value_type(e, Some(elem));
            if t != elem {
                self.errors.push(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    e.span(),
                    format!(
                        "this array's elements are `{}`, and this is `{}`",
                        self.show(elem),
                        self.show(t)
                    ),
                ));
            }
        }
        let n = u32::try_from(elems.len()).unwrap_or(u32::MAX);
        let t = self.res.types_mut().array_of(elem, n);
        self.validate_array(t, span);
        t
    }

    /// `[value; count]` (Gate 2): `count` copies, `count` a literal or a constant.
    #[inline(never)]
    pub(super) fn check_array_repeat(
        &mut self,
        value: &Expr,
        count: &Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let want = self.expected_element(expected);
        let elem = self.value_type(value, want);
        let n = match count {
            Expr::Var { name, span } if self.types.consts.values.contains_key(name) => {
                self.const_value(name, *span);
                array_length(&self.types, count, &mut self.errors)
            }
            _ => array_length(&self.types, count, &mut self.errors),
        };
        let t = self.res.types_mut().array_of(elem, n.unwrap_or(1));
        if n.is_some() {
            self.validate_array(t, span);
        }
        t
    }

    /// An index's type, which is an `Int`, and a constant one's range (Gate 2).
    pub(super) fn check_index_value(&mut self, index: &Expr, len: Option<u32>) {
        let it = self.value_type(index, Some(Type::Int));
        if it != Type::Int {
            self.errors.push(Diagnostic::error(
                codes::TYPE_MISMATCH,
                index.span(),
                format!("an index is an `Int`, and this is `{}`", self.show(it)),
            ));
            return;
        }
        let constant = match index {
            Expr::Int { value, .. } => Some(i128::from(*value)),
            Expr::Neg { operand, .. } => match operand.as_ref() {
                Expr::Int { value, .. } => Some(-i128::from(*value)),
                _ => None,
            },
            _ => None,
        };
        if let (Some(i), Some(n)) = (constant, len) {
            if i < 0 || i >= i128::from(n) {
                self.errors.push(Diagnostic::error(
                    codes::ARRAY_INDEX,
                    index.span(),
                    format!("index {i} is outside an array of {n} elements"),
                ));
            }
        }
    }

    /// `base[index]` (Gate 2): an element of an array.
    #[inline(never)]
    pub(super) fn check_index(
        &mut self,
        base: &Expr,
        index: &Expr,
        bracket: Span,
        expected: Option<Type>,
    ) -> Type {
        let before = self.errors.len();
        let bt = self.value_type(base, None);
        let info = match bt {
            Type::Array(a) => self.res.types().array_info(a),
            _ => None,
        };
        // `xs[i]` on a sequence (R1, 5b.15): `vec_get(xs, i)`, `ints_get`, `strs_get` or
        // `bytes_get`, by name as a method is — unless the base was refused already.
        if info.is_none() && self.errors.len() == before {
            if let Some(t) = self.element_by_name(
                bt,
                "get",
                bracket,
                vec![base.clone(), index.clone()],
                vec![(base.span(), bt)],
                expected,
            ) {
                return t;
            }
        }
        self.check_index_value(index, info.map(|(_, n)| n));
        match info {
            Some((elem, _)) => elem,
            None => {
                let mut d = Diagnostic::error(
                    codes::ARRAY_INDEX,
                    bracket,
                    format!("`[…]` indexes an array, and this is `{}`", self.show(bt)),
                );
                if bt == Type::Bytes {
                    d = d.help("a `Bytes`'s byte is read with `bytes_get(b, i)`");
                } else if bt.is_sequence() {
                    d = d.help("a `Vec`'s element is read with `vec_get(xs, i)`");
                }
                self.errors.push(d);
                expected.unwrap_or(Type::Int)
            }
        }
    }

    /// A float literal's type (Gate 2): `Float32` where one is expected, `Float64` everywhere else.
    #[inline(never)]
    pub(super) fn float_literal(&mut self, span: Span, expected: Option<Type>) -> Type {
        if expected == Some(Type::Num(NumKind::F32)) {
            self.res
                .record_numeric(span, NumericFact::Literal(Type::Num(NumKind::F32)));
            Type::Num(NumKind::F32)
        } else {
            Type::Num(NumKind::F64)
        }
    }

    /// `-operand` (Gate 2 widened it to every numeric type). Its own function, out of
    /// [`Self::check_expr`]: that one recurses once per nesting level, and its frame is what
    /// bounds how deep a program may nest before the checker's stack runs out.
    #[inline(never)]
    pub(super) fn check_negation(
        &mut self,
        operand: &Expr,
        op_span: Span,
        expected: Option<Type>,
    ) -> Type {
        // `-128` written where an `Int8` is expected is one literal, judged whole.
        if let Expr::Int { value, span } = operand {
            if let Some(k) = expected.and_then(Type::num).filter(|k| k.is_integer()) {
                if k != NumKind::I64 {
                    return self.integer_literal((-i128::from(*value)).into(), *span, expected);
                }
            }
        }
        // And past `Int`'s digits, for an `Int128` (R1b, L13b).
        if let Expr::WideInt { value, span } = operand {
            if let Some(k) = expected.and_then(Type::num).filter(|k| k.is_integer()) {
                if k != NumKind::I64 {
                    let v = nazm_sema::numeric::Wide::from_u128(*value).negate();
                    return self.integer_literal(v, *span, expected);
                }
            }
        }
        let hint = expected.filter(|t| t.num().is_some()).or(Some(Type::Int));
        let t = self.value_type(operand, hint);
        if t.num().is_none() {
            self.errors.push(Diagnostic::error(
                codes::INVALID_OPERANDS,
                op_span,
                format!("`-` applies to a number, but this is `{}`", self.show(t)),
            ));
            return Type::Int;
        }
        t
    }

    /// `~operand` (Gate 2), out of [`Self::check_expr`] for the reason [`Self::check_negation`] is.
    #[inline(never)]
    pub(super) fn check_complement(
        &mut self,
        operand: &Expr,
        op_span: Span,
        expected: Option<Type>,
    ) -> Type {
        let hint = expected.filter(|t| t.is_integer()).or(Some(Type::Int));
        let t = self.value_type(operand, hint);
        if !t.is_integer() {
            self.errors.push(Diagnostic::error(
                codes::INVALID_OPERANDS,
                op_span,
                format!("`~` applies to an integer, but this is `{}`", self.show(t)),
            ));
            return Type::Int;
        }
        t
    }

    /// A binary operator's operands and result, out of [`Self::check_expr`] for the reason
    /// [`Self::check_negation`] is. A literal takes the type the other operand has (Gate 2): `x + 1`
    /// and `1 + x` with `x: UInt8` are both `UInt8` arithmetic. A shift's amount is an `Int`.
    #[inline(never)]
    pub(super) fn check_binary_expr(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        op_span: Span,
        expected: Option<Type>,
    ) -> Type {
        let shift = matches!(op, BinOp::Shl | BinOp::Shr);
        let before = self.errors.len();
        let hint = if op.is_comparison() || op.is_logical() {
            None
        } else {
            expected.filter(|t| t.num().is_some())
        };
        let (lt, rt) = if shift {
            let lt = self.value_type(lhs, hint.filter(|t| t.is_integer()));
            let rt = self.value_type(rhs, Some(Type::Int));
            (lt, rt)
        } else {
            // A literal takes the type of the operand beside it, when that is a number or —
            // for a string literal — a `Text` (Gate 2); so the literal is checked second.
            let takes = |t: &Type| t.num().is_some() || *t == Type::Text;
            let string = |e: &Expr| matches!(e, Expr::Str { .. });
            let literal_first = (is_number_literal(lhs) && !is_number_literal(rhs))
                || (string(lhs) && !string(rhs));
            if literal_first {
                let rt = self.value_type(rhs, hint);
                let lt = self.value_type(lhs, Some(rt).filter(takes).or(hint));
                (lt, rt)
            } else {
                let lt = self.value_type(lhs, hint);
                let rt = self.value_type(rhs, Some(lt).filter(takes).or(hint));
                (lt, rt)
            }
        };
        if shift {
            self.refuse_constant_shift(lt, rhs);
        }
        // `s + t` on two `Str`s (R1, 5b.6): `str_concat(s, t)`, checked as that call and keyed by
        // the `+`, which Core IR lowering reads as the call — unless an operand was refused.
        if op == BinOp::Add && lt == Type::Str && rt == Type::Str && self.errors.len() == before {
            let call = Expr::Call {
                qualifier: None,
                callee: "str_concat".to_owned(),
                callee_span: op_span,
                type_args: None,
                args: vec![lhs.clone(), rhs.clone()],
                span: lhs.span().join(rhs.span()),
            };
            if self.env.get("str_concat").is_some() {
                self.res.record_named_method(op_span);
                // Both operands are checked already: the call reads their types.
                let t = self.check_again(&[(lhs.span(), lt), (rhs.span(), rt)], &call, expected);
                return t.unwrap_or(Type::Str);
            }
        }
        self.check_binary(
            &BinaryCheck {
                op,
                lt,
                rt,
                op_span,
                lhs_span: lhs.span(),
                rhs_span: rhs.span(),
            },
            expected,
        )
    }

    /// A numeric conversion's type, where `callee` names one and no function of that name is
    /// in scope; `None` for every other call (Gate 2).
    #[inline(never)]
    pub(super) fn maybe_conversion(
        &mut self,
        callee: &str,
        at: Span,
        written: Option<&[TypeName]>,
        args: &[Expr],
        span: Span,
    ) -> Option<Type> {
        if self.env.contains(callee) {
            return None;
        }
        let mode = match callee {
            "try_convert" => ConvertMode::Try,
            "wrap_convert" => ConvertMode::Wrap,
            "saturate_convert" => ConvertMode::Saturate,
            name if NumKind::by_name(name).is_some() || name == "Text" => ConvertMode::Exact,
            _ => return None,
        };
        Some(self.check_conversion(callee, at, written, args, span, mode))
    }

    /// An integer literal's type (Gate 2): the integer type expected, where one is, and `Int`
    /// everywhere else — refused, by name, when its value does not fit that type.
    #[inline(never)]
    pub(super) fn integer_literal(
        &mut self,
        v: nazm_sema::numeric::Wide,
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let ty = match expected.and_then(Type::num) {
            Some(k) if k.is_integer() => Type::of_num(k),
            _ => Type::Int,
        };
        let k = ty.num().unwrap_or(NumKind::I64);
        if NumVal::int(k, v).is_none() {
            let (lo, hi) = k.int_range().unwrap_or((
                nazm_sema::numeric::Wide::ZERO,
                nazm_sema::numeric::Wide::ZERO,
            ));
            self.errors.push(
                Diagnostic::error(
                    codes::INTEGER_TOO_LARGE,
                    span,
                    format!("`{v}` does not fit in `{}`", k.name()),
                )
                .help(format!("`{}` holds {lo} to {hi}", k.name())),
            );
        }
        if ty != Type::Int {
            self.res.record_numeric(span, NumericFact::Literal(ty));
        }
        ty
    }

    /// A shift by a constant amount outside `0..width` (Gate 2): it would stop the program
    /// every time it ran, so it is refused where it is written (`N0617`).
    fn refuse_constant_shift(&mut self, lt: Type, amount: &Expr) {
        let Some(k) = lt.num().filter(|k| k.is_integer()) else {
            return;
        };
        let n = match amount {
            Expr::Int { value, .. } => i128::from(*value),
            Expr::Neg { operand, .. } => match operand.as_ref() {
                Expr::Int { value, .. } => -i128::from(*value),
                _ => return,
            },
            _ => return,
        };
        if n < 0 || n >= i128::from(k.bits()) {
            self.errors.push(
                Diagnostic::error(
                    codes::SHIFT_AMOUNT_REFUSED,
                    amount.span(),
                    format!("`{}` cannot be shifted by {n}", k.name()),
                )
                .help(format!(
                    "a shift's amount is 0 to {} for a `{}`; a larger or negative one stops \
                     the program (`N0412`) rather than being masked",
                    k.bits() - 1,
                    k.name()
                )),
            );
        }
    }

    /// A numeric conversion (Gate 2): `T(x)` exactly, or `try_convert[T](x)`,
    /// `wrap_convert[T](x)`, `saturate_convert[T](x)`. `docs/internal/general-purpose.md` §1–§2.
    fn check_conversion(
        &mut self,
        name: &str,
        at: Span,
        written: Option<&[TypeName]>,
        args: &[Expr],
        span: Span,
        mode: ConvertMode,
    ) -> Type {
        let to = if mode == ConvertMode::Exact {
            if written.is_some() {
                self.errors
                    .push(not_generic(name, "a conversion, not generic", at));
            }
            if name == "Text" {
                Type::Text
            } else {
                NumKind::by_name(name).map_or(Type::Int, Type::of_num)
            }
        } else {
            match written {
                Some([one]) => {
                    let t = self.resolve_written_type(one);
                    // `Text` (Gate 2): `Text(s)` and `try_convert[Text](s)`, nothing else.
                    let text = t == Type::Text && mode == ConvertMode::Try;
                    if t.num().is_none() && !text {
                        self.errors.push(Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            at,
                            format!(
                                "`{name}` converts to a numeric type, and `{}` is not one",
                                self.show(t)
                            ),
                        ));
                        Type::Int
                    } else {
                        t
                    }
                }
                _ => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::CANNOT_INFER_TYPE_ARGUMENT,
                            at,
                            format!("`{name}` needs the type it converts to, written"),
                        )
                        .help(format!("write it as `{name}[UInt8](x)`")),
                    );
                    Type::Int
                }
            }
        };
        if args.len() != 1 {
            self.errors.push(Diagnostic::error(
                codes::WRONG_ARGUMENT_COUNT,
                span,
                format!(
                    "`{name}` takes 1 argument, but {} {} given",
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            ));
        }
        // A literal converted exactly is that literal, written at the target type.
        let hint = (mode == ConvertMode::Exact && to != Type::Text).then_some(to);
        let mut from = Type::Int;
        for (i, a) in args.iter().enumerate() {
            let t = self.value_type(a, hint);
            if i == 0 {
                from = t;
            }
        }
        if to == Type::Text {
            if from != Type::Str {
                if let Some(a) = args.first() {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "`{name}` converts a `Str` to `Text`, and this is `{}`",
                            self.show(from)
                        ),
                    ));
                }
            }
        } else if from.num().is_none() {
            if let Some(a) = args.first() {
                self.errors.push(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    a.span(),
                    format!(
                        "`{name}` converts a number, but this is `{}`",
                        self.show(from)
                    ),
                ));
            }
        } else if mode == ConvertMode::Wrap && !(from.is_integer() && to.is_integer()) {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    at,
                    format!(
                        "`wrap_convert` is between integer types, and this converts `{}` to `{}`",
                        self.show(from),
                        self.show(to)
                    ),
                )
                .help("a float converts by `T(x)`, `try_convert` or `saturate_convert`"),
            );
        }
        let result = if mode == ConvertMode::Try {
            match self.option {
                Some(o) => self.res.types_mut().apply_enum(o, vec![to]),
                None => to,
            }
        } else {
            to
        };
        self.res.record_numeric(
            at,
            NumericFact::Convert {
                to,
                from,
                mode,
                result,
            },
        );
        result
    }
}

/// The zero a compound update of a map's absent entry starts from (R1, 5b.7): `0` for an
/// integer, `0.0` for a float, `""` for a `Str`, written at `span`; `None` for any other type,
/// whose entry must be there.
pub(crate) fn zero_of(ty: Type, span: Span) -> Option<Expr> {
    match ty {
        Type::Str => Some(Expr::Str {
            value: String::new(),
            span,
        }),
        _ => match ty.num()? {
            nazm_sema::types::NumKind::F32 | nazm_sema::types::NumKind::F64 => Some(Expr::Float {
                text: "0.0".to_owned(),
                span,
            }),
            _ => Some(Expr::Int { value: 0, span }),
        },
    }
}

impl Checker<'_> {
    /// `xs[i]` and `xs[i] = v` on a `Vec`, `Ints`, `Strs` or `Bytes` (R1, 5b.15): the call
    /// `<prefix>_<op>(args…)`, checked as that call is and keyed by the brackets' span, which
    /// Core IR lowering reads as the call. On any other type — a `HashMap`, an `OrdMap` — the
    /// prefix is the type's name in lower case and a read is `<prefix>_at` (R1, 5b.7), or, as
    /// `m[k] += n` reads it, `<prefix>_at_or` with the value type's zero. `None` where no such
    /// function is in scope.
    pub(super) fn element_by_name(
        &mut self,
        of: Type,
        op: &str,
        bracket: Span,
        mut args: Vec<Expr>,
        checked: Vec<(Span, Type)>,
        expected: Option<Type>,
    ) -> Option<Type> {
        let name = match of {
            Type::Ints => format!("ints_{op}"),
            Type::Strs => format!("strs_{op}"),
            Type::Bytes => format!("bytes_{op}"),
            Type::Vec(_) => format!("vec_{op}"),
            _ => {
                let shown = self.show(of);
                let prefix = shown.split('[').next().unwrap_or("").to_ascii_lowercase();
                if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return None;
                }
                let value = self.res.types().type_args(of).last().copied();
                let compound = bracket.start == bracket.end;
                match (op, value.and_then(|v| zero_of(v, bracket).map(|z| (v, z)))) {
                    ("get", Some((v, zero)))
                        if compound && self.env.get(&format!("{prefix}_at_or")).is_some() =>
                    {
                        args.push(zero);
                        self.res.record_map_fallback(bracket, v);
                        format!("{prefix}_at_or")
                    }
                    ("get", _) => format!("{prefix}_at"),
                    _ => format!("{prefix}_{op}"),
                }
            }
        };
        self.env.get(&name)?;
        let call = Expr::Call {
            qualifier: None,
            callee: name,
            callee_span: bracket,
            type_args: None,
            args,
            span: bracket,
        };
        self.res.record_named_method(bracket);
        // The sequence is checked already where it is read (`check_index`): the call reads its
        // type.
        let t = self.check_again(&checked, &call, expected);
        Some(t.unwrap_or(expected.unwrap_or(Type::Int)))
    }

    /// Check `call`, a desugaring's, whose arguments include `checked` — expressions checked
    /// already, with their types — without checking those again ([`Checker::checked`]).
    /// `None` when the call completes with no value.
    pub(super) fn check_again(
        &mut self,
        checked: &[(Span, Type)],
        call: &Expr,
        expected: Option<Type>,
    ) -> Option<Type> {
        let mark = self.checked.len();
        self.checked.extend_from_slice(checked);
        let c = self.check_expr(call, expected);
        self.checked.truncate(mark);
        match c {
            Completion::Value(t) => Some(t),
            Completion::Unit | Completion::Diverges => None,
        }
    }

    /// The type of the expression at `span` when it is one checked already and not yet taken.
    pub(super) fn take_checked(&mut self, span: Span) -> Option<Type> {
        if self.checked.is_empty() {
            return None;
        }
        let i = self.checked.iter().rposition(|(s, _)| *s == span)?;
        Some(self.checked.remove(i).1)
    }
}
