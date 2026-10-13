//! What a mutation campaign will do, decided before anything is injected.
//!
//! # The catalogue, and the two things it may now say
//!
//! Every `[[mutation]]` still names a defect: `name`, `why`, `file`, `find`, `replace`.
//! Two optional fields say where the defect is expected to be *seen*:
//!
//! ```toml
//! killers = [{ package = "nazm-cli", test = "value_blocks", name = "a_str_tail_survives_its_local" }]
//! profile = "native"
//! ```
//!
//! A **killer** is one test, named exactly: its package, one target (`lib = true`,
//! `test = "…"` or `bin = "…"`), and its full libtest path. It is data, not a command.
//! The harness builds the invocation itself, so the catalogue cannot run anything the
//! harness would not already run.
//!
//! A **profile** is a named group of whole test targets, declared once in `[[profile]]`
//! with the source paths it `owns`. A mutation that names none gets the profile owning
//! the longest prefix of its file, or no profile at all. A profile's test target may name
//! a `module`: one file of an executable that gathers several (`nazm-cli`'s `cli_*`), run
//! as the tests whose paths begin with it.
//!
//! # What neither of them can do
//!
//! Establish that a mutant survived. Only the workspace suite ([`broad_suite`]) can do
//! that, and the tier logic in `campaign.rs` makes it structurally impossible to reach
//! `SURVIVED` any other way. A wrong killer or an over-narrow profile costs time. It
//! cannot produce a false verdict.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// One catalogued defect.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mutation {
    pub(crate) name: String,
    pub(crate) why: String,
    pub(crate) file: String,
    pub(crate) find: String,
    pub(crate) replace: String,
    /// Tests verified to fail under this mutant and pass without it.
    #[serde(default)]
    pub(crate) killers: Vec<KillerSpec>,
    /// Overrides the profile that would be chosen from `file`.
    #[serde(default)]
    pub(crate) profile: Option<String>,
    /// Why this entry has no killer. Required of an entry that names a profile but no
    /// killer, so the exception is written down where it is made.
    #[serde(default)]
    pub(crate) no_killer: Option<String>,
}

/// A test target, as the catalogue spells it: exactly one of `lib`, `test`, `bin`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetSpec {
    pub(crate) package: String,
    #[serde(default)]
    pub(crate) lib: bool,
    #[serde(default)]
    pub(crate) test: Option<String>,
    #[serde(default)]
    pub(crate) bin: Option<String>,
    /// One module of a `test` target that gathers several files: only its tests run.
    #[serde(default)]
    pub(crate) module: Option<String>,
}

/// One test, as the catalogue spells it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KillerSpec {
    pub(crate) package: String,
    #[serde(default)]
    pub(crate) lib: bool,
    #[serde(default)]
    pub(crate) test: Option<String>,
    #[serde(default)]
    pub(crate) bin: Option<String>,
    /// The full libtest path, matched with `--exact`.
    pub(crate) name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) name: String,
    pub(crate) why: String,
    /// Source path prefixes this profile is the default for.
    #[serde(default)]
    pub(crate) owns: Vec<String>,
    pub(crate) targets: Vec<TargetSpec>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalogue {
    pub(crate) mutation: Vec<Mutation>,
    #[serde(default)]
    pub(crate) profile: Vec<Profile>,
}

pub(crate) fn load(path: &Path) -> Result<Catalogue, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let catalogue: Catalogue =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if catalogue.mutation.is_empty() {
        return Err("catalogue has no [[mutation]] entries".to_owned());
    }
    Ok(catalogue)
}

