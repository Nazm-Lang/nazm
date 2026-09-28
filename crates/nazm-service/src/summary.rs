//! Compact command summaries: what a `check` or a `build` concluded, and a reference to each
//! diagnostic it reported, as `nazm.command-summary/1`. N31.
//!
//! # The contract
//!
//! **A summary is an additive view, not a replacement for existing detailed machine output.**
//! It summarizes existing execution truth — the command's own outcome and the diagnostics it
//! produced — and invents no build, test, diagnostic or semantic fact. **Absence of
//! diagnostics does not imply command success**: the status is the command's outcome, given
//! by the caller that ran it, never inferred from a count.
//!
//! # Diagnostic references
//!
//! Every diagnostic the command reported is referenced, by its code, severity, place and fix
//! counts — no message, help, label text or fix prose. Where the diagnostic is one N30 also
//! indexes — the same code, primary, secondary and fix places, in the same order among equals
//! — it carries N30's `id`, and the summary carries N30's `state` for it: `nazm diagnostics
//! --detail` answers it in full. A diagnostic outside N30's universe — a program's missing
//! `main` (`N0204`), which a module's analysis does not ask about, or a backend's refusal of
//! a construct — is a **command** reference with no id: it is never given one it does not
//! have. Linkage needs the loaded sources to be exactly those N30 reads now; if a file moved
//! between the command's read and N30's, nothing is linked and `state` is absent.
//!
//! Places are root-relative files and UTF-8 byte ranges, by N30's source map; references are
//! in N30's canonical order.

use std::path::Path;

use nazm_diag::Diagnostic;
use nazm_span::{FileId, SourceMap, Span};
use serde::Serialize;

use crate::context::SourceSpan;
use crate::diagnostics::{fix_counts, severity, structure, Current, FixCounts, Structure};
use crate::service::{identity, FileKey, LanguageService};

/// The schema every command summary declares.
pub const SCHEMA: &str = "nazm.command-summary/1";

/// Which command a summary is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Check,
    Build,
}

/// How the command ended — its own outcome, one per exit path it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// It did what it was asked: checked clean, or wrote the executable.
    Success,
    /// The program did not check, or the backend refused a construct: diagnostics say why.
    Rejected,
    /// The input could not be read or loaded: a missing file or module, or no source root.
    Unreadable,
    /// The command refused an action it would not take, such as writing an executable over
    /// its own source.
    Refused,
    /// The toolchain was missing or failed, or an output could not be written.
    ToolchainFailed,
}

/// A command's outcome, compactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandSummary {
    pub schema: &'static str,
    pub command: Command,
    pub status: Status,
    /// The process exit status the command returns, unchanged by asking for a summary.
    pub exit_code: u8,
    /// The root module's durable key, where the compilation has one.
    pub root: Option<String>,
    /// N30's `state.source`, which the references' ids are of: present whenever the command
    /// read exactly the sources N30 reads now, absent otherwise.
    pub state: Option<String>,
    pub counts: Counts,
    /// Every diagnostic the command reported, in N30's canonical order.
    pub diagnostics: Vec<DiagnosticRef>,
    /// What a build wrote; `None` for a check, and for a build that wrote nothing.
    pub artifact: Option<Artifact>,
}

/// How many diagnostics, and the compiler fixes they carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub diagnostics: usize,
    /// Of severity `error`, the one severity the compiler has.
    pub error: usize,
    pub with_fixes: usize,
    pub automatic_fixes: usize,
    pub needs_review_fixes: usize,
}

/// One diagnostic the command reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticRef {
    /// N30's id for it, within `state`; `None` for a command reference.
    pub id: Option<String>,
    /// `index` — N30 indexes it, and `id` selects its detail — or `command`, one only the
    /// command reports.
    pub scope: &'static str,
    pub code: String,
    pub severity: &'static str,
    pub primary: Option<SourceSpan>,
    pub fixes: FixCounts,
}

/// The file a build wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Artifact {
    /// As the command prints it: `-o`'s path as given, or the default beside the working
    /// directory.
    pub path: String,
}

impl CommandSummary {
    /// The one canonical serialisation.
    ///
    /// # Panics
    ///
    /// Never: every field is plain data.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a summary is plain data")
    }
}

/// What the caller ran, and what it concluded. The summary adds references and nothing
/// else.
#[derive(Debug)]
pub struct Outcome<'a> {
    pub command: Command,
    pub status: Status,
    pub exit_code: u8,
    /// Every diagnostic the command reported, and the map their spans are in.
    pub diagnostics: &'a [Diagnostic],
    pub map: Option<&'a SourceMap>,
    pub artifact: Option<String>,
}

