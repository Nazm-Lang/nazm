//! R1e, S4: `@std/args` (`docs/spec.md`, *The standard library*).
//!
//! One specification shaped like a toolchain's — global flags, sub-commands nested two deep,
//! options with defaults, a required option, repeated options, a conflict, required, optional and
//! trailing positional arguments — read against every form a command line takes (`--name value`,
//! `--name=value`, `-n value`, `-nvalue`, bundled `-abc`, `--`, `-`) and against every mistake,
//! with the generated help; the same under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and
//! Cranelift, with nothing live. And a built executable reading the arguments it was started with.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The specification, shared by both programs.
const SPEC: &str = r#####"use "@std/args";

fn spec() -> ArgCommand ! {} {
    let cli = args_program("tool", "A toolchain");
    args_flag(cli, "verbose", "v", "Say more");
    args_option(cli, "color", "", "WHEN", "When to colour output");
    let build = args_command(cli, "build", "Build a program");
    args_option_default(build, "opt-level", "O", "N", "0", "Optimisation level");
    args_option(build, "output", "o", "FILE", "Where to write");
    args_option_required(build, "target", "t", "TRIPLE", "The target");
    args_option(build, "define", "D", "NAME", "A feature to enable");
    args_flag(build, "json", "j", "Write JSON");
    args_flag(build, "quiet", "q", "Write nothing");
    args_conflict(build, "json", "quiet");
    args_positional(build, "input", true, "The program");
    args_positional(build, "mode", false, "How to build it");
    args_rest(build, "extra", "More files");
    let pkg = args_command(cli, "pkg", "Work with packages");
    let add = args_command(pkg, "add", "Add a dependency");
    args_flag(add, "dev", "", "Only for development");
    args_positional(add, "name", true, "The package");
    cli
}

fn shown(o: Option[Str]) -> Str { match o { Option.Some(value: v) => f"[{v}]", Option.None() => "none" } }

fn describe(r: Result[Args, Str]) -> Str {
    match r {
        Result.Ok(value: a) => {
            if args_help_wanted(a) { f"help for `{args_subcommand(a)}`" } else {
                f"cmd=`{args_subcommand(a)}` v={args_times(a, "verbose")} color={shown(args_get(a, "color"))} O={shown(args_get(a, "opt-level"))} O-given={args_has(a, "opt-level")} out={shown(args_get(a, "output"))} target={shown(args_get(a, "target"))} D={str_join(args_get_all(a, "define"), ",")} json={args_has(a, "json")} input={shown(args_get(a, "input"))} mode={shown(args_get(a, "mode"))} extra={str_join(args_get_all(a, "extra"), ",")} dev={args_has(a, "dev")} name={shown(args_get(a, "name"))}"
            }
        },
        Result.Err(error: e) => f"error: {e}",
    }
}
"#####;

/// Every case, an argument list split at `|`, and the help of every command.
const CASES: &str = r#####"
fn split(s: Str) -> Strs {
    let out = strs_new();
    if s == "" { return out; }
    let mut from = 0;
    for i in 0..str_len(s) {
        if str_byte(s, i) == 124 { strs_push(out, str_slice(s, from, i)); from = i + 1; }
    }
    strs_push(out, str_slice(s, from, str_len(s)));
    out
}

fn help_of(cli: ArgCommand, c: Str) -> Str {
    match args_parse(cli, split(f"{c}|--help")) {
        Result.Ok(value: a) => args_help_text(a),
        Result.Err(error: e) => f"error: {e}\n",
    }
}

