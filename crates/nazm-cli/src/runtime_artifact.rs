//! `nazm runtime build` and `nazm runtime verify`, and what `nazm build --runtime` checks (N82).
//!
//! `docs/architecture.md` §7.83. A runtime artifact is the whole runtime for one target and profile
//! — every service — compiled once into `libnazmrt.a`, beside `nazm-runtime.json`, a
//! `nazm.runtime/1` manifest saying what it is: the runtime ABI revision and digest it was built
//! from, the target, the profile, the services, the optimisation level, the toolchain, and BLAKE3
//! digests of the text and the archive. A build given one links it instead of generating a runtime,
//! and refuses, by name and before linking, every artifact that does not fit: there is no fallback.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nazm_lir::backend::{Arch, Os, Target};
use nazm_lir::runtime::{Needs, EVERYTHING, RUNTIME_ABI_REVISION};
use serde_json::{json, Value};

use crate::backend::{Backend, Outcome};
use crate::exit;

/// The manifest's schema.
pub(crate) const SCHEMA: &str = "nazm.runtime/1";
/// The archive's file name in an artifact directory.
pub(crate) const ARCHIVE: &str = "libnazmrt.a";
/// The manifest's file name in an artifact directory.
pub(crate) const MANIFEST: &str = "nazm-runtime.json";

/// Which runtime a target links: the operating-system one, a board's, or WebAssembly's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Profile {
    Hosted,
    Board,
    Wasm,
}