/// The summary of `outcome`, a command run over the compilation rooted at `root`.
///
/// Must run on the parser's stack: it analyses the root, as N30 does, to link references.
#[must_use]
pub fn summarize(outcome: &Outcome<'_>, root: &Path, source_root: Option<&Path>) -> CommandSummary {
    let mut service = LanguageService::with_source_root(source_root.map(Path::to_path_buf));
    let opened = service.open_from_disk(root).is_ok();
    let current = if opened {
        service.current_diagnostics(root, 0).ok()
    } else {
        None
    };
    let linked = match (&current, outcome.map) {
        (Some(c), Some(map)) => same_sources(c, map).then_some(c),
        _ => None,
    };

    let mut refs: Vec<(Structure, usize, DiagnosticRef)> = Vec::new();
    for (k, d) in outcome.diagnostics.iter().enumerate() {
        let place = |at: Span| match (linked, outcome.map) {
            (Some(c), Some(map)) => place(c, map, at),
            _ => None,
        };
        let primary = place(d.primary.span);
        let secondary: Vec<Option<SourceSpan>> =
            d.secondary.iter().map(|l| place(l.span)).collect();
        let fixes: Vec<Option<SourceSpan>> = d.fixes.iter().map(|f| place(f.span)).collect();
        let key = structure(&primary, d.code.as_str(), &secondary, &fixes);
        refs.push((
            key,
            k,
            DiagnosticRef {
                id: None,
                scope: "command",
                code: d.code.to_string(),
                severity: severity(d),
                primary,
                fixes: fix_counts(d),
            },
        ));
    }
    refs.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));

    // Each reference is N30's diagnostic with the same structure, the n-th of those with it
    // matched to the n-th — never by what it says.
    if let Some(c) = linked {
        let mut taken = vec![false; c.entries.len()];
        for (key, _, r) in &mut refs {
            let found = c.entries.iter().enumerate().find(|(i, e)| {
                !taken.get(*i).copied().unwrap_or(true)
                    && structure(&e.primary, e.d.code.as_str(), &e.secondary, &e.fixes) == *key
            });
            if let Some((i, e)) = found {
                if let Some(t) = taken.get_mut(i) {
                    *t = true;
                }
                r.id = Some(e.id.clone());
                r.scope = "index";
            }
        }
    }
    let diagnostics: Vec<DiagnosticRef> = refs.into_iter().map(|(_, _, r)| r).collect();
    let counts = Counts {
        diagnostics: diagnostics.len(),
        error: diagnostics.iter().filter(|r| r.severity == "error").count(),
        with_fixes: diagnostics.iter().filter(|r| r.fixes.total > 0).count(),
        automatic_fixes: diagnostics.iter().map(|r| r.fixes.automatic).sum(),
        needs_review_fixes: diagnostics.iter().map(|r| r.fixes.needs_review).sum(),
    };
    CommandSummary {
        schema: SCHEMA,
        command: outcome.command,
        status: outcome.status,
        exit_code: outcome.exit_code,
        root: current.as_ref().map(|c| c.root.module.clone()),
        state: linked.map(|c| c.state.source.clone()),
        counts,
        diagnostics,
        artifact: outcome.artifact.clone().map(|path| Artifact { path }),
    }
}

/// Whether the command read exactly the sources N30 has now: the same files, in the same
/// load order — both come from one loader — each by identity and with the same bytes.
fn same_sources(c: &Current<'_>, map: &SourceMap) -> bool {
    map.ids().count() == c.s.files.len()
        && map.ids().all(|f| {
            let i = f.index();
            let same_file = match c.s.files.get(i) {
                Some(FileKey::Path(p)) => *p == identity(Path::new(map.name(f))),
                Some(FileKey::Prelude) => true,
                None => false,
            };
            same_file && c.s.texts.get(i).is_some_and(|t| **t == *map.text(f))
        })
}

/// `at`, a span in the command's map, as a place in N30's compilation.
fn place(c: &Current<'_>, map: &SourceMap, at: Span) -> Option<SourceSpan> {
    if at == Span::synthetic() {
        return None;
    }
    let key = FileKey::Path(identity(Path::new(map.name(at.file))));
    let f = c.s.files.iter().position(|k| *k == key)?;
    c.paths
        .span(Span::new(FileId::from_index(f), at.start, at.end))
}

/// `nazm check`'s outcome for the compilation rooted at `root`, without its cache: the same
/// loader, the same checker and the same diagnostics, and nothing written. For a caller
/// that must not write — the MCP server. Must run on the parser's stack.
#[must_use]
pub fn check(root: &Path, source_root: Option<&Path>) -> CommandSummary {
    let decided = match crate::root::decide(root, source_root) {
        Ok(r) => r,
        Err(_) => {
            return summarize(
                &Outcome {
                    command: Command::Check,
                    status: Status::Unreadable,
                    exit_code: 3,
                    diagnostics: &[],
                    map: None,
                    artifact: None,
                },
                root,
                source_root,
            )
        }
    };
    match crate::load::load(root, decided.path()) {
        Err((partial, diagnostics)) => summarize(
            &Outcome {
                command: Command::Check,
                status: Status::Unreadable,
                exit_code: 3,
                diagnostics: &diagnostics,
                map: Some(&partial.map),
                artifact: None,
            },
            root,
            source_root,
        ),
        Ok(loaded) => {
            let diagnostics = nazm_core::check_map(&loaded.map, &loaded.graph);
            let (status, exit_code) = if diagnostics.is_empty() {
                (Status::Success, 0)
            } else {
                (Status::Rejected, 1)
            };
            summarize(
                &Outcome {
                    command: Command::Check,
                    status,
                    exit_code,
                    diagnostics: &diagnostics,
                    map: Some(&loaded.map),
                    artifact: None,
                },
                root,
                source_root,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    /// The structural half of "summarizes existing truth": this module never reads a
    /// diagnostic's prose or a command's output, and keeps nothing between requests.
    #[test]
    fn a_summary_is_never_read_from_prose_or_output() {
        let source = include_str!("summary.rs");
        let code: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            ".message",
            ".help",
            ".description",
            ".precondition",
            "stdout",
            "stderr",
            "Regex",
            ".find(\"",
            ".contains(\"",
            "starts_with(\"",
            "HashMap",
            "OnceLock",
            "static mut",
            "Command::new",
            "fs::write",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in summary: a summary fact would come from prose or output"
            );
        }
    }
}
