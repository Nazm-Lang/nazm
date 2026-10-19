//! Everything one native object depends on, as one number.
//!
//! # The question an `ObjectKey` answers
//!
//! > Has this exact LLVM input already been assembled by this exact backend under this
//! > exact configuration?
//!
//! Not *has this module changed* and not *has its dependency's interface moved*. Those
//! are [`CheckKey`](crate::CheckKey)'s questions, and they are about a different stage of
//! a different pipeline. This one is about the transformation `nazm build` actually pays
//! for:
//!
//! ```text
//! LLVM unit bytes + codegen configuration + backend tool  →  object
//! ```
//!
//! # Why the key is the backend's input rather than a summary of the front end's
//!
//! The obvious design is to rebuild the dependency graph in another key: this module's
//! source, plus each dependency's exported signatures, plus the emitter's version, plus
//! the target. Every one of those is an *approximation* of what `clang -c` reads, and
//! every approximation has the same failure — something reaches the object that nobody
//! remembered to add.
//!
//! The emitted IR is not an approximation. It **is** the input. N5 established that a
//! caller's unit contains a dependency only through a `declare` and a `call`, so:
//!
//! | Edit | Emitted IR of the caller | This key |
//! | --- | --- | --- |
//! | a dependency's body | unchanged | unchanged, and the object is reused |
//! | an export the caller does not call | unchanged | unchanged |
//! | the signature of something it does call | the `declare` moves | moves |
//! | the emitter, with the same output | unchanged | unchanged |
//! | the emitter, with different output | changed | moves |
//! | a comment, where positions do not move | unchanged | unchanged |
//!
//! No rule in that table had to be written down. It follows from hashing the bytes that
//! are handed over, and it is the reason this key is smaller than a correct approximation
//! would have to be.
//!
//! # What is deliberately not in it
//!
//! | Left out | Why it cannot affect the object |
//! | --- | --- |
//! | [`ModuleKey`](nazm_sema::key::ModuleKey) | two units with identical IR are identical inputs to `clang`. Which module a definition belongs to already reaches the IR, as its native symbol — adding the key again would only refuse valid reuse |
//! | a dependency's `InterfaceHash` | the parts of an interface an object depends on are in its own `declare` lines; the rest provably is not, and including it would invalidate a caller when an export it never mentions changes |
//! | the module's source fingerprint | an edit that does not reach the IR does not reach the object |
//! | the output path, the input path, the working directory | measured: the object bytes are identical across all three |
//! | the linker, its version and its options | a linker does not produce an object. That belongs to a `LinkKey`, which does not exist |
//! | the cache's own location | a store holds entries; it does not name them |
//!
//! # Three keys, still apart
//!
//! [`CheckKey`](crate::CheckKey) may hit while this misses — a target changed, an
//! optimisation level changed — and this may hit while that misses, which is the ordinary
//! case for a comment edit. Neither implies the other, and nothing in this file reads
//! anything semantic.

use std::fmt;

use crate::digest::{self, Framed};

/// The exact bytes handed to the object compiler.
///
/// Of the **file's contents**, not of the tree it was emitted from: a fingerprint of a
/// `Unit` would be a fingerprint of something `clang` never sees, and the two could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LlvmInput([u8; 32]);

impl LlvmInput {
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Framed::new("nazm.llvm-input/1").bytes(bytes).finish())
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }
}

/// Which backend produced, or is asking for, an object.
///
/// # What it is derived from
///
/// The version report the driver prints about **itself**, with the line naming where it
/// is installed removed — a path is not an identity, and the same compiler reached
/// through two paths is one compiler.
///
/// # What it cannot distinguish
///
/// Two builds of a compiler that report the same version and differ in behaviour. That is
/// a real limit and it is stated rather than papered over: replacing a toolchain in place
/// without changing what it calls itself is the case where `.nazm/native` should be
/// deleted. The alternative — hashing the driver's own bytes — was measured and rejected
/// in `docs/internal/architecture.md` §7.6: on the machine this was built on it reads a 124 MB
/// executable behind a 200 KB shim, so it would cost more per build than the objects it
/// saves and would identify the shim rather than the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BackendIdentity([u8; 32]);