/// Which kind of test target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub(crate) enum TargetKind {
    Lib,
    Bin(String),
    Test(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub(crate) struct Target {
    pub(crate) package: String,
    pub(crate) kind: TargetKind,
}

impl Target {
    fn from_parts(
        package: &str,
        lib: bool,
        test: Option<&String>,
        bin: Option<&String>,
    ) -> Result<Self, String> {
        let kind = match (lib, test, bin) {
            (true, None, None) => TargetKind::Lib,
            (false, Some(t), None) => TargetKind::Test(t.clone()),
            (false, None, Some(b)) => TargetKind::Bin(b.clone()),
            _ => {
                return Err(format!(
                    "a target in `{package}` must name exactly one of `lib = true`, \
                     `test = \"…\"` or `bin = \"…\"`"
                ))
            }
        };
        Ok(Self {
            package: package.to_owned(),
            kind,
        })
    }

    pub(crate) fn from_spec(s: &TargetSpec) -> Result<Self, String> {
        Self::from_parts(&s.package, s.lib, s.test.as_ref(), s.bin.as_ref())
    }

    /// The cargo flags that select exactly this target.
    fn flags(&self) -> Vec<String> {
        let mut v = vec!["-p".to_owned(), self.package.clone()];
        match &self.kind {
            TargetKind::Lib => v.push("--lib".to_owned()),
            TargetKind::Bin(b) => {
                v.push("--bin".to_owned());
                v.push(b.clone());
            }
            TargetKind::Test(t) => {
                v.push("--test".to_owned());
                v.push(t.clone());
            }
        }
        v
    }

    pub(crate) fn label(&self) -> String {
        match &self.kind {
            TargetKind::Lib => format!("{} --lib", self.package),
            TargetKind::Bin(b) => format!("{} --bin {b}", self.package),
            TargetKind::Test(t) => format!("{} --test {t}", self.package),
        }
    }

    /// `cargo test` arguments that list this target's tests, one `name: test` line each.
    pub(crate) fn list_args(&self, ignored_only: bool, par: Parallelism) -> Vec<String> {
        let mut v = vec!["test".to_owned()];
        v.extend(self.flags());
        v.extend(["--jobs".to_owned(), par.cargo_jobs.to_string()]);
        v.extend(["--", "--list", "--format", "terse"].map(str::to_owned));
        if ignored_only {
            v.push("--ignored".to_owned());
        }
        v
    }
}

/// Something the harness can run and read a verdict from: one target whole, or one
/// test in it. Knows nothing about mutations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub(crate) struct TestSelector {
    pub(crate) target: Target,
    /// `Some` for a killer: exactly this test and no other.
    pub(crate) exact: Option<String>,
    /// `Some` for one module of a gathered target: the tests whose paths begin with it.
    pub(crate) module: Option<String>,
}

impl TestSelector {
    pub(crate) fn from_killer(k: &KillerSpec) -> Result<Self, String> {
        Ok(Self {
            target: Target::from_parts(&k.package, k.lib, k.test.as_ref(), k.bin.as_ref())?,
            exact: Some(k.name.clone()),
            module: None,
        })
    }

    pub(crate) fn whole(target: Target) -> Self {
        Self {
            target,
            exact: None,
            module: None,
        }
    }

    /// One module of `target`, or the whole of it.
    pub(crate) fn of_module(target: Target, module: Option<&String>) -> Self {
        Self {
            module: module.cloned(),
            ..Self::whole(target)
        }
    }

    /// The `cargo test` arguments, under the same [`Parallelism`] as the broad suite: the
    /// same execution model, so a test cannot pass here and fail there for scheduling
    /// reasons.
    pub(crate) fn cargo_args(&self, par: Parallelism) -> Vec<String> {
        let mut v = vec!["test".to_owned()];
        v.extend(self.target.flags());
        v.extend(par.test_flags());
        if let Some(name) = &self.exact {
            v.push(name.clone());
            v.push("--exact".to_owned());
        } else if let Some(m) = &self.module {
            v.push(format!("{m}::"));
        }
        v
    }

    pub(crate) fn label(&self) -> String {
        match (&self.exact, &self.module) {
            (Some(n), _) => format!("{} {n}", self.target.label()),
            (None, Some(m)) => format!("{} {m}::", self.target.label()),
            (None, None) => self.target.label(),
        }
    }
}

/// **The** workspace suite: the single definition of Tier 3, and of the baseline.
///
/// Both strategies call this and nothing else calls `cargo test --workspace`. If the
/// oracle and the optimisation each spelled the command, they could drift apart and the
/// optimisation would be checked against a suite nobody runs.
///
/// Under [`Parallelism::SERIAL`] it is the invocation the legacy runner used, flag for
/// flag: one build job, one test thread, and `--no-fail-fast` so a caught mutant still
/// reports every binary.
pub(crate) fn broad_suite(par: Parallelism) -> Vec<String> {
    let mut v = vec!["test".to_owned(), "--workspace".to_owned()];
    v.extend(par.test_flags());
    v
}

/// Does the tree build, tests included? The legacy runner's check, unchanged, and still
/// the thing that separates UNUSABLE from CAUGHT.
pub(crate) fn workspace_build(par: Parallelism) -> Vec<String> {
    // `--message-format=json` makes Cargo say, per unit, whether it compiled it or found it
    // fresh (N32-H3); it changes nothing Cargo decides.
    let mut v: Vec<String> = [
        "build",
        "--workspace",
        "--tests",
        "--quiet",
        "--message-format=json",
    ]
    .map(str::to_owned)
    .to_vec();
    v.extend(["--jobs".to_owned(), par.cargo_jobs.to_string()]);
    v
}

