//! Restriction profiles on the command line (N47, `docs/architecture.md` §7.49).
//!
//! The profiles in force are the one `--profile` names and every one a package manifest in the
//! build requires; their rules are evaluated over the facts a full check settles
//! (`nazm_service::profile`), before the command does its own work. `general`, alone and with
//! no report asked for, costs nothing: no second check is made.

use std::path::Path;
use std::process::ExitCode;

use nazm_diag::{codes, Diagnostic};
use nazm_service::profile::{self, Profile};
use nazm_span::Span;

use crate::exit;

/// The configuration `@cfg` is decided under (Gate 2, §17): the target's operating system and
/// architecture — the host's when no triple is named — and every profile the command is held
/// to, `--profile`'s and each package manifest's. A name no profile has is [`enforce`]'s to
/// refuse; here it simply holds nothing.
///
/// The features are each package's own (§22): its `default`, what its dependents ask of it, and
/// `--features` of the root. A feature no package declares is refused here, before anything is
/// checked; `--features` outside a package is a usage error.
pub(crate) fn configuration(
    named_path: &Path,
    named: Option<&str>,
    triple: Option<&str>,
    features: &[String],
) -> Result<nazm_core::sema::BuildConfig, ExitCode> {
    let mut config = triple.map_or_else(
        nazm_core::sema::BuildConfig::host,
        nazm_core::sema::BuildConfig::for_target,
    );
    config.profiles.extend(named.map(str::to_owned));
    match crate::package::profile_graph(named_path) {
        Some(mut g) => {
            config
                .profiles
                .extend(g.packages.iter().filter_map(|p| p.profile.clone()));
            if let Err(ds) = g.enable(features) {
                for d in &ds {
                    eprintln!("error[{}]: {}", d.code, d.message);
                }
                return Err(ExitCode::from(exit::CHECK_FAILED));
            }
            for (i, p) in g.packages.iter().enumerate() {
                if i == g.root {
                    config.features.clone_from(&p.features);
                } else {
                    config
                        .package_features
                        .insert(p.key_prefix(), p.features.clone());
                }
            }
        }
        None if !features.is_empty() => {
            eprintln!("nazm: `--features` enables a package's features, and this program is not in a package");
            return Err(ExitCode::from(exit::USAGE));
        }
        None => {}
    }
    config.profiles.sort();
    config.profiles.dedup();
    Ok(config)
}

fn unknown(name: &str, whose: &str) -> ExitCode {
    let known: Vec<&str> = Profile::ALL.iter().map(|p| p.name()).collect();
    let d = Diagnostic::error(
        codes::UNKNOWN_PROFILE,
        Span::synthetic(),
        format!("{whose} names the profile `{name}`, which does not exist"),
    )
    .help(format!("the profiles are {}", known.join(", ")));
    eprintln!("error[{}]: {}", d.code, d.message);
    if let Some(h) = &d.help {
        eprintln!("  = help: {h}");
    }
    ExitCode::from(exit::CHECK_FAILED)
}

/// Every allocation site whose count per call is unknown, as a profile's witnesses (N52).
/// What the profile rules read off MIR (N60): the unbounded allocations; the functions in a
/// cycle of direct calls and spawns, from MIR's own call edges; and the calls through a function
/// value, whose targets the compiler does not know. MIR's stack checks are not the fact here:
/// they are also placed, conservatively, where an indirect call might close a cycle.
fn mir_facts(program: &nazm_mir::ir::Program) -> profile::MirFacts {
    let n = program.functions.len();
    let index = |id: nazm_mir::ir::FuncId| program.functions.iter().position(|f| f.id == id);
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indirect = Vec::new();
    for (i, f) in program.functions.iter().enumerate() {
        for b in &f.blocks {
            for s in &b.stmts {
                match &s.kind {
                    nazm_mir::ir::StatementKind::Assign {
                        value: nazm_mir::ir::Rvalue::Call { callee, .. },
                        ..
                    }
                    | nazm_mir::ir::StatementKind::Spawn { callee, .. } => {
                        if let (Some(j), Some(e)) = (index(*callee), edges.get_mut(i)) {
                            e.push(j);
                        }
                    }
                    nazm_mir::ir::StatementKind::Assign {
                        value: nazm_mir::ir::Rvalue::CallIndirect { .. },
                        ..
                    } => indirect.push(profile::Witness {
                        span: s.at,
                        note: format!("`{}` calls a function value here", f.name),
                    }),
                    _ => {}
                }
            }
        }
    }
    // A function is in a cycle when it reaches itself: a search from each, bounded by `n`.
    let mut recursive = Vec::new();
    for (i, f) in program.functions.iter().enumerate() {
        let mut seen = vec![false; n];
        let mut stack: Vec<usize> = edges.get(i).cloned().unwrap_or_default();
        let mut cycle = false;
        while let Some(j) = stack.pop() {
            if j == i {
                cycle = true;
                break;
            }
            if seen.get(j).copied().unwrap_or(true) {
                continue;
            }
            if let Some(s) = seen.get_mut(j) {
                *s = true;
            }
            stack.extend(edges.get(j).into_iter().flatten().copied());
        }
        if cycle {
            recursive.push(profile::Witness {
                span: f.span,
                note: format!("`{}` is in a call cycle", f.name),
            });
        }
    }
    // The longest chain of direct calls from `main` (N63): only where there is no cycle and no
    // call through a value, so the search below terminates and means what it says.
    let call_depth = (recursive.is_empty() && indirect.is_empty())
        .then(|| program.entry.and_then(index))
        .flatten()
        .map(|root| depth(root, &edges, &mut vec![None; n]));
    let sites = nazm_mir::cost::of(program);
    let allocation_sites = sites
        .iter()
        .flat_map(|fc| fc.sites.iter())
        .filter(|s| s.kind == nazm_mir::cost::SiteKind::Allocation)
        .count();
    let task_sites = program
        .functions
        .iter()
        .flat_map(|f| f.blocks.iter().flat_map(|b| b.stmts.iter()))
        .filter(|s| matches!(s.kind, nazm_mir::ir::StatementKind::Spawn { .. }))
        .count();
    profile::MirFacts {
        unbounded: unbounded_allocations(program),
        recursive,
        indirect,
        bounds: profile::Bounds {
            call_depth,
            allocation_sites,
            task_sites,
            ..profile::Bounds::default()
        },
        unsigned_writes: Vec::new(),
        allocations: allocations(program),
        feeding_tasks: feeding_tasks(program),
    }
}