impl BackendIdentity {
    /// The identity of a driver called `driver` that reports `report` about itself.
    #[must_use]
    pub fn of(driver: &str, report: &str) -> Self {
        Self(
            Framed::new("nazm.backend-tool/1")
                .field(driver)
                .field(report)
                .finish(),
        )
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }
}

/// How the backend was told to compile, as the backend itself resolved it.
///
/// # Resolved, not requested
///
/// `nazm build` passes three arguments. The compiler turns them into something like two
/// hundred, and the ones that decide the object are mostly not among the three: the
/// target triple with its deployment version, the CPU and its thirty feature flags, the
/// relocation model, the ABI. Hashing what was *asked for* would leave every one of those
/// outside the key, and each of them changes the object.
///
/// So this is a digest of the arguments the driver says it will actually run, with only
/// the ones naming **this invocation's own files** removed — the input, the output, and
/// the directories recorded for debug and coverage information. Each of those was
/// measured not to change a single byte of the object; `docs/internal/architecture.md` §7.6 has
/// the measurements. Everything else is kept, including arguments this compiler has never
/// heard of, because the safe direction is to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodegenConfig([u8; 32]);

impl CodegenConfig {
    /// The configuration these resolved arguments describe.
    #[must_use]
    pub fn of(arguments: &[String]) -> Self {
        let mut f = Framed::new("nazm.codegen-config/1");
        f.number(u32::try_from(arguments.len()).unwrap_or(u32::MAX));
        for a in arguments {
            f.field(a);
        }
        Self(f.finish())
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }
}

/// The bytes of a produced object, so that a damaged one is a miss rather than a mystery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectFingerprint([u8; 32]);

impl ObjectFingerprint {
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Framed::new("nazm.object-bytes/1").bytes(bytes).finish())
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }
}

/// The inputs to compiling one LLVM unit into one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey([u8; 32]);

impl ObjectKey {
    #[must_use]
    pub fn of(input: LlvmInput, tool: BackendIdentity, config: CodegenConfig) -> Self {
        Self(
            Framed::new("nazm.object-key/1")
                .field(&input.to_string())
                .field(&tool.to_string())
                .field(&config.to_string())
                .finish(),
        )
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

macro_rules! hex {
    ($($t:ty),*) => {$(
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                digest::write_hex(f, &self.0)
            }
        }
    )*};
}

/// An accelerator kernel's identity (N67, `nazm.kernel/1`): its device source, the device and
/// driver that compile it, and the host program that runs it — what a cache of the driver's
/// binaries would be keyed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KernelKey([u8; 32]);

impl KernelKey {
    #[must_use]
    pub fn of(source: &str, device: &str, driver: &str, host: &str) -> Self {
        Self(
            Framed::new("nazm.kernel/1")
                .bytes(source.as_bytes())
                .bytes(device.as_bytes())
                .bytes(driver.as_bytes())
                .bytes(host.as_bytes())
                .finish(),
        )
    }
}