fn main(io: IoCap) -> Int ! { io } {
    let cases = strs_new();
    strs_push(cases, "");
    strs_push(cases, "-v|-v|--color=never");
    strs_push(cases, "build|-t|x86|in.nz");
    strs_push(cases, "build|--target=x86|-O2|--output|a.out|-DA|-D|B|--define=C|in.nz|fast|x|y");
    strs_push(cases, "--verbose|build|-vjt|arm|-vO3|in.nz");
    strs_push(cases, "build|-t|x|--opt-level=|in.nz");
    strs_push(cases, "build|-t|x|--|-in.nz|--json|-");
    strs_push(cases, "build|-o|-|-t|-v|in.nz");
    strs_push(cases, "pkg|add|--dev|--color|always|json");
    strs_push(cases, "pkg");
    strs_push(cases, "--help");
    strs_push(cases, "build|--frob|-h");
    strs_push(cases, "pkg|add|-h|--unknown");
    strs_push(cases, "--frob");
    strs_push(cases, "build|-t|x|-z|in.nz");
    strs_push(cases, "build|-t|x|-vzq|in.nz");
    strs_push(cases, "build|-t");
    strs_push(cases, "build|in.nz|--output");
    strs_push(cases, "build|-t|x|--json=yes|in.nz");
    strs_push(cases, "build|-t|x|-j|-q|in.nz");
    strs_push(cases, "build|in.nz");
    strs_push(cases, "build|-t|x");
    strs_push(cases, "biuld");
    strs_push(cases, "pkg|add");
    strs_push(cases, "pkg|add|a|b");
    strs_push(cases, "pkg|add|-é");
    for i in 0..strs_len(cases) {
        let c = strs_get(cases, i);
        print(f"{c} => {describe(args_parse(spec(), split(c)))}\n");
    }
    let cli = spec();
    print(args_help(cli));
    print(help_of(cli, "build"));
    print(help_of(cli, "pkg|add"));
    let wrong = args_program("bad", "");
    args_flag(wrong, "x", "x", "");
    args_option(wrong, "y", "x", "Y", "");
    print(f"{describe(args_parse(wrong, split("")))}\n");
    let late = args_program("late", "");
    args_positional(late, "a", false, "");
    args_positional(late, "b", true, "");
    print(f"{describe(args_parse(late, split("")))}\n");
    print(args_usage(late));
    print("\n");
    0
}
"#####;

const EXPECTED: &str = r#####" => cmd=`` v=0 color=none O=none O-given=false out=none target=none D= json=false input=none mode=none extra= dev=false name=none
-v|-v|--color=never => cmd=`` v=2 color=[never] O=none O-given=false out=none target=none D= json=false input=none mode=none extra= dev=false name=none
build|-t|x86|in.nz => cmd=`build` v=0 color=none O=[0] O-given=false out=none target=[x86] D= json=false input=[in.nz] mode=none extra= dev=false name=none
build|--target=x86|-O2|--output|a.out|-DA|-D|B|--define=C|in.nz|fast|x|y => cmd=`build` v=0 color=none O=[2] O-given=true out=[a.out] target=[x86] D=A,B,C json=false input=[in.nz] mode=[fast] extra=x,y dev=false name=none
--verbose|build|-vjt|arm|-vO3|in.nz => cmd=`build` v=3 color=none O=[3] O-given=true out=none target=[arm] D= json=true input=[in.nz] mode=none extra= dev=false name=none
build|-t|x|--opt-level=|in.nz => cmd=`build` v=0 color=none O=[] O-given=true out=none target=[x] D= json=false input=[in.nz] mode=none extra= dev=false name=none
build|-t|x|--|-in.nz|--json|- => cmd=`build` v=0 color=none O=[0] O-given=false out=none target=[x] D= json=false input=[-in.nz] mode=[--json] extra=- dev=false name=none
build|-o|-|-t|-v|in.nz => cmd=`build` v=0 color=none O=[0] O-given=false out=[-] target=[-v] D= json=false input=[in.nz] mode=none extra= dev=false name=none
pkg|add|--dev|--color|always|json => cmd=`pkg add` v=0 color=[always] O=none O-given=false out=none target=none D= json=false input=none mode=none extra= dev=true name=[json]
pkg => cmd=`pkg` v=0 color=none O=none O-given=false out=none target=none D= json=false input=none mode=none extra= dev=false name=none
--help => help for ``
build|--frob|-h => error: tool build: unknown option `--frob`
pkg|add|-h|--unknown => help for `pkg add`
--frob => error: tool: unknown option `--frob`
build|-t|x|-z|in.nz => error: tool build: unknown option `-z`
build|-t|x|-vzq|in.nz => error: tool build: unknown option `-z` in `-vzq`
build|-t => error: tool build: `-t` needs a value `<TRIPLE>`
build|in.nz|--output => error: tool build: `--output` needs a value `<FILE>`
build|-t|x|--json=yes|in.nz => error: tool build: `--json` takes no value, and was given `--json=yes`
build|-t|x|-j|-q|in.nz => error: tool build: `--json` and `--quiet` cannot be used together
build|in.nz => error: tool build: missing the option `--target <TRIPLE>`
build|-t|x => error: tool build: missing the argument `<input>`
biuld => error: tool: unknown command `biuld`
pkg|add => error: tool pkg add: missing the argument `<name>`
pkg|add|a|b => error: tool pkg add: unexpected argument `b`
pkg|add|-é => error: tool pkg add: unknown option `-é`
tool: A toolchain

Usage: tool [options] <command>

Options:
  -v, --verbose       Say more
      --color <WHEN>  When to colour output
  -h, --help          Print this help

Commands:
  build  Build a program
  pkg    Work with packages
tool build: Build a program

