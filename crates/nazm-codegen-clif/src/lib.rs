//! The Cranelift backend: LIR to native object code, in process.
//!
//! `docs/architecture.md` §7.43 is its constitution, and §7.106 (N105) what it consumes: each
//! unit's instruction-level LIR, lowered and validated by `nazm-lir`, which has decided every
//! guard, failure text, offset, runtime entry and retain or release already. This crate
//! translates instructions into Cranelift IR and produces one object per unit with
//! `cranelift-object`; it reads no MIR. The runtime and the platform entry stay the runtime's
//! own LLVM text.
//!
//! Nothing of Cranelift's appears in this crate's public interface: a caller names a target and
//! an optimisation level and gets a [`Backend`].

mod dwarf;
mod translate;

use cranelift_codegen::settings::{self, Configurable};
use nazm_diag::{codes, Diagnostic};
use nazm_lir::backend::{units, Backend, BackendId, Code, Generated, Planned, Target};
use nazm_lir::runtime::RUNTIME_ABI_REVISION;
use nazm_lir::{Native, Needs, SourceNames, Unit};
use nazm_span::Span;

/// The exact Cranelift this backend is built with, which enters every cache key.
pub const CRANELIFT_VERSION: &str = "0.136.1";

/// This translator's revision: bumped with any change to how an instruction becomes Cranelift
/// IR, since an object is a function of the translator as well as of its LIR (N105).
/// 2 in Q1: stack probes (Q1-A1-01). 3 in Gate 3: a float converted to an 8- or 16-bit integer is
/// converted at 32 bits and clamped (3B-2). 4 in R1 (5b.8): a Nazm function's failure bit returned
/// beside its result, and the blocks only a failure reaches laid out cold. 5 after 0.10.0: x86_64
/// passes a 128-bit integer as LLVM does (`enable_llvm_abi_extensions`). 6 in R1.5 (D14): an ELF
/// object has a section per function and per datum, so `--gc-sections` strips what is unused.
pub const TRANSLATOR_REVISION: u32 = 6;

/// The Cranelift backend.
#[derive(Debug, Clone)]
pub struct Cranelift {
    target: Target,
    opt_level: u8,
    /// Whether Cranelift's IR verifier runs on every function before code generation.
    verify: bool,
    /// Whether each object carries DWARF (N91).
    debug: bool,
    /// Whether an eligible `Vec[R]` is laid out field by field (N68), as LIR decides it.
    soa: bool,
    /// Whether every object allocates through the counted heap (R1d, D5: `--heap-limit`).
    counted_heap: bool,
}

impl Cranelift {
    /// A Cranelift backend for `target` at `opt_level` (0: none; 1–3: speed).
    #[must_use]
    pub const fn new(target: Target, opt_level: u8) -> Self {
        Self {
            target,
            opt_level,
            verify: true,
            debug: false,
            soa: false,
            counted_heap: false,
        }
    }

    /// The same backend, every object allocating through the counted heap (R1d, D5): its LIR's
    /// imports of C's allocator renamed, which the object's key then covers.
    #[must_use]
    pub const fn with_counted_heap(mut self, counted: bool) -> Self {
        self.counted_heap = counted;
        self
    }

    /// The same backend, laying out every eligible `Vec[R]` field by field (N68).
    #[must_use]
    pub const fn with_soa(mut self, soa: bool) -> Self {
        self.soa = soa;
        self
    }

    /// The same backend, writing debug information into every object (N91).
    #[must_use]
    pub const fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    fn settings(&self) -> Result<settings::Flags, String> {
        let mut b = settings::builder();
        let opt = if self.opt_level == 0 { "none" } else { "speed" };
        b.set("opt_level", opt).map_err(|e| e.to_string())?;
        b.set("is_pic", "true").map_err(|e| e.to_string())?;
        // Stack probes, inline, as the LLVM backend's (Q1-A1-01): a frame larger than a task
        // stack's one guard page faults on the guard rather than skipping it.
        b.set("enable_probestack", "true")
            .map_err(|e| e.to_string())?;
        b.set("probestack_strategy", "inline")
            .map_err(|e| e.to_string())?;
        // A Nazm function returns its failure bit beside its result (R1, 5b.8): where the two
        // need more registers than the target returns in — an `Int128`'s pair and the bit on
        // x86_64 — the rest come back through memory, as Cranelift arranges for both sides.
        b.set("enable_multi_ret_implicit_sret", "true")
            .map_err(|e| e.to_string())?;
        // A 128-bit integer crosses a call as an argument or a result (R1b, L13b; the hash maps'
        // `widening_mul`): x86_64 passes it as the System V pair LLVM does only with this set.
        // Without it Cranelift refuses such a signature there; aarch64's is unaffected.
        b.set("enable_llvm_abi_extensions", "true")
            .map_err(|e| e.to_string())?;
        b.set(
            "enable_verifier",
            if self.verify { "true" } else { "false" },
        )
        .map_err(|e| e.to_string())?;
        Ok(settings::Flags::new(b))
    }

