# The formal core, v1

`nazm.formal-core/1` — N61, 2026-10-02. This document is the semantics; `crates/nazm-formal` is it
transcribed rule by rule (each rule's name is a comment on its arm), and
`crates/nazm-formal/tests/exhaustive.rs` checks the properties below over **every** program of the
core up to a stated size. It is a bounded, machine-checked model, not a proof: nothing outside the
bound is established, and no proof assistant was used.

## Syntax

```text
e ::= n                      integer literal, n ∈ [-2^63, 2^63)
    | true | false
    | x                      a bound name
    | e + e | e - e | e * e | e / e | e % e
    | e < e | e == e
    | !e | e && e
    | if e { e } else { e }
    | let x = e; e
```

A core program is one expression, the body of `fn main() -> T`. It is printed to Nazm source by
`nazm_formal::to_source` and so is also a Nazm program — that is the correspondence.

## Types

`τ ::= Int | Bool`. Γ maps names to types.

| rule | |
| --- | --- |
| T-Int | Γ ⊢ n : Int |
| T-Bool | Γ ⊢ true : Bool, Γ ⊢ false : Bool |
| T-Var | Γ(x) = τ ⟹ Γ ⊢ x : τ |
| T-Arith | Γ ⊢ a : Int, Γ ⊢ b : Int ⟹ Γ ⊢ a ⊕ b : Int, ⊕ ∈ {+,-,*,/,%} |
| T-Lt | Γ ⊢ a : Int, Γ ⊢ b : Int ⟹ Γ ⊢ a < b : Bool |
| T-Eq | Γ ⊢ a : τ, Γ ⊢ b : τ ⟹ Γ ⊢ a == b : Bool |
| T-Not | Γ ⊢ a : Bool ⟹ Γ ⊢ !a : Bool |
| T-And | Γ ⊢ a : Bool, Γ ⊢ b : Bool ⟹ Γ ⊢ a && b : Bool |
| T-If | Γ ⊢ c : Bool, Γ ⊢ a : τ, Γ ⊢ b : τ ⟹ Γ ⊢ if c { a } else { b } : τ |
| T-Let | Γ ⊢ a : σ, Γ[x ↦ σ] ⊢ b : τ ⟹ Γ ⊢ let x = a; b : τ |

## Evaluation (big step)

Outcomes `r ::= v | trap(c)`, values `v ::= n | true | false`, `c ∈ {N0400, N0401}`. ρ maps names to
values. Evaluation is left to right; a trap propagates.

| rule | |
| --- | --- |
| E-Lit | ρ ⊢ n ⇓ n, ρ ⊢ true ⇓ true, ρ ⊢ false ⇓ false |
| E-Var | ρ ⊢ x ⇓ ρ(x) |
| E-Arith | ρ ⊢ a ⇓ m, ρ ⊢ b ⇓ k ⟹ ρ ⊢ a ⊕ b ⇓ m ⊕ k when the mathematical result is in range, else trap(N0400) |
| E-Div | `/` and `%` truncate toward zero; k = 0 is trap(N0401); `i64::MIN / -1` (and `% -1`) is trap(N0400) |
| E-Lt, E-Eq | compare; `==` on Bool is equality of truth values |
| E-Not | ρ ⊢ a ⇓ b ⟹ ρ ⊢ !a ⇓ ¬b |
| E-And | ρ ⊢ a ⇓ false ⟹ ρ ⊢ a && b ⇓ false (b not evaluated); ρ ⊢ a ⇓ true, ρ ⊢ b ⇓ v ⟹ ρ ⊢ a && b ⇓ v |
| E-If | ρ ⊢ c ⇓ true ⟹ the then branch; false ⟹ the else branch; only one branch is evaluated |
| E-Let | ρ ⊢ a ⇓ v, ρ[x ↦ v] ⊢ b ⇓ r ⟹ ρ ⊢ let x = a; b ⇓ r |
| E-Trap | any premise ⇓ trap(c) ⟹ the conclusion ⇓ trap(c), for the first such premise in order |

## Properties checked

| | property | over |
| --- | --- | --- |
| P1 | **Soundness**: if ∅ ⊢ e : τ then evaluation terminates in a value of type τ or a trap — never stuck | every well-typed program up to the bound |
| P2 | **Determinism**: two evaluations agree | the same |
| P3 | **Checked arithmetic**: an `Int` result equals the mathematical value whenever no trap occurred | the same, against 128-bit arithmetic |
| P4 | **Correspondence**: the Nazm interpreter, run on `to_source(e)`, prints the same value or fails with the same code | the same, and a sample through the native backend |
| P5 | **Typing agrees**: Nazm's checker accepts `to_source(e)` exactly when the formal rules type `e` | every program up to the bound, typed or not |

## Not in the core

Functions, recursion, loops, mutation, strings, sequences, records, enums, effects, tasks,
channels, closures, the memory model and the backends' code generation. Each is outside the
properties above, and no claim is made about it.