/// The `@task`s that call `watchdog_feed` themselves (Gate 3): what `watchdog` asks for.
fn feeding_tasks(program: &nazm_mir::ir::Program) -> Vec<String> {
    program
        .functions
        .iter()
        .filter(|f| f.task.is_some())
        .filter(|f| {
            f.blocks.iter().flat_map(|b| b.stmts.iter()).any(|s| {
                matches!(
                    &s.kind,
                    nazm_mir::ir::StatementKind::Assign {
                        value: nazm_mir::ir::Rvalue::Primitive {
                            op: nazm_core::sema::Intrinsic::WatchdogFeed,
                            ..
                        },
                        ..
                    }
                )
            })
        })
        .map(|f| f.name.clone())
        .collect()
}

/// The longest chain of calls from `f`, counting `f`, over an acyclic graph.
fn depth(f: usize, edges: &[Vec<usize>], memo: &mut Vec<Option<usize>>) -> usize {
    if let Some(Some(d)) = memo.get(f) {
        return *d;
    }
    let d = 1 + edges
        .get(f)
        .into_iter()
        .flatten()
        .map(|&g| depth(g, edges, memo))
        .max()
        .unwrap_or(0);
    if let Some(m) = memo.get_mut(f) {
        *m = Some(d);
    }
    d
}

/// Every allocation site of the program's own functions (Gate 3): what `no-heap` refuses — a
/// site in a helper the program calls is the helper's, and reported there.
fn allocations(program: &nazm_mir::ir::Program) -> Vec<profile::Witness> {
    nazm_mir::cost::of(program)
        .into_iter()
        .flat_map(|fc| {
            let name = program
                .function(fc.func)
                .map(|f| f.name.clone())
                .unwrap_or_default();
            fc.sites
                .into_iter()
                .filter(|s| s.kind == nazm_mir::cost::SiteKind::Allocation)
                .map(move |s| profile::Witness {
                    span: s.at,
                    note: format!("`{}` allocates here, in `{name}`", s.op),
                })
        })
        .collect()
}

fn unbounded_allocations(program: &nazm_mir::ir::Program) -> Vec<profile::Witness> {
    // A function's own sites, and its calls that may allocate through any chain of calls: an
    // allocation moved into a helper, a trait method or a generic is still one (Q1-C-01).
    nazm_mir::cost::of(program)
        .into_iter()
        .chain(nazm_mir::cost::allocating_calls(program))
        .flat_map(|fc| {
            let name = program
                .function(fc.func)
                .map(|f| f.name.clone())
                .unwrap_or_default();
            fc.sites
                .into_iter()
                .filter(|s| {
                    s.kind == nazm_mir::cost::SiteKind::Allocation
                        && s.count == nazm_mir::cost::Count::Unknown
                })
                .map(move |s| profile::Witness {
                    span: s.at,
                    note: format!("`{}` allocates here in a loop of `{name}`", s.op),
                })
        })
        .collect()
}