impl Profile {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Hosted => "hosted",
            Self::Board => "board",
            Self::Wasm => "wasm",
        }
    }

    fn by_name(name: &str) -> Option<Self> {
        match name {
            "hosted" => Some(Self::Hosted),
            "board" => Some(Self::Board),
            "wasm" => Some(Self::Wasm),
            _ => None,
        }
    }

    /// The profile a target's runtime is: its own, and no other.
    pub(crate) const fn of(target: &Target) -> Self {
        match (target.arch, target.os) {
            (Arch::Wasm32, _) => Self::Wasm,
            (_, Os::None) => Self::Board,
            _ => Self::Hosted,
        }
    }

    /// The runtime's text for this profile, with every service; a board's is its own board's
    /// (N86).
    fn text(self, target: &Target) -> String {
        match self {
            Self::Hosted => nazm_lir::emit_runtime(EVERYTHING),
            Self::Board => {
                nazm_lir::runtime::board::emit_runtime(crate::build::board_of(target), true)
            }
            Self::Wasm => nazm_lir::runtime::wasm::emit_runtime(true),
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The toolchain's own first line of `--version`, or why it could not be read.
fn toolchain() -> String {
    Command::new(crate::backend::DRIVER)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn target_of(triple: Option<&str>) -> Result<Target, String> {
    triple.map_or_else(Target::host, Target::parse)
}

/// `nazm runtime build`.
pub(crate) fn build(triple: Option<&str>, profile: Option<&str>, opt: u8, out: &Path) -> ExitCode {
    let refuse = |why: String| {
        eprintln!("nazm: {why}");
        ExitCode::from(exit::TOOLCHAIN_FAILED)
    };
    let target = match target_of(triple) {
        Ok(t) => t,
        Err(e) => return refuse(e),
    };
    let own = Profile::of(&target);
    let profile = match profile {
        None => own,
        Some(p) => match Profile::by_name(p) {
            Some(p) if p == own => p,
            Some(p) => {
                return refuse(format!(
                    "`{}` links the `{}` runtime, not `{}`",
                    target.triple,
                    own.name(),
                    p.name()
                ))
            }
            None => {
                return refuse(format!(
                    "`{p}` is not a runtime profile; the profiles are hosted, board and wasm"
                ))
            }
        },
    };
    if let Err(e) = std::fs::create_dir_all(out) {
        return refuse(format!("cannot create {}: {e}", out.display()));
    }
    // The working files live beside the artifact, and go when it is written.
    let work = out.join(".nazm-runtime-work");
    if let Err(e) = std::fs::create_dir_all(&work) {
        return refuse(format!("cannot create {}: {e}", work.display()));
    }
    let finish = |code: ExitCode| {
        let _ = std::fs::remove_dir_all(&work);
        code
    };
    let text = profile.text(&target);
    let ll = work.join("runtime.ll");
    let obj = work.join("runtime.o");
    if let Err(e) = std::fs::write(&ll, &text) {
        return finish(refuse(format!("cannot write {}: {e}", ll.display())));
    }
    let host = Target::host().ok();
    let cross = (host.as_ref().map(|h| &h.triple) != Some(&target.triple))
        .then_some(target.triple.as_str());
    let backend = Backend::discover(opt, &work, cross);
    // The hosted runtime has tasks, so it carries its target's task switch (N83) as a second
    // member of the archive.
    // And it calls C, so it carries where its target keeps `errno` (N85) as a third.
    let per_target = (profile == Profile::Hosted)
        .then(|| crate::build::switch_target(&target))
        .flatten();
    let switch = per_target.map(nazm_lir::runtime::switch::emit_switch);
    let errno = per_target.map(nazm_lir::runtime::switch::emit_errno);
    let mut members = vec![obj.clone()];
    for (name, text) in [("switch", &switch), ("errno", &errno)] {
        let Some(text) = text else { continue };
        let sll = work.join(format!("{name}.ll"));
        let sobj = work.join(format!("{name}.o"));
        if let Err(e) = std::fs::write(&sll, text) {
            return finish(refuse(format!("cannot write {}: {e}", sll.display())));
        }
        match backend.compile(&sll, &sobj) {
            Outcome::Ran(o) if o.status.success() => members.push(sobj),
            _ => {
                return finish(refuse(format!(
                    "the toolchain refused the runtime's `{name}` unit"
                )))
            }
        }
    }
    match backend.compile(&ll, &obj) {
        Outcome::Ran(o) if o.status.success() => {}
        Outcome::Ran(o) => {
            return finish(refuse(format!(
                "the toolchain refused the runtime: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )))
        }
        Outcome::Missing => {
            return finish(refuse(format!(
                "`{}` was not found",
                crate::backend::DRIVER
            )));
        }
        Outcome::Failed(e) => return finish(refuse(format!("the toolchain could not run: {e}"))),
    }
    let archive = out.join(ARCHIVE);
    let _ = std::fs::remove_file(&archive);
    // `ZERO_AR_DATE`: the same object archives to the same bytes (Apple's `ar`; GNU `ar` is
    // deterministic by default).
    let archived = Command::new("ar")
        .env("ZERO_AR_DATE", "1")
        .arg("rcs")
        .arg(&archive)
        .args(&members)
        .output();
    match archived {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            return finish(refuse(format!(
                "`ar` refused the runtime: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )))
        }
        Err(e) => return finish(refuse(format!("`ar` could not run: {e}"))),
    }
    let bytes = match std::fs::read(&archive) {
        Ok(b) => b,
        Err(e) => return finish(refuse(format!("cannot read {}: {e}", archive.display()))),
    };
    let manifest = json!({
        "schema": SCHEMA,
        "abi": RUNTIME_ABI_REVISION,
        "runtime": nazm_lir::runtime::digest(),
        "target": target.triple,
        "profile": profile.name(),
        // A board's or WebAssembly's runtime is the whole of it, with the stack check; what it
        // cannot do is refused by the freestanding build itself (N62), not by a service list.
        "services": if profile == Profile::Hosted { EVERYTHING.names() } else { vec!["stack"] },
        "opt_level": opt,
        "toolchain": toolchain(),
        "text": digest(
            format!(
                "{text}{}{}",
                switch.as_deref().unwrap_or(""),
                errno.as_deref().unwrap_or("")
            )
            .as_bytes()
        ),
        "archive": digest(&bytes),
    });
    if let Err(e) = std::fs::write(out.join(MANIFEST), format!("{manifest}\n")) {
        return finish(refuse(format!("cannot write the manifest: {e}")));
    }
    println!(
        "nazm: built the {} runtime for {} into {}",
        profile.name(),
        target.triple,
        archive.display()
    );
    finish(ExitCode::from(exit::OK))
}

/// An artifact that fits: its archive, to link, and its digest.
#[derive(Debug, Clone)]
pub(crate) struct Fitting {
    pub(crate) archive: PathBuf,
    pub(crate) digest: String,
}

/// Whether the artifact in `dir` fits this compiler, `target`, and a program needing `needs` —
/// every check `nazm build --runtime` makes before linking. `Err` names the first that fails.
pub(crate) fn fits(dir: &Path, target: &Target, needs: Option<Needs>) -> Result<Fitting, String> {
    let path = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "the runtime at {} has no readable manifest: {e}",
            dir.display()
        )
    })?;
    let m: Value = serde_json::from_str(&text)
        .map_err(|e| format!("the runtime manifest {} is not JSON: {e}", path.display()))?;
    let field = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("");
    if field("schema") != SCHEMA {
        return Err(format!(
            "the runtime manifest is `{}`, and this compiler reads `{SCHEMA}`",
            field("schema")
        ));
    }
    let abi = m.get("abi").and_then(Value::as_u64);
    if abi != Some(u64::from(RUNTIME_ABI_REVISION)) {
        return Err(format!(
            "the runtime is ABI revision {}, and this compiler's is {RUNTIME_ABI_REVISION}",
            abi.map_or_else(|| "?".to_owned(), |a| a.to_string())
        ));
    }
    let ours = nazm_lir::runtime::digest();
    if field("runtime") != ours {
        return Err(format!(
            "the runtime was built from runtime {}, and this compiler's is {ours}",
            field("runtime")
        ));
    }
    if field("target") != target.triple {
        return Err(format!(
            "the runtime is for {}, and this build is for {}",
            field("target"),
            target.triple
        ));
    }
    let profile = Profile::of(target);
    if field("profile") != profile.name() {
        return Err(format!(
            "the runtime is the `{}` profile, and {} links `{}`",
            field("profile"),
            target.triple,
            profile.name()
        ));
    }
    let services: Vec<&str> = m
        .get("services")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let have = Needs::by_names(services.iter().copied()).ok_or_else(|| {
        "the runtime manifest names a service this compiler does not know".to_owned()
    })?;
    if let Some(needs) = needs.filter(|_| profile == Profile::Hosted) {
        let missing = needs.missing_from(have);
        if !missing.is_empty() {
            return Err(format!(
                "the program reaches {} of the runtime, which this artifact does not provide",
                missing.join(", ")
            ));
        }
    }
    let archive = dir.join(ARCHIVE);
    let bytes = std::fs::read(&archive)
        .map_err(|e| format!("cannot read the runtime archive {}: {e}", archive.display()))?;
    let actual = digest(&bytes);
    if field("archive") != actual {
        return Err(format!(
            "the runtime archive's bytes are {actual}, and its manifest says {}: it is not the \
             archive that was built",
            field("archive")
        ));
    }
    Ok(Fitting {
        archive,
        digest: actual,
    })
}

/// `nazm runtime verify`.
pub(crate) fn verify(dir: &Path, triple: Option<&str>) -> ExitCode {
    let target = match target_of(triple) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    match fits(dir, &target, None) {
        Ok(f) => {
            println!(
                "nazm: the runtime at {} fits this compiler for {} (archive {})",
                dir.display(),
                target.triple,
                f.digest
            );
            ExitCode::from(exit::OK)
        }
        Err(why) => {
            eprintln!("nazm: {why}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
    }
}
