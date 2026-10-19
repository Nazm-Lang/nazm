//! C layout: foreign structs, layout-annotated records, and arrays' lengths and element types.

use super::written::{closing_field, hop_name, is_declaration, node_span, resolve_written};
use super::*;

/// The rules an `extern "C" struct` meets (N85, §7.86): the C ABI; no type parameters, since
/// C has one layout per struct; and for a C-layout record, fields of `Int`, `Bool`, an opaque
/// handle or another C-layout record — each of which has the layout C gives `int64_t`, `bool`,
/// a pointer and the nested struct.
pub(super) fn foreign_struct(
    r: &nazm_syntax::Record,
    f: &nazm_syntax::ForeignStruct,
    fields: &[FieldDef],
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    if f.abi != "C" {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_ABI,
                f.abi_span,
                format!("`\"{}\"` is not an ABI this compiler lays out", f.abi),
            )
            .help("the one foreign ABI is `\"C\"`"),
        );
    }
    if let (Some(first), Some(last)) = (r.type_params.first(), r.type_params.last()) {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SHAPE,
                first.span.join(last.span),
                "a C struct cannot be generic",
            )
            .help("C has one layout per struct; declare one per type"),
        );
    }
    for field in fields {
        if !c_layout_type(res, field.ty) {
            errors.push(
                Diagnostic::error(
                    codes::FOREIGN_TYPE,
                    field.ty_span,
                    format!(
                        "a C struct cannot have a field of this type (`{}`)",
                        field.name
                    ),
                )
                .primary_label("no C layout")
                .help(
                    "a C struct's fields are `Int` (`int64_t`), `Bool` (`bool`), an opaque \
                     handle (a pointer) or another C struct; a `Str`, a sequence, an enum or \
                     an ordinary record has no layout C shares",
                ),
            );
        }
    }
}

/// `N0611`: an opaque handle built or taken apart (N85).
pub(super) fn opaque_refused(at: Span, ty: &str, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::OPAQUE_HANDLE,
        at,
        format!("`{ty}` is an opaque C handle, which cannot be {what}"),
    )
    .help(
        "a handle declared `extern \"C\" struct Name;` is a C pointer Nazm never reads: only a \
         foreign function returns one, and only C reads what it points to",
    )
}

/// Whether a value of `t` has a C layout (N85): `Int`, `Bool`, or a record declared
/// `extern "C"` — a C-layout record or an opaque handle.
fn c_layout_type(res: &Resolution, t: Type) -> bool {
    match t {
        Type::Int | Type::Bool => true,
        Type::Record(id) => res.records().get(id).is_some_and(|d| d.repr != CRepr::Nazm),
        _ => false,
    }
}

/// One declared field list, resolved. Shared by a record's fields and a variant's payload,
/// because they are the same thing: `docs/user/spec.md` makes a payload record-shaped so that a
/// future tuple variant is a new decision rather than a drift.
pub(super) fn resolve_fields(
    types: &Types,
    res: &mut Resolution,
    owner: &str,
    written: &[nazm_syntax::Field],
    scope: &[(String, Type)],
    errors: &mut Vec<Diagnostic>,
) -> Vec<FieldDef> {
    let mut fields: Vec<FieldDef> = Vec::new();
    for f in written {
        if let Some(first) = fields.iter().find(|e| e.name == f.name) {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FIELD,
                    f.name_span,
                    format!("`{owner}` has two fields called `{}`", f.name),
                )
                .secondary(first.name_span, "first declared here")
                .help("a field is identified by its name, so two cannot share one"),
            );
            continue;
        }
        let ty = resolve_written(types, res, &f.ty, scope, None, errors);
        fields.push(FieldDef {
            name: f.name.clone(),
            ty,
            name_span: f.name_span,
            ty_span: f.ty.span,
        });
    }
    fields
}