/// Hold the program rooted at `file` to the profiles in force.
///
/// # Errors
///
/// The exit code, once reported: an unknown profile (`N0511`), or a rule a profile in force
/// refuses (`N0510`, one per rule).
pub(crate) fn enforce(
    file: &Path,
    named_path: &Path,
    named: Option<&str>,
    locked_flag: bool,
    report: bool,
) -> Result<(), ExitCode> {
    let mut profiles = Vec::new();
    if let Some(n) = named {
        profiles.push(Profile::by_name(n).ok_or_else(|| unknown(n, "`--profile`"))?);
    }
    // The package graph of the path the command was given — the one it resolved and builds —
    // not one re-derived from the entry file: a `nazm.root` beside the entry is nearer than the
    // manifest, and re-deriving from it dropped the manifest's profile (Q1-C-04).
    let graph = crate::package::profile_graph(named_path);
    if let Some(g) = &graph {
        for p in &g.packages {
            if let Some(n) = &p.profile {
                let prof = Profile::by_name(n)
                    .ok_or_else(|| unknown(n, &format!("the package `{}`'s manifest", p.name)))?;
                profiles.push(prof);
            }
        }
    }
    let strict = profiles.iter().any(|p| *p != Profile::General);
    if !strict && !report {
        return Ok(());
    }
    // A package build held to its lockfile: `--locked` already verified it; a profile that
    // requires it verifies it here, so the flag is not needed to satisfy the rule.
    let locked = graph
        .as_ref()
        .is_some_and(|g| locked_flag || nazm_package::verify_locked(g).is_ok());
    let Ok(root) = nazm_service::root::decide(file, None) else {
        return Ok(());
    };
    let Ok(loaded) = nazm_service::load::load(file, root.path()) else {
        // The command reports what could not be loaded, in its own words.
        return Ok(());
    };
    let map = &loaded.map;
    // A rule over resource facts reads MIR (N52): lowered only when one is in force.
    let needs_cost = profiles.iter().any(|p| {
        p.rules().contains(&profile::Rule::BoundedAllocation)
            || p.rules().contains(&profile::Rule::NoRecursion)
            || p.rules().contains(&profile::Rule::NoUnknownCalls)
            || p.rules().contains(&profile::Rule::NoHeap)
            || p.rules().contains(&profile::Rule::BoundedStack)
            || p.rules().contains(&profile::Rule::BoundedLoops)
            || p.rules().contains(&profile::Rule::SignedWrites)
            || p.rules().contains(&profile::Rule::Watchdog)
    });
    let evaluated = if needs_cost {
        nazm_core::on_lowered(
            map,
            &loaded.graph,
            nazm_core::Entry::Required,
            |core, res| {
                // Loops are read off Core IR (§7.65), before it is lowered.
                let loops = nazm_service::bounds::loops(core);
                let queue_sites = res
                    .functions()
                    .iter()
                    .flat_map(|f| f.call_sites.iter())
                    .filter(|s| {
                        matches!(
                            s.callee,
                            nazm_core::sema::Callee::Builtin(
                                nazm_core::sema::Intrinsic::ChanNew
                                    | nazm_core::sema::Intrinsic::ChanNewOf
                            )
                        )
                    })
                    .count();
                let facts = crate::mir::lower(core, res, &loaded.graph)
                    .ok()
                    .map(|program| {
                        let mut f = mir_facts(&program);
                        f.bounds.loops = loops;
                        f.bounds.queue_sites = queue_sites;
                        // A contract's unguarded writes (N72): only where the module is one.
                        let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
                        if let Ok(c) = nazm_service::contract::of(core, res, root) {
                            f.unsigned_writes = c
                                .entrypoints
                                .iter()
                                .filter(|e| {
                                    !e.caller_checked
                                        && e.writes.as_ref().is_none_or(|w| !w.is_empty())
                                })
                                .map(|e| profile::Witness {
                                    span: e.at,
                                    note: format!(
                                        "`{}` writes the state and never decides on its caller",
                                        e.name
                                    ),
                                })
                                .collect();
                        }
                        f
                    });
                Ok(profile::evaluate(
                    &profiles,
                    &loaded.graph,
                    res,
                    locked,
                    facts.as_ref(),
                ))
            },
        )
    } else {
        nazm_core::on_checked_map(map, &loaded.graph, |_, res| {
            Ok(profile::evaluate(
                &profiles,
                &loaded.graph,
                res,
                locked,
                None,
            ))
        })
    };
    let Ok(evaluation) = evaluated else {
        // A program that does not check is refused by the command, which says why.
        return Ok(());
    };
    if report {
        println!("{}", profile::report(&evaluation, map));
    }
    if evaluation.accepted() {
        return Ok(());
    }
    let diagnostics = profile::diagnostics(&evaluation);
    crate::emit(&diagnostics, map, false);
    let n = diagnostics.len();
    eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
    Err(ExitCode::from(exit::CHECK_FAILED))
}