Usage: tool build [options] --target <TRIPLE> <input> [<mode>] [<extra>...]

Arguments:
  <input>     The program
  <mode>      How to build it
  <extra>...  More files

Options:
  -O, --opt-level <N>    Optimisation level [default: 0]
  -o, --output <FILE>    Where to write
  -t, --target <TRIPLE>  The target (required)
  -D, --define <NAME>    A feature to enable
  -j, --json             Write JSON
  -q, --quiet            Write nothing
  -v, --verbose          Say more
      --color <WHEN>     When to colour output
  -h, --help             Print this help
tool pkg add: Add a dependency

Usage: tool pkg add [options] <name>

Arguments:
  <name>  The package

Options:
      --dev           Only for development
  -v, --verbose       Say more
      --color <WHEN>  When to colour output
  -h, --help          Print this help
error: bad: the specification is wrong: `-x` is declared twice
error: late: the specification is wrong: the required `<b>` follows an optional one
Usage: late [options] [<a>] <b>
0
"#####;

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn has_nothing_live(label: &str, r: &Ran) {
    let line = r
        .stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("{label}: no memory report: {r:?}"));
    assert!(
        line.split_whitespace()
            .filter(|w| w.starts_with("live="))
            .all(|w| w == "live=0"),
        "{label}: {line}"
    );
}

fn build(dir: &Path, label: &str, backend: &str, opt: &str) -> std::path::PathBuf {
    let exe = dir.join(crate::common::exe_name(label.replace([' ', '-'], "")));
    let b = nazm(
        dir,
        &[
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--opt-level",
            opt,
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ],
    );
    assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
    exe
}

const NATIVE: [(&str, &str, &str); 3] = [
    ("llvm -O0", "llvm", "0"),
    ("llvm -O2", "llvm", "2"),
    ("cranelift", "cranelift", "0"),
];

#[test]
fn every_form_and_every_mistake_parse_alike_everywhere() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), format!("{SPEC}{CASES}")).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        runs.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in NATIVE {
        let exe = build(d.path(), label, backend, opt);
        runs.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe)
                    .current_dir(d.path())
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, r) in runs {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, EXPECTED, "{label}");
        if label != "lir oracle" {
            has_nothing_live(&label, &r);
        }
    }
}

/// The program a tool would be: its own command line, help on standard output, an error on
/// standard error with exit status 2.
const MAIN: &str = r#####"use "@std/process";

fn main(io: IoCap) -> Int ! { io } {
    match args_parse(spec(), process_args(io)) {
        Result.Ok(value: a) => {
            if args_help_wanted(a) { print(args_help_text(a)); } else { print(f"{describe(Result[Args, Str].Ok(value: a))}\n"); }
            0
        },
        Result.Err(error: e) => { eprint(f"{e}\n"); exit_with(2) },
    }
}
"#####;

#[test]
fn a_built_executable_parses_the_arguments_it_was_started_with() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), format!("{SPEC}{MAIN}")).unwrap();
    for (label, backend, opt) in [NATIVE[1], NATIVE[2]] {
        let exe = build(d.path(), label, backend, opt);
        let run = |args: &[&str]| {
            run_bounded(
                common::bounded_exe(&exe)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            )
        };
        let r = run(&[
            "build",
            "--target",
            "wasm32 wasi",
            "-vvO2",
            "-D",
            "é",
            "in put.nz",
            "--",
            "--x",
        ]);
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(
            r.stdout,
            "cmd=`build` v=2 color=none O=[2] O-given=true out=none target=[wasm32 wasi] D=é json=false input=[in put.nz] mode=[--x] extra= dev=false name=none\n0\n",
            "{label}"
        );
        has_nothing_live(label, &r);
        let r = run(&["pkg", "add", "--help"]);
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout.starts_with(
                "tool pkg add: Add a dependency\n\nUsage: tool pkg add [options] <name>\n"
            ),
            "{label}: {}",
            r.stdout
        );
        let r = run(&["build", "--target"]);
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert!(r.stdout.is_empty(), "{label}: {r:?}");
        assert!(
            r.stderr
                .starts_with("tool build: `--target` needs a value `<TRIPLE>`\n"),
            "{label}: {}",
            r.stderr
        );
    }
    // The interpreter hands a program the same arguments after `--`.
    let r = nazm(
        d.path(),
        &[
            "run",
            "--interpret",
            "prog.nz",
            "--",
            "pkg",
            "add",
            "--dev",
            "x",
        ],
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(
        r.stdout,
        "cmd=`pkg add` v=0 color=none O=none O-given=false out=none target=none D= json=false input=none mode=none extra= dev=true name=[x]\n0\n"
    );
}
