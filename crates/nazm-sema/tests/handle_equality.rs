//! `Ints` and `Strs` are handles — sequences by reference — and a handle has no equality (N13).
//!
//! Since R1 closed they are written `Vec[Int]` and `Vec[Str]`, and a program cannot name the older
//! types; only an optimiser pass builds them. So no program refuses them, and a program-level test
//! cannot hold the rule for them: the rule is held here, where the predicate is.

use nazm_sema::{Type, UserTypes};

#[test]
fn a_sequence_handle_has_no_equality_though_every_scalar_has() {
    let user = UserTypes::default();
    for scalar in [Type::Int, Type::Bool, Type::Str, Type::Text] {
        assert!(user.equality(scalar).is_ok(), "{scalar:?}");
    }
    for handle in [Type::Ints, Type::Strs, Type::Chan] {
        assert!(user.equality(handle).is_err(), "{handle:?}");
    }
}