/// How much parallelism **one** step of a campaign may use inside itself. N14.1.
///
/// Never how many mutants run at once — that is always one. This is the Cargo job count
/// for each build and the libtest thread count for each test run, and it is part of a
/// campaign's identity: a verdict that carries a deadline is evidence about the
/// execution that produced it, so a journal written under one setting is never resumed
/// under another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Parallelism {
    pub(crate) cargo_jobs: u32,
    pub(crate) test_threads: u32,
}

impl Parallelism {
    /// One job, one thread: the legacy runner's execution, and the default.
    pub(crate) const SERIAL: Self = Self {
        cargo_jobs: 1,
        test_threads: 1,
    };

    /// `--jobs N --no-fail-fast -- --test-threads M`.
    fn test_flags(self) -> Vec<String> {
        vec![
            "--jobs".to_owned(),
            self.cargo_jobs.to_string(),
            "--no-fail-fast".to_owned(),
            "--".to_owned(),
            "--test-threads".to_owned(),
            self.test_threads.to_string(),
        ]
    }
}

/// Where a profile choice came from, so a plan can say.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) enum ProfileSource {
    Explicit,
    Owner(String),
}

/// Everything a campaign will do for one mutation, fixed before injection.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct MutationPlan {
    /// Position in the catalogue. Campaign order is catalogue order.
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) killers: Vec<TestSelector>,
    pub(crate) profile: Option<(String, ProfileSource, Vec<TestSelector>)>,
}

/// Plan every selected mutation. Pure: reads nothing but the catalogue.
///
/// Refuses rather than guessing when the metadata is inconsistent. The static checks
/// here are the same ones `cargo xtask check` runs, minus the file reads.
pub(crate) fn plan(
    catalogue: &Catalogue,
    selected: &[usize],
) -> Result<Vec<MutationPlan>, Vec<String>> {
    let mut problems = Vec::new();
    let profiles: BTreeMap<&str, &Profile> = catalogue
        .profile
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();
    let mut out = Vec::new();
    for &i in selected {
        let Some(m) = catalogue.mutation.get(i) else {
            problems.push(format!("no mutation at index {i}"));
            continue;
        };
        let mut killers = Vec::new();
        for k in &m.killers {
            match TestSelector::from_killer(k) {
                Ok(s) => killers.push(s),
                Err(e) => problems.push(format!("{}: {e}", m.name)),
            }
        }
        let chosen = match &m.profile {
            Some(p) => Some((p.clone(), ProfileSource::Explicit)),
            None => owner(&catalogue.profile, &m.file)
                .map(|(p, prefix)| (p, ProfileSource::Owner(prefix))),
        };
        let profile = match chosen {
            None => None,
            Some((name, source)) => match profiles.get(name.as_str()) {
                None => {
                    problems.push(format!("{}: profile `{name}` is not declared", m.name));
                    None
                }
                Some(p) => {
                    let mut targets = Vec::new();
                    for t in &p.targets {
                        match Target::from_spec(t) {
                            Ok(target) => {
                                targets.push(TestSelector::of_module(target, t.module.as_ref()));
                            }
                            Err(e) => problems.push(format!("profile {}: {e}", p.name)),
                        }
                    }
                    Some((name, source, targets))
                }
            },
        };
        out.push(MutationPlan {
            index: i,
            name: m.name.clone(),
            file: m.file.clone(),
            killers,
            profile,
        });
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// The profile owning the longest prefix of `file`, and that prefix.
fn owner(profiles: &[Profile], file: &str) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for p in profiles {
        for prefix in &p.owns {
            if file.starts_with(prefix.as_str())
                && best.as_ref().is_none_or(|(_, b)| prefix.len() > b.len())
            {
                best = Some((p.name.clone(), prefix.clone()));
            }
        }
    }
    best
}

/// The plan, as `--plan` prints it.
pub(crate) fn render(plans: &[MutationPlan]) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for p in plans {
        let _ = writeln!(s, "{:>4}  {}  ({})", p.index + 1, p.name, p.file);
        if p.killers.is_empty() {
            let _ = writeln!(s, "        tier 1  —  no verified killer");
        }
        for k in &p.killers {
            let _ = writeln!(s, "        tier 1  {}", k.label());
        }
        match &p.profile {
            None => {
                let _ = writeln!(s, "        tier 2  —  no profile owns this file");
            }
            Some((name, source, targets)) => {
                let why = match source {
                    ProfileSource::Explicit => "named by the entry".to_owned(),
                    ProfileSource::Owner(prefix) => format!("owns {prefix}"),
                };
                let list: Vec<String> = targets.iter().map(TestSelector::label).collect();
                let _ = writeln!(s, "        tier 2  {name} ({why}): {}", list.join(", "));
            }
        }
        let _ = writeln!(
            s,
            "        tier 3  workspace suite (the only tier that can report SURVIVED)"
        );
    }
    let with_killer = plans.iter().filter(|p| !p.killers.is_empty()).count();
    let profile_only = plans
        .iter()
        .filter(|p| p.killers.is_empty() && p.profile.is_some())
        .count();
    let neither = plans.len() - with_killer - profile_only;
    let _ = writeln!(
        s,
        "\n{} selected: {with_killer} with a killer, {profile_only} with a profile only, \
         {neither} straight to the workspace suite",
        plans.len()
    );
    s
}