hex!(
    KernelKey,
    LlvmInput,
    BackendIdentity,
    CodegenConfig,
    ObjectFingerprint,
    ObjectKey
);

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_kernel_s_identity_is_every_part_of_what_runs_it() {
        let k = |s, d, v, h| KernelKey::of(s, d, v, h);
        let base = k("src", "Apple M1 Pro", "1.2 1.0", "host");
        assert_eq!(base, k("src", "Apple M1 Pro", "1.2 1.0", "host"));
        assert_ne!(base, k("src2", "Apple M1 Pro", "1.2 1.0", "host"));
        assert_ne!(base, k("src", "Another GPU", "1.2 1.0", "host"));
        assert_ne!(base, k("src", "Apple M1 Pro", "1.2 2.0", "host"));
        assert_ne!(base, k("src", "Apple M1 Pro", "1.2 1.0", "host2"));
        // Framed, so a boundary cannot move between parts.
        assert_ne!(k("ab", "c", "d", "e"), k("a", "bc", "d", "e"));
        assert_eq!(base.to_string().len(), 64);
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn key(ir: &[u8], report: &str, argv: &[&str]) -> ObjectKey {
        ObjectKey::of(
            LlvmInput::of(ir),
            BackendIdentity::of("driver", report),
            CodegenConfig::of(&args(argv)),
        )
    }

    #[test]
    fn the_same_inputs_give_the_same_key() {
        assert_eq!(
            key(b"define i64 @f()", "v1", &["-O0"]),
            key(b"define i64 @f()", "v1", &["-O0"])
        );
    }

    #[test]
    fn every_input_moves_it() {
        // One assertion per field, because a key missing a field is exactly a wrong hit.
        let base = key(b"ir", "v1", &["-O0"]);
        assert_ne!(base, key(b"ir2", "v1", &["-O0"]));
        assert_ne!(base, key(b"ir", "v2", &["-O0"]));
        assert_ne!(base, key(b"ir", "v1", &["-O2"]));
    }

    #[test]
    fn an_optimisation_level_and_a_target_cannot_collide() {
        // `docs/internal/architecture.md` §7.6: the target is not a flag this compiler passes, it
        // is one the driver resolves — so it arrives here as a resolved argument, and the
        // key separates two targets exactly as it separates two optimisation levels.
        let o0 = key(b"ir", "v1", &["-triple", "arm64-apple-macosx27.0.0", "-O0"]);
        let o2 = key(b"ir", "v1", &["-triple", "arm64-apple-macosx27.0.0", "-O2"]);
        let other = key(b"ir", "v1", &["-triple", "x86_64-unknown-linux-gnu", "-O0"]);
        assert_ne!(o0, o2);
        assert_ne!(o0, other);
        assert_ne!(o2, other);
    }

    #[test]
    fn a_cpu_feature_moves_the_configuration() {
        // The reason the configuration is what the driver resolved rather than what this
        // compiler asked for: `-target-cpu` and its feature flags are never passed by
        // `nazm build` and always decide the object.
        let m1 = CodegenConfig::of(&args(&["-target-cpu", "apple-m1"]));
        let m4 = CodegenConfig::of(&args(&["-target-cpu", "apple-m4"]));
        assert_ne!(m1, m4);
    }

    #[test]
    fn an_argument_boundary_cannot_be_moved() {
        // Two argument vectors that concatenate to one string are two configurations.
        assert_ne!(
            CodegenConfig::of(&args(&["-O", "2"])),
            CodegenConfig::of(&args(&["-O2"]))
        );
        assert_ne!(
            CodegenConfig::of(&args(&["-a"])),
            CodegenConfig::of(&args(&["-a", ""]))
        );
    }

    #[test]
    fn the_component_digests_are_not_interchangeable() {
        // Each carries its own domain, so an entry cannot pass one field's value off as
        // another's even though both are thirty-two bytes.
        let a = LlvmInput::of(b"x").to_string();
        let b = ObjectFingerprint::of(b"x").to_string();
        assert_ne!(a, b);
        assert_ne!(BackendIdentity::of("driver", "x").to_string(), a);
    }

    #[test]
    fn each_kind_round_trips_through_its_written_form() {
        let k = key(b"ir", "v1", &["-O0"]);
        assert_eq!(ObjectKey::parse(&k.to_string()), Some(k));
        assert!(ObjectKey::parse("short").is_none());
        let i = LlvmInput::of(b"ir");
        assert_eq!(LlvmInput::parse(&i.to_string()), Some(i));
        let t = BackendIdentity::of("driver", "v1");
        assert_eq!(BackendIdentity::parse(&t.to_string()), Some(t));
        let c = CodegenConfig::of(&args(&["-O0"]));
        assert_eq!(CodegenConfig::parse(&c.to_string()), Some(c));
        let o = ObjectFingerprint::of(b"o");
        assert_eq!(ObjectFingerprint::parse(&o.to_string()), Some(o));
    }
}