    fn isa(&self) -> Result<cranelift_codegen::isa::OwnedTargetIsa, String> {
        let triple: target_lexicon::Triple = self
            .target
            .triple
            .parse()
            .map_err(|e| format!("`{}`: {e}", self.target.triple))?;
        // The triple's baseline ISA: no feature of this host is detected, so the object is the
        // same on any machine of the target.
        let builder = cranelift_codegen::isa::lookup(triple).map_err(|e| e.to_string())?;
        builder.finish(self.settings()?).map_err(|e| e.to_string())
    }

    /// One unit's LIR, validated, on this target (N105).
    fn lir(
        &self,
        native: &Native,
        unit: &Unit,
        names: &SourceNames<'_>,
    ) -> Result<nazm_lir::op::Module, Vec<Diagnostic>> {
        let data = nazm_lir::abi::DataLayout::of(&self.target);
        let mut m = nazm_lir::op::lower_unit(native, unit, names, data, self.debug, self.soa)?;
        nazm_lir::op::validate::module(&m).map_err(|e| {
            vec![Diagnostic::error(
                codes::COMPILER_DEFECT,
                Span::synthetic(),
                format!("LIR does not validate: {}", e.join("; ")),
            )]
        })?;
        if self.counted_heap {
            nazm_lir::op::count_heap(&mut m);
        }
        Ok(m)
    }

    /// What an object is a function of (§7.106): the unit's LIR — which carries every failure
    /// text, and so every position — this translator's revision, Cranelift's version and
    /// settings, the target, and the runtime. Until N105 it was the unit's MIR, which omits
    /// positions: an object reporting a failure at a line it had moved from was reused.
    fn key(&self, m: &nazm_lir::op::Module) -> String {
        let mut h = blake3::Hasher::new();
        h.update(self.version().as_bytes());
        h.update(&TRANSLATOR_REVISION.to_le_bytes());
        // Every Cranelift setting, as Cranelift prints them: a changed setting is a new object
        // without anyone remembering to bump the revision (Q1-M-10).
        if let Ok(flags) = self.settings() {
            h.update(flags.to_string().as_bytes());
        }
        h.update(self.target.triple.as_bytes());
        h.update(&RUNTIME_ABI_REVISION.to_le_bytes());
        h.update(nazm_lir::runtime::digest().as_bytes());
        h.update(nazm_lir::op::digest(m).as_bytes());
        h.finalize().to_hex().to_string()
    }
}

fn refusal(what: String) -> Diagnostic {
    Diagnostic::error(
        codes::UNSUPPORTED_CONSTRUCT,
        Span::synthetic(),
        format!("the Cranelift backend cannot build this: {what}"),
    )
    .help("`nazm build --backend llvm` is the default backend")
}

impl Backend for Cranelift {
    fn id(&self) -> BackendId {
        BackendId::Cranelift
    }

    fn version(&self) -> String {
        format!(
            "cranelift-{CRANELIFT_VERSION} opt={} pic verify={} runtime-abi={RUNTIME_ABI_REVISION}{}",
            if self.opt_level == 0 { "none" } else { "speed" },
            self.verify,
            if self.debug { " debug" } else { "" }
        )
    }

    fn target(&self) -> &Target {
        &self.target
    }

    fn plan(&self, native: &Native, names: &SourceNames<'_>) -> Vec<Planned> {
        units(native)
            .into_iter()
            .map(|(name, unit, instance)| Planned {
                // A debug object is generated every time, never reused (N91). A unit whose LIR
                // cannot be lowered has no key: generating it reports why.
                key: if self.debug {
                    None
                } else {
                    self.lir(native, unit, names).ok().map(|m| self.key(&m))
                },
                name,
                instance,
            })
            .collect()
    }

    fn generate(
        &self,
        native: &Native,
        names: &SourceNames<'_>,
        unit: usize,
    ) -> Result<Generated, Vec<Diagnostic>> {
        let Some((name, u, _)) = units(native).into_iter().nth(unit) else {
            return Err(vec![refusal(format!("there is no unit {unit}"))]);
        };
        let m = self.lir(native, u, names)?;
        let isa = self.isa().map_err(|e| vec![refusal(e)])?;
        let bytes = translate::object(isa, &m, names, &name, self.debug, self.verify)?;
        Ok(Generated {
            code: Code::Object(bytes),
            needs: m.needs,
        })
    }

    /// Every part of the runtime: a reused object must not need generating to say what it
    /// reaches, so a Cranelift build links the whole runtime.
    fn baseline(&self) -> Needs {
        Needs {
            tasks: true,
            channels: true,
            sequences: true,
            io: true,
            join: true,
            strings: true,
            shims: true,
            stack: true,
            closures: true,
            foreign: true,
            numbers: true,
            files: true,
            net: true,
            wide: true,
            // TLS links the platform's library: only where the program reaches it.
            tls: false,
            single: false,
        }
    }
}