// ---------------------------------------------------------------------------
// Static validation: file reads only, so it belongs in `cargo xtask check`.
// ---------------------------------------------------------------------------

/// A workspace member, as far as target resolution needs one.
#[derive(Debug)]
struct Package {
    dir: PathBuf,
    has_lib: bool,
    /// Binary name and its root file.
    bins: BTreeMap<String, PathBuf>,
}

fn workspace(root: &Path) -> Result<BTreeMap<String, Package>, String> {
    let manifest =
        fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let doc: toml::Value = toml::from_str(&manifest).map_err(|e| format!("Cargo.toml: {e}"))?;
    let members = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or("Cargo.toml has no [workspace] members")?;
    let mut out = BTreeMap::new();
    for m in members.iter().filter_map(toml::Value::as_str) {
        let dir = root.join(m);
        let text = fs::read_to_string(dir.join("Cargo.toml"))
            .map_err(|e| format!("{m}/Cargo.toml: {e}"))?;
        let pkg: toml::Value = toml::from_str(&text).map_err(|e| format!("{m}/Cargo.toml: {e}"))?;
        let Some(name) = pkg
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
        else {
            continue;
        };
        let mut bins = BTreeMap::new();
        if let Some(list) = pkg.get("bin").and_then(toml::Value::as_array) {
            for b in list {
                if let Some(n) = b.get("name").and_then(toml::Value::as_str) {
                    let path = b
                        .get("path")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("src/main.rs");
                    bins.insert(n.to_owned(), dir.join(path));
                }
            }
        } else if dir.join("src/main.rs").exists() {
            bins.insert(name.to_owned(), dir.join("src/main.rs"));
        }
        out.insert(
            name.to_owned(),
            Package {
                has_lib: dir.join("src/lib.rs").exists(),
                dir,
                bins,
            },
        );
    }
    Ok(out)
}

/// The root source file of a target, or why there is none.
fn target_root(ws: &BTreeMap<String, Package>, t: &Target) -> Result<PathBuf, String> {
    let Some(p) = ws.get(&t.package) else {
        return Err(format!("package `{}` is not a workspace member", t.package));
    };
    match &t.kind {
        TargetKind::Lib if p.has_lib => Ok(p.dir.join("src/lib.rs")),
        TargetKind::Lib => Err(format!("package `{}` has no library target", t.package)),
        TargetKind::Bin(b) => p
            .bins
            .get(b)
            .cloned()
            .ok_or_else(|| format!("package `{}` has no binary `{b}`", t.package)),
        TargetKind::Test(name) => {
            let flat = p.dir.join("tests").join(format!("{name}.rs"));
            let nested = p.dir.join("tests").join(name).join("main.rs");
            if flat.exists() {
                Ok(flat)
            } else if nested.exists() {
                Ok(nested)
            } else {
                Err(format!(
                    "package `{}` has no integration test `{name}`",
                    t.package
                ))
            }
        }
    }
}