/// Stage 0c for the whole compilation: refuse types that cannot be laid out.
///
/// A record contains its fields by value and an enum contains its active payload by value,
/// so a chain of either that returns to where it started describes a value containing
/// itself. `docs/user/spec.md` refuses it as an **impossible layout** rather than as a safety
/// problem, and states the rule over *inline containment* so that a future indirection can
/// break a cycle without the rule changing.
///
/// One graph over both kinds since N10: a cycle that runs through an enum is the same
/// impossibility as one that does not, and a walk that knew only about records would have
/// run forever on `struct A { b: B } enum B { V(a: A) }`.
///
/// Reported before anything lowers, so LLVM never sees a recursive type — a backend
/// complaining about one is a refusal that arrives after the compiler decided the program
/// was fine.
pub fn validate_type_layouts(res: &mut Resolution, errors: &mut Vec<Diagnostic>) {
    // Every array written in a field (Gate 2), once every definition it might contain is known.
    validate_new_arrays(res, errors);
    // Over **definitions**, since N11. An applied type is not a declaration and has no
    // name span of its own to point at, and whether it can contain itself is decided by
    // the definitions it applies: `nazm_sema::UserTypes::ownership_cycles` says how.
    let cycles = res.types().ownership_cycles();
    let mut reported: Vec<TypeNode> = Vec::new();
    for cycle in &cycles {
        reported.extend(cycle.through.iter().copied());
        let Some(first) = cycle.through.first().copied() else {
            continue;
        };
        let Some(start_span) = node_span(res, first) else {
            continue;
        };
        let names: Vec<String> = cycle
            .through
            .iter()
            .map(|n| res.types().show(n.ty()))
            .collect();
        let start = res.types().show(first.ty());
        let closing = cycle.hops.last().and_then(|via| closing_field(res, via));
        let d = match cycle.reach {
            nazm_sema::Reach::Inline => {
                let message = if names.len() == 2 {
                    format!("`{start}` contains itself")
                } else {
                    format!("`{start}` contains itself through {}", names.join(" → "))
                };
                Diagnostic::error(codes::RECURSIVE_RECORD, start_span, message).help(
                    "a record holds its fields by value and an enum holds its active \
                     payload by value, so this one would have no finite size. This is an \
                     impossible layout rather than a safety problem: a later type that \
                     holds its contents somewhere else would make the same shape \
                     expressible",
                )
            }
            nazm_sema::Reach::Counted => {
                let path = cycle
                    .hops
                    .iter()
                    .map(|via| hop_name(res, via))
                    .chain(std::iter::once(start.clone()))
                    .collect::<Vec<_>>()
                    .join(" → ");
                Diagnostic::error(
                    codes::OWNERSHIP_CYCLE,
                    start_span,
                    format!("`{start}` can own itself through a `Vec`: {path}"),
                )
                .help(
                    "a `Vec` holds its elements through a reference-counted handle, so this \
                     layout is finite — but a value of it could hold a `Vec` containing \
                     itself, and no count would ever reach zero. Nazm refuses such an \
                     ownership cycle rather than leak it. Store the children somewhere that \
                     does not own them: an index into a `Vec` owned outside the type",
                )
            }
        };
        errors.push(match closing {
            Some(at) => d.secondary(at, "this field closes the cycle"),
            None => d,
        });
    }

    // Depth is only meaningful once there is no cycle to walk forever, and it is asked of
    // declarations: an applied type's depth is its definition's with its arguments filled.
    let nodes: Vec<TypeNode> = (0..res.records().len())
        .map(|i| TypeNode::Record(RecordId::from_index(i)))
        .filter(|n| is_declaration(res, *n))
        .chain(
            (0..res.enums().len())
                .map(|i| TypeNode::Enum(EnumId::from_index(i)))
                .filter(|n| is_declaration(res, *n)),
        )
        .collect();
    for node in &nodes {
        if reported.contains(node) {
            continue;
        }
        if res.types().depth(node.ty()).is_none() {
            let Some(at) = node_span(res, *node) else {
                continue;
            };
            errors.push(
                Diagnostic::error(
                    codes::TYPE_NESTING_TOO_DEEP,
                    at,
                    format!("user-defined types may nest {MAX_TYPE_DEPTH} deep"),
                )
                .help(
                    "each level multiplies the copy and cleanup code generated for every \
                     value of the outer type, so the bound is on the emitted program \
                     rather than on the compiler. It is stated rather than discovered",
                ),
            );
        }
    }
}

/// Hold every array type written since the last stage to its element and size rules (Gate 2).
pub fn validate_new_arrays(res: &mut Resolution, errors: &mut Vec<Diagnostic>) {
    for (t, at) in res.take_arrays_to_validate() {
        if let Some(d) = array_refusal(res, t, at) {
            errors.push(d);
        }
    }
}

/// An array's length, written as an integer literal or a constant's name (Gate 2).
pub(super) fn array_length(types: &Types, e: &Expr, errors: &mut Vec<Diagnostic>) -> Option<u32> {
    let n = match e {
        Expr::Int { value, .. } => Some(i128::from(*value)),
        // A static is storage, not a length (Gate 3).
        Expr::Var { name, .. } if types.consts.statics.contains(name) => None,
        Expr::Var { name, .. } => types.consts.values.get(name).and_then(|(_, v)| v.as_int()),
        _ => None,
    };
    let Some(n) = n else {
        errors.push(
            Diagnostic::error(
                codes::NOT_A_CONSTANT,
                e.span(),
                "an array's length is an integer literal or an `Int` constant's name",
            )
            .help("declare one at the top of the module: `const SIZE: Int = 64;`"),
        );
        return None;
    };
    match u32::try_from(n) {
        Ok(n) if n > 0 => Some(n),
        _ => {
            errors.push(Diagnostic::error(
                codes::ARRAY_TOO_LARGE,
                e.span(),
                format!("an array has 1 to {} elements, and this is {n}", u32::MAX),
            ));
            None
        }
    }
}

/// Why an array type is refused, as a diagnostic at `at` (Gate 2); `None` when it is one the
/// language has.
pub(super) fn array_refusal(res: &Resolution, t: Type, at: Span) -> Option<Diagnostic> {
    let Type::Array(a) = t else {
        return None;
    };
    let (elem, len) = res.types().array_info(a)?;
    match res.types().array_problem(elem, len)? {
        nazm_sema::ArrayProblem::Empty => Some(Diagnostic::error(
            codes::ARRAY_TOO_LARGE,
            at,
            "an array has at least one element",
        )),
        nazm_sema::ArrayProblem::NotPlain(owner) => Some(
            Diagnostic::error(
                codes::ARRAY_ELEMENT,
                at,
                if owner == elem {
                    format!(
                        "an array holds values, and `{}` is a handle",
                        res.types().show(elem)
                    )
                } else {
                    format!(
                        "an array holds values, and `{}` holds a `{}`",
                        res.types().show(elem),
                        res.types().show(owner)
                    )
                },
            )
            .help(
                "an array's elements are numbers, `Bool`s, strings, and records, enums and \
                 arrays of them — values, copied with the strings they hold; a collection of \
                 handles is a `Vec`",
            ),
        ),
        nazm_sema::ArrayProblem::TooLarge(bytes) => Some(
            Diagnostic::error(
                codes::ARRAY_TOO_LARGE,
                at,
                format!(
                    "`{}` holds {bytes} bytes, and an array holds at most {}",
                    res.types().show(t),
                    nazm_sema::MAX_ARRAY_BYTES
                ),
            )
            .help("an array lives inline, on a task's stack; a larger buffer is a `Vec`"),
        ),
    }
}
