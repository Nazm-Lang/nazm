//! The Cranelift backend: validated MIR to native object code, in process.
//!
//! `docs/architecture.md` §7.43 is its constitution. It implements `nazm-lir`'s [`Backend`]
//! over the units, symbols, linkage and canonical layout tables `nazm-lir` settled from MIR, and
//! produces one object per unit with `cranelift-object`. Every MIR function body is compiled by
//! Cranelift; the runtime and the platform entry stay the runtime's own LLVM text, reached only
//! through the typed registry in `nazm_lir::runtime`.
//!
//! Nothing of Cranelift's appears in this crate's public interface: a caller names a target and
//! an optimisation level and gets a [`Backend`].

mod func;
mod helpers;
mod layout;
mod prims;
mod unit;

use cranelift_codegen::settings::{self, Configurable};
use nazm_diag::{codes, Diagnostic};
use nazm_lir::backend::{units, Backend, BackendId, Code, Generated, Planned, Target};
use nazm_lir::runtime::RUNTIME_ABI_REVISION;
use nazm_lir::{Native, Needs, SourceNames, Unit};
use nazm_span::Span;

/// The exact Cranelift this backend is built with, which enters every cache key.
pub const CRANELIFT_VERSION: &str = "0.136.1";

/// The Cranelift backend.
#[derive(Debug, Clone)]
pub struct Cranelift {
    target: Target,
    opt_level: u8,
    /// Whether Cranelift's IR verifier runs on every function before code generation.
    verify: bool,
}

impl Cranelift {
    /// A Cranelift backend for `target` at `opt_level` (0: none; 1–3: speed).
    #[must_use]
    pub const fn new(target: Target, opt_level: u8) -> Self {
        Self {
            target,
            opt_level,
            verify: true,
        }
    }

    fn settings(&self) -> Result<settings::Flags, String> {
        let mut b = settings::builder();
        let opt = if self.opt_level == 0 { "none" } else { "speed" };
        b.set("opt_level", opt).map_err(|e| e.to_string())?;
        b.set("is_pic", "true").map_err(|e| e.to_string())?;
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

    /// One unit's object, and what it reaches of the runtime.
    fn unit(
        &self,
        native: &Native,
        unit: &Unit,
        names: &SourceNames<'_>,
        name: &str,
    ) -> Result<(Vec<u8>, Needs), Vec<Diagnostic>> {
        let isa = self.isa().map_err(|e| vec![refusal(e)])?;
        let g = unit::Gen::new(isa, native, unit, names, name).map_err(|e| vec![refusal(e)])?;
        g.run(self.verify)
    }

    /// What an object is a function of: the unit's MIR (each function's executable digest), the
    /// layout and symbol tables it was emitted with, the target, this backend's version and
    /// settings, and the runtime ABI revision (§7.43).
    fn key(&self, native: &Native, unit: &Unit) -> String {
        let mut h = blake3::Hasher::new();
        h.update(self.version().as_bytes());
        h.update(self.target.triple.as_bytes());
        h.update(&RUNTIME_ABI_REVISION.to_le_bytes());
        for id in &unit.functions {
            if let (Some(f), Some(sig)) =
                (native.program.function(*id), native.sigs.get(id.index()))
            {
                h.update(nazm_mir::digest::executable(&native.program, f).as_bytes());
                h.update(sig.symbol.as_str().as_bytes());
                h.update(format!("{:?}", sig.linkage).as_bytes());
                // A callee's symbol and signature decide a call's code.
                for b in &f.blocks {
                    for s in &b.stmts {
                        if let nazm_mir::StatementKind::Assign {
                            value: nazm_mir::Rvalue::Call { callee, .. },
                            ..
                        }
                        | nazm_mir::StatementKind::Spawn { callee, .. } = &s.kind
                        {
                            if let Some(c) = native.sigs.get(callee.index()) {
                                h.update(c.symbol.as_str().as_bytes());
                                h.update(format!("{:?}{:?}", c.params, c.ret).as_bytes());
                            }
                        }
                    }
                }
            }
        }
        h.update(format!("{:?}{:?}{:?}", unit.records, unit.enums, unit.vecs).as_bytes());
        // Failure messages name the file a span is in.
        h.update(unit.source.as_bytes());
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
            "cranelift-{CRANELIFT_VERSION} opt={} pic verify={} runtime-abi={RUNTIME_ABI_REVISION}",
            if self.opt_level == 0 { "none" } else { "speed" },
            self.verify
        )
    }

    fn target(&self) -> &Target {
        &self.target
    }

    fn plan(&self, native: &Native) -> Vec<Planned> {
        units(native)
            .into_iter()
            .map(|(name, unit, instance)| Planned {
                key: Some(self.key(native, unit)),
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
        let (bytes, needs) = self.unit(native, u, names, &name)?;
        Ok(Generated {
            code: Code::Object(bytes),
            needs,
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
        }
    }
}