/// The file a module path inside a crate resolves to, and the inline modules left over.
///
/// `root` is the crate root (`lib.rs`, `main.rs`, `tests/x.rs`). Follows `mod x;` the
/// way rustc does for the layouts this repository uses (`x.rs` beside the parent, or
/// `x/mod.rs`), and stops at the first segment with no file, leaving the rest to be
/// found as inline `mod x {` blocks.
fn resolve_module<'a>(root: &Path, segments: &'a [&'a str]) -> (PathBuf, &'a [&'a str]) {
    let mut file = root.to_path_buf();
    let mut dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut rest = segments;
    while let Some((seg, tail)) = rest.split_first() {
        let flat = dir.join(format!("{seg}.rs"));
        let modrs = dir.join(seg).join("mod.rs");
        if flat.exists() {
            file = flat;
            dir = dir.join(seg);
        } else if modrs.exists() {
            file = modrs;
            dir = dir.join(seg);
        } else {
            break;
        }
        rest = tail;
    }
    (file, rest)
}

/// Is there a non-ignored `#[test] fn <last>` at this path? Returns why not.
///
/// A static check, and deliberately not the authority: the campaign lists every killer's
/// target from the built binary before injecting anything, and refuses on a name the
/// binary does not have. This exists so a renamed test fails `cargo xtask check` in
/// milliseconds rather than a campaign in minutes.
fn find_test(root: &Path, name: &str) -> Result<(), String> {
    let segments: Vec<&str> = name.split("::").collect();
    let Some((last, modules)) = segments.split_last() else {
        return Err("an empty test name".to_owned());
    };
    let (file, inline) = resolve_module(root, modules);
    let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    for m in inline {
        if !text.contains(&format!("mod {m} {{")) && !text.contains(&format!("mod {m}{{")) {
            return Err(format!(
                "`{name}`: module `{m}` is neither a file nor an inline module in {}",
                file.display()
            ));
        }
    }
    let lines: Vec<&str> = text.lines().collect();
    let needle = format!("fn {last}(");
    let mut found = 0;
    let mut ignored = false;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if !(t.starts_with(&needle) || t.starts_with(&format!("pub {needle}"))) {
            continue;
        }
        // The attributes directly above the function, through doc comments and blank
        // lines, up to the previous item.
        let mut is_test = false;
        let mut is_ignored = false;
        for prev in lines.get(..i).unwrap_or_default().iter().rev() {
            let p = prev.trim();
            if p.starts_with("#[test]") {
                is_test = true;
            } else if p.starts_with("#[ignore") {
                is_ignored = true;
            } else if p.starts_with("#[") || p.starts_with("//") || p.is_empty() || p.ends_with(']')
            {
                continue;
            } else {
                break;
            }
        }
        if is_test {
            found += 1;
            ignored |= is_ignored;
        }
    }
    match (found, ignored) {
        (0, _) => Err(format!(
            "`{name}`: no `#[test] fn {last}` in {}",
            file.display()
        )),
        (_, true) => Err(format!(
            "`{name}` is `#[ignore]`d, so the workspace suite does not run it and a failure \
             there is not evidence the suite detects anything"
        )),
        _ => Ok(()),
    }
}

/// Every check that can be made without building anything.
///
/// Refuses: unknown fields (at parse), duplicate mutation or profile names, a target
/// naming no or several kinds, a package, target or test that does not exist, an
/// ignored killer, a duplicated killer, an unknown profile, a profile with no targets,
/// two profiles owning the same prefix, a profile named with no killer and no written
/// reason, and a reason written where a killer exists.
pub(crate) fn validate(root: &Path, catalogue: &Catalogue) -> Vec<String> {
    let mut bad = Vec::new();
    let ws = match workspace(root) {
        Ok(w) => w,
        Err(e) => return vec![e],
    };

    let mut names = BTreeSet::new();
    for m in &catalogue.mutation {
        if !names.insert(m.name.as_str()) {
            bad.push(format!(
                "{}: the name is used by more than one mutation",
                m.name
            ));
        }
    }

    let mut profile_names = BTreeSet::new();
    let mut prefixes: BTreeMap<&str, &str> = BTreeMap::new();
    for p in &catalogue.profile {
        if !profile_names.insert(p.name.as_str()) {
            bad.push(format!("profile {}: declared twice", p.name));
        }
        if p.targets.is_empty() {
            bad.push(format!("profile {}: has no targets", p.name));
        }
        if p.why.trim().is_empty() {
            bad.push(format!(
                "profile {}: does not say why these targets observe what it owns",
                p.name
            ));
        }
        for prefix in &p.owns {
            if let Some(other) = prefixes.insert(prefix.as_str(), p.name.as_str()) {
                bad.push(format!(
                    "profiles {other} and {} both own `{prefix}`; ownership must be unique",
                    p.name
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for t in &p.targets {
            match Target::from_spec(t) {
                Err(e) => bad.push(format!("profile {}: {e}", p.name)),
                Ok(target) => {
                    match target_root(&ws, &target) {
                        Err(e) => bad.push(format!("profile {}: {e}", p.name)),
                        Ok(root) => {
                            if let Some(m) = &t.module {
                                if !matches!(&target.kind, TargetKind::Test(_)) {
                                    bad.push(format!(
                                        "profile {}: only a test target has modules, not {}",
                                        p.name,
                                        target.label()
                                    ));
                                } else if resolve_module(&root, &[m.as_str()]).1.len() == 1 {
                                    bad.push(format!(
                                        "profile {}: {} has no module `{m}`",
                                        p.name,
                                        target.label()
                                    ));
                                }
                            }
                        }
                    }
                    let label = TestSelector::of_module(target.clone(), t.module.as_ref()).label();
                    if !seen.insert(label.clone()) {
                        bad.push(format!("profile {}: {label} is listed twice", p.name));
                    }
                }
            }
        }
    }

    for m in &catalogue.mutation {
        let mut seen = BTreeSet::new();
        for k in &m.killers {
            let sel = match TestSelector::from_killer(k) {
                Ok(s) => s,
                Err(e) => {
                    bad.push(format!("{}: {e}", m.name));
                    continue;
                }
            };
            if !seen.insert(sel.clone()) {
                bad.push(format!(
                    "{}: killer {} is listed twice",
                    m.name,
                    sel.label()
                ));
            }
            match target_root(&ws, &sel.target) {
                Err(e) => bad.push(format!("{}: {e}", m.name)),
                Ok(root_file) => {
                    if let Err(e) = find_test(&root_file, &k.name) {
                        bad.push(format!(
                            "{}: killer {} does not resolve: {e}. Stale killer metadata is a \
                             catalogue defect; repoint it at the test that observes the rule \
                             now, and verify it with `cargo xtask contained mutate \
                             --verify-killer {}`",
                            m.name,
                            sel.target.label(),
                            m.name
                        ));
                    }
                }
            }
        }
        if let Some(p) = &m.profile {
            if !profile_names.contains(p.as_str()) {
                bad.push(format!("{}: profile `{p}` is not declared", m.name));
            }
        }
        // No silent tier (N101): an entry with no killer would cost a whole suite in every
        // campaign, and its verdict would wait on that. It names its killers, or says why it has
        // none — a known survivor, or a rule no single test observes.
        if m.killers.is_empty() && m.no_killer.as_deref().is_none_or(|w| w.trim().is_empty()) {
            bad.push(format!(
                "{}: has no killer, and does not say why in `no_killer`",
                m.name
            ));
        }
        if !m.killers.is_empty() && m.no_killer.is_some() {
            bad.push(format!("{}: has killers and also says it has none", m.name));
        }
    }
    bad
}

/// Indices of the selected mutations, in catalogue order. Substring match, repeatable,
/// exactly as `--only` has always worked; a selection of nothing is an error, because a
/// run that selects nothing reports a clean result having tested nothing.
/// Whether a mutant's name answers one `--only` filter: a substring, or — written `=NAME` —
/// exactly that name, for a mutant whose name another's contains (N40:
/// `vec-get-returns-a-borrowed-element` inside `selfhost-vec-get-returns-a-borrowed-element`).
fn answers(name: &str, filter: &str) -> bool {
    match filter.strip_prefix('=') {
        Some(exact) => name == exact,
        None => name.contains(filter),
    }
}

pub(crate) fn select(catalogue: &Catalogue, only: &[String]) -> Result<Vec<usize>, String> {
    let all: Vec<usize> = (0..catalogue.mutation.len()).collect();
    if only.is_empty() {
        return Ok(all);
    }
    let chosen: Vec<usize> = all
        .into_iter()
        .filter(|&i| {
            catalogue
                .mutation
                .get(i)
                .is_some_and(|m| only.iter().any(|w| answers(&m.name, w)))
        })
        .collect();
    if chosen.is_empty() {
        return Err(format!(
            "no mutation matches {only:?}. A filter that selects nothing would report a \
             clean run having tested nothing."
        ));
    }
    Ok(chosen)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::{broad_suite, find_test, owner, plan, select, validate, Catalogue, ProfileSource};
    use std::fs;

    fn cat(text: &str) -> Catalogue {
        toml::from_str(text).expect("catalogue")
    }

    const ONE: &str = r#"
[[mutation]]
name = "m"
why = "w"
file = "crates/a/src/x.rs"
find = "f"
replace = "r"
"#;

    #[test]
    fn a_historical_entry_still_parses_and_plans_with_no_killer() {
        let c = cat(ONE);
        let p = plan(&c, &[0]).expect("plan");
        assert!(p[0].killers.is_empty());
        assert!(p[0].profile.is_none());
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        // `killer` for `killers` would otherwise be a silent no-op: the entry would run
        // without its killer and nobody would know the metadata was never read.
        let r: Result<Catalogue, _> = toml::from_str(&format!("{ONE}killer = []\n"));
        assert!(r.is_err());
    }

    #[test]
    fn a_target_must_name_exactly_one_kind() {
        let c = cat(&format!(
            "{ONE}killers = [{{ package = \"a\", lib = true, test = \"t\", name = \"x\" }}]\n"
        ));
        let e = plan(&c, &[0]).expect_err("refused");
        assert!(e[0].contains("exactly one"), "{e:?}");
        let c = cat(&format!(
            "{ONE}killers = [{{ package = \"a\", name = \"x\" }}]\n"
        ));
        assert!(plan(&c, &[0]).is_err());
    }

    #[test]
    fn a_killer_is_run_exactly_and_serially() {
        let c = cat(&format!(
            "{ONE}killers = [{{ package = \"nazm-cli\", test = \"memory\", name = \"a::b\" }}]\n"
        ));
        let p = plan(&c, &[0]).expect("plan");
        assert_eq!(
            p[0].killers[0].cargo_args(super::Parallelism::SERIAL),
            [
                "test",
                "-p",
                "nazm-cli",
                "--test",
                "memory",
                "--jobs",
                "1",
                "--no-fail-fast",
                "--",
                "--test-threads",
                "1",
                "a::b",
                "--exact"
            ]
        );
    }

    #[test]
    fn a_profile_target_s_module_runs_only_its_own_tests() {
        let target = super::Target {
            package: "nazm-cli".to_owned(),
            kind: super::TargetKind::Test("cli_m_r".to_owned()),
        };
        let one = super::TestSelector::of_module(target.clone(), Some(&"mir".to_owned()));
        let args = one.cargo_args(super::Parallelism::SERIAL);
        assert_eq!(args.last().map(String::as_str), Some("mir::"), "{args:?}");
        assert!(!args.contains(&"--exact".to_owned()), "{args:?}");
        assert_eq!(one.label(), "nazm-cli --test cli_m_r mir::");
        let whole = super::TestSelector::of_module(target, None);
        assert_eq!(
            whole
                .cargo_args(super::Parallelism::SERIAL)
                .last()
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn the_broad_suite_is_the_legacy_invocation() {
        assert_eq!(
            broad_suite(super::Parallelism::SERIAL),
            [
                "test",
                "--workspace",
                "--jobs",
                "1",
                "--no-fail-fast",
                "--",
                "--test-threads",
                "1"
            ]
        );
    }

    #[test]
    fn the_longest_owning_prefix_chooses_the_profile_and_an_explicit_one_wins() {
        let c = cat(&format!(
            "{ONE}\n[[profile]]\nname = \"wide\"\nwhy = \"w\"\nowns = [\"crates/\"]\n\
             targets = [{{ package = \"a\", lib = true }}]\n\
             [[profile]]\nname = \"narrow\"\nwhy = \"w\"\nowns = [\"crates/a/\"]\n\
             targets = [{{ package = \"a\", lib = true }}]\n"
        ));
        assert_eq!(
            owner(&c.profile, "crates/a/src/x.rs"),
            Some(("narrow".to_owned(), "crates/a/".to_owned()))
        );
        assert_eq!(owner(&c.profile, "compiler/x.nz"), None);
        let p = plan(&c, &[0]).expect("plan");
        let (name, source, _) = p[0].profile.clone().expect("profile");
        assert_eq!(name, "narrow");
        assert_eq!(source, ProfileSource::Owner("crates/a/".to_owned()));

        let mut c = c;
        c.mutation[0].profile = Some("wide".to_owned());
        let p = plan(&c, &[0]).expect("plan");
        assert_eq!(
            p[0].profile.clone().expect("profile").1,
            ProfileSource::Explicit
        );
    }

    #[test]
    fn an_undeclared_profile_is_refused() {
        let c = cat(&format!(
            "{ONE}profile = \"nope\"\nno_killer = \"because\"\n"
        ));
        assert!(plan(&c, &[0]).is_err());
    }

    #[test]
    fn a_selection_of_nothing_is_an_error() {
        let c = cat(ONE);
        assert!(select(&c, &["zzz".to_owned()]).is_err());
        assert_eq!(select(&c, &[]).unwrap(), vec![0]);
        assert_eq!(select(&c, &["m".to_owned()]).unwrap(), vec![0]);
    }

    #[test]
    fn an_exact_filter_selects_one_name_and_not_the_names_containing_it() {
        assert!(super::answers("vec-get", "vec-get"));
        assert!(super::answers("selfhost-vec-get", "vec-get"));
        assert!(super::answers("vec-get", "=vec-get"));
        assert!(!super::answers("selfhost-vec-get", "=vec-get"));
        assert!(!super::answers("vec-get-x", "=vec-get"));
    }

    /// A tiny workspace on disk, for the resolution checks.
    fn workspace() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::write(r.join("Cargo.toml"), "[workspace]\nmembers = [\"a\"]\n").unwrap();
        fs::create_dir_all(r.join("a/src/deep")).unwrap();
        fs::create_dir_all(r.join("a/tests")).unwrap();
        fs::write(r.join("a/Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        fs::write(
            r.join("a/src/lib.rs"),
            "mod deep;\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn top() {}\n\n    #[test]\n    #[ignore = \"slow\"]\n    fn slow() {}\n}\n",
        )
        .unwrap();
        fs::write(
            r.join("a/src/deep.rs"),
            "#[cfg(test)]\nmod tests {\n    /// doc\n    #[test]\n    fn inner() {}\n}\n",
        )
        .unwrap();
        fs::write(r.join("a/tests/it.rs"), "#[test]\nfn whole() {}\n").unwrap();
        d
    }

    fn with_killer(root: &std::path::Path, k: &str) -> Vec<String> {
        let c = cat(&format!("{ONE}killers = [{k}]\n"));
        validate(root, &c)
    }

    #[test]
    fn killers_that_exist_validate() {
        let w = workspace();
        for k in [
            r#"{ package = "a", lib = true, name = "tests::top" }"#,
            r#"{ package = "a", lib = true, name = "deep::tests::inner" }"#,
            r#"{ package = "a", test = "it", name = "whole" }"#,
        ] {
            assert!(
                with_killer(w.path(), k).is_empty(),
                "{k}: {:?}",
                with_killer(w.path(), k)
            );
        }
    }

    #[test]
    fn stale_killer_metadata_fails_validation_before_any_campaign() {
        let w = workspace();
        for (k, says) in [
            (
                r#"{ package = "a", lib = true, name = "tests::gone" }"#,
                "no `#[test] fn gone`",
            ),
            (
                r#"{ package = "a", lib = true, name = "nowhere::tests::top" }"#,
                "module `nowhere`",
            ),
            (
                r#"{ package = "a", test = "missing", name = "whole" }"#,
                "no integration test",
            ),
            (
                r#"{ package = "b", lib = true, name = "x" }"#,
                "not a workspace member",
            ),
            (
                r#"{ package = "a", bin = "tool", name = "x" }"#,
                "no binary",
            ),
            (
                r#"{ package = "a", lib = true, name = "tests::slow" }"#,
                "ignore",
            ),
        ] {
            let bad = with_killer(w.path(), k);
            assert!(bad.iter().any(|b| b.contains(says)), "{k}: {bad:?}");
        }
    }

    #[test]
    fn a_duplicate_name_or_killer_is_refused() {
        let w = workspace();
        let c = cat(&format!("{ONE}{ONE}"));
        assert!(validate(w.path(), &c)
            .iter()
            .any(|b| b.contains("more than one")));
        let k = r#"{ package = "a", test = "it", name = "whole" }"#;
        let bad = with_killer(w.path(), &format!("{k}, {k}"));
        assert!(bad.iter().any(|b| b.contains("listed twice")), "{bad:?}");
    }

    #[test]
    fn a_profile_without_a_killer_must_say_why() {
        let w = workspace();
        let base = format!(
            "{ONE}profile = \"p\"\n\n[[profile]]\nname = \"p\"\nwhy = \"w\"\n\
             targets = [{{ package = \"a\", lib = true }}]\n"
        );
        let bad = validate(w.path(), &cat(&base));
        assert!(
            bad.iter().any(|b| b.contains("does not say why")),
            "{bad:?}"
        );
        let fixed = base.replace(
            "profile = \"p\"\n\n",
            "profile = \"p\"\nno_killer = \"only visible end to end\"\n\n",
        );
        assert!(
            validate(w.path(), &cat(&fixed)).is_empty(),
            "{:?}",
            validate(w.path(), &cat(&fixed))
        );
    }

    #[test]
    fn an_entry_without_a_killer_must_say_why() {
        let w = workspace();
        let bad = validate(w.path(), &cat(ONE));
        assert!(bad.iter().any(|b| b.contains("has no killer")), "{bad:?}");
        let said = cat(&format!("{ONE}no_killer = \"a known survivor\"\n"));
        assert!(validate(w.path(), &said).is_empty());
        let blank = cat(&format!("{ONE}no_killer = \"  \"\n"));
        assert!(!validate(w.path(), &blank).is_empty());
    }

    #[test]
    fn two_profiles_cannot_own_the_same_prefix() {
        let w = workspace();
        let c = cat(&format!(
            "{ONE}\n[[profile]]\nname = \"p\"\nwhy = \"w\"\nowns = [\"a/\"]\n\
             targets = [{{ package = \"a\", lib = true }}]\n\
             [[profile]]\nname = \"q\"\nwhy = \"w\"\nowns = [\"a/\"]\n\
             targets = [{{ package = \"a\", lib = true }}]\n"
        ));
        assert!(validate(w.path(), &c)
            .iter()
            .any(|b| b.contains("both own")));
    }

    #[test]
    fn the_static_resolver_reads_attributes_through_doc_comments() {
        let w = workspace();
        assert!(find_test(&w.path().join("a/src/lib.rs"), "deep::tests::inner").is_ok());
    }
}
