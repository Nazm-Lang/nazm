# M1b source changes

Generated 2026-09-20T17:20:52Z. Contains the full diff of tracked files
plus the complete contents of every new untracked file, so the adviser does not have
to reconstruct anything from a transcript.

## Tracked files — diff

```diff
diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml
index d2a5a95..9102083 100644
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -41,6 +41,13 @@ jobs:
           NAZM_REQUIRE_NONUTF8: "1"
         run: cargo test --workspace --locked
 
+      # `cargo test` does not run these: they drive nested cargo builds and kill real
+      # process groups, and `cargo xtask mutate` below would pay their cost 44 times
+      # over. Ignored by default and run here, so they are executed rather than merely
+      # present.
+      - name: mutation-runner lifecycle
+        run: cargo test -p xtask --test lifecycle -- --ignored --test-threads=1
+
       - name: project checks
         run: cargo xtask check
 
diff --git a/Cargo.toml b/Cargo.toml
index e2fda9f..8d26974 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,12 +1,16 @@
 # Nazm — an AI-native programming language.
 #
+# Direction: docs/direction.md · Roadmap: docs/user/roadmap.md
 # Design record: docs/internal/architecture.md · Spec: docs/user/spec.md
-# Evaluation protocol and pre-registered decision rule: docs/evaluation.md
-# Plan: ~/.claude/plans/in-my-download-folder-sparkling-minsky.md
+# Evaluation protocol (draft, not pre-registered): docs/evaluation.md
 #
-# Phase 2 is the whole workspace right now: a benchmark harness and the
-# architectural gates. There is deliberately no compiler here yet — the harness
-# has to work, and be shown to work, before the language is worth building.
+# Two tracks in one workspace. The language — span, diag, syntax, core, cli — is
+# an interpreter: lexer, parser, checker, tree-walking evaluator, no IR and no
+# backend. Alongside it, nazm-bench, an agent-cost harness that now measures
+# specific claims rather than gating the project's existence (docs/direction.md).
+#
+# This header previously read "there is deliberately no compiler here yet". That
+# stopped being true on 2026-09-20, when a .nz program first executed.
 
 [workspace]
 resolver = "2"
@@ -15,6 +19,7 @@ members = [
     "crates/nazm-diag",
     "crates/nazm-syntax",
     "crates/nazm-core",
+    "crates/nazm-lir",
     "crates/nazm-cli",
     "crates/nazm-bench",
     "xtask",
@@ -24,10 +29,13 @@ members = [
 version = "0.0.1"
 edition = "2021"
 # Pinned toolchain is 1.98.1 (see rust-toolchain.toml). This floor is lower on
-# purpose: it is the oldest compiler the Phase 2 crates actually need, and CI
+# purpose: it is the oldest compiler the *shipped* crates actually need, and CI
 # verifies the claim rather than trusting it. The floor will rise to 1.95+ when
 # cranelift arrives in Stage 7 — at which point this line is the thing that has
 # to change deliberately.
+#
+# xtask overrides it with 1.89: it needs std's file locking, and a development
+# tool's requirement has no business raising the floor for the crates that ship.
 rust-version = "1.85"
 license = "Apache-2.0"
 repository = "https://github.com/mdferdousalam/nazm"
@@ -52,6 +60,7 @@ nazm-span = { path = "crates/nazm-span" }
 nazm-diag = { path = "crates/nazm-diag" }
 nazm-syntax = { path = "crates/nazm-syntax" }
 nazm-core = { path = "crates/nazm-core" }
+nazm-lir = { path = "crates/nazm-lir" }
 nazm-bench = { path = "crates/nazm-bench" }
 
 # --- cli / config ---
diff --git a/README.md b/README.md
index fc793e8..7930ada 100644
--- a/README.md
+++ b/README.md
@@ -1,7 +1,7 @@
 # Nazm
 
-**An AI-native programming language — or, first, an experiment to find out whether one is
-worth building.**
+**An AI-native programming language: fast, memory-efficient, concurrent — with a compiler
+eventually written in Nazm itself.**
 
 > نظم — *systematic order, well-formed arrangement.*
 
@@ -10,20 +10,42 @@ writes most code, languages should be redesigned to save tokens. The evidence sa
 otherwise. What the evidence *does* show is that agents waste enormous effort fighting
 compilers and churning over code that already works — and that is attackable.
 
-So this repository contains two things, in the order that matters. First, the instrument
-that decides whether the language should exist. Second, a small executable language to
-point it at — an interpreter, not a compiler, which is why nothing here speaks to the
-performance claims yet.
+So this repository contains two things. **The language** — today an interpreter, not a
+compiler, which is why nothing here speaks to the performance claims yet. And **an
+agent-cost harness** built to measure specific claims about it honestly, including the
+uncomfortable one: that better tooling on an existing language might capture much of the
+benefit.
+
+The language is the primary track. That was decided on 2026-09-20 and recorded in
+[`docs/direction.md`](docs/direction.md); what comes next, in dependency order, is
+[`docs/user/roadmap.md`](docs/user/roadmap.md). The harness keeps its protocol, its thresholds and
+its control arm unchanged — it is now evidence for claims rather than a gate on the
+project's existence.
 
 ## What is actually here
 
-**A `.nz` program runs.**
+**A `.nz` program runs — and a subset of one now compiles.**
 
 ```sh
 $ cargo run -p nazm-cli --bin nazm -- run examples/max.nz
 25
+
+$ cargo run -p nazm-cli --bin nazm -- build arithmetic.nz -o arithmetic
+arithmetic
+$ ./arithmetic          # no interpreter, no nazm process
+-54
 ```
 
+`nazm build` is the native path (**M1a + M1b**) and its subset is narrow on purpose: one
+`fn main() -> Int`, `Int` and `Bool` literals, immutable `let` bindings of either, unary
+`-`, `+ - * / %`, the comparisons `== != < <= > >=`, and `if`/`else` **used as a value**,
+with branch-local bindings and nesting. Everything else — functions, loops, mutation,
+`return`, `Str` — is **refused by name** after ordinary checking, with no partial
+compilation, and still runs under `nazm run`. The arithmetic the specification promises
+is preserved rather than inherited from the hardware: overflow is a checked intrinsic and
+a branch (never `nsw`, which would let the optimiser delete the check), `Int::MIN / -1`
+fails, and `Int::MIN % -1` is `0`. See [`docs/user/roadmap.md`](docs/user/roadmap.md).
+
 ```nazm
 fn first_pair_with_product(target: Int, limit: Int) -> Int {
     let mut a = 2;
@@ -90,9 +112,9 @@ development fixtures — containers, a restricted database role, verified cleanu
 produce a benchmark number yet: there is no task corpus, no agent driver, and no provider
 adapter.
 
-The harness came first on purpose. The decisive experiment sat at month 30 in the first
-version of this plan; it now sits at roughly month 13, and the instrument has to be shown
-to work before anything it measures means anything.
+The harness was built first, and that was the right order: an instrument has to be shown
+to work before anything it measures means anything. It is now maintained rather than
+extended, until the claims in the roadmap need it.
 
 ## The argument
 
@@ -117,12 +139,15 @@ churn. None of that is fixed by shorter keywords. All of it is attackable by mak
 programs unrepresentable, closing the check loop in milliseconds, and giving formatting no
 degrees of freedom to churn over.
 
-**Whether that is worth a new language, or just better tooling for TypeScript, is the
-question this repository is built to answer** — and the answer may well be the second one.
+**Whether that is worth a new language, or just better tooling for TypeScript, is a real
+question and the harness is built to keep answering it honestly** — see
+[`docs/direction.md`](docs/direction.md) for what changed about how the answer is used.
 
 ## The experiment
 
-Three arms. The third is the hypothesis; the second is why the experiment is honest.
+Three arms. The third is the hypothesis; the second is why the experiment is honest. It
+measures specific claims — it no longer decides whether the language gets built
+([`docs/direction.md`](docs/direction.md)).
 
 | Arm | Contents |
 | --- | --- |
@@ -130,18 +155,20 @@ Three arms. The third is the hypothesis; the second is why the experiment is hon
 | **A2** | **TypeScript + improved agent tooling** — structured diagnostics, canonical formatter, repeat-edit detection |
 | A3 | Nazm + comparable tooling |
 
-If A2 captures most of the benefit, the right deliverable is better TypeScript tooling and
-Nazm should not be built. The decision rule is written down and committed **before** the
-evaluation runs — see [`docs/evaluation.md`](docs/evaluation.md).
+If A2 captures most of the benefit, that is a finding about the claim under test and it
+stays in the record. The decision rule, its thresholds and the cost policy are unchanged,
+and are written down **before** the evaluation runs — see
+[`docs/evaluation.md`](docs/evaluation.md).
 
 First workload: small HTTP/JSON services on PostgreSQL — validation, transactions,
 authorization, error handling, cancellation. First user: a backend developer using coding
 agents to build and maintain services, so debugging and operations stay in scope rather
 than just "generated programs pass tests".
 
-## The design, if it gets built
+## The design it is being built toward
 
-Documented in [`docs/internal/architecture.md`](docs/internal/architecture.md). In brief: mutable value
+Documented in [`docs/internal/architecture.md`](docs/internal/architecture.md), and staged in
+[`docs/user/roadmap.md`](docs/user/roadmap.md). In brief: mutable value
 semantics so memory safety needs no lifetime annotations; effect rows, capabilities, and
 position-sensitive taint so untrusted data cannot reach a code-construction site;
 structured concurrency where data races are a compile error; and a four-tier compilation
@@ -151,6 +178,19 @@ The answer to "does this need a new compiler design?" is **new front end, new IR
 middle end, reused back end**. Code generation is decades of work with no novelty available
 in it.
 
+**What comes next**, in dependency order and no further:
+
+| | |
+| --- | --- |
+| **M1** | A native vertical slice. The existing lexer, parser, checker and diagnostics, one new typed IR, LLVM IR emitted as text and assembled by `clang`, one target. A compiled program runs with no interpreter, and is differentially compared against the interpreter on the existing 98-case corpus |
+| **M2** | The facilities a compiler actually needs — value/copy/move and cleanup first, then strings, tagged unions, collections, arena-indexed recursive structures, modules, and a narrow capability-based I/O contract |
+| **M3** | The bootstrap: `Rust → C1`, `C1 → C2`, `C2 → C3`, with byte equality asserted only where determinism has been established |
+
+Self-hosting is the long-term objective and it is **M3, not next**. Nazm today has no
+collections, no user-defined types, no pattern matching, no string concatenation and no
+I/O, so a compiler cannot be written in it in any form. The Rust implementation is retained
+as the bootstrap seed and the differential oracle.
+
 ## What this does not establish
 
 Stated up front, because these are easy to blur:
@@ -175,26 +215,46 @@ cargo build --workspace
 cargo test  --workspace
 cargo lint              # clippy; CI runs this same alias, so they cannot drift
 cargo xtask check       # architectural gates
+
+# Not in the default set: these drive nested cargo builds and kill real process
+# groups, and `cargo xtask mutate` would otherwise pay their cost 44 times over.
+cargo test -p xtask --test lifecycle -- --ignored --test-threads=1
 ```
 
 The toolchain is pinned to 1.98.1 in `rust-toolchain.toml`. That pin is load-bearing:
 `cranelift` 0.135 declares MSRV 1.95.0, so the 1.93.1 an earlier draft assumed could not
-have built the eventual backend.
+have built the eventual backend. The workspace's declared MSRV floor is 1.85; `xtask`
+declares 1.89 for itself, because it uses std's file locking and a development tool has no
+business raising the floor for the crates that ship.
 
 ## What is not built yet
 
 Almost everything. Specifically:
 
-- **The language is an interpreter, not a compiler.** Lexer, parser, type checker and
-  tree-walking evaluator, no IR and no backend — so nothing here says anything yet about
-  the performance claims, which are a backend question. The subset is the one described
-  above; everything else in `docs/user/spec.md` under *Open* is undecided.
+- **The language is an interpreter with one narrow native path.** Lexer, parser, type
+  checker and tree-walking evaluator, plus `nazm build` for `Int`/`Bool` expressions and
+  value-position `if` — no functions, loops, mutation or `Str`. One
+  IR level (`nazm-lir`), one backend (LLVM IR as text → `clang`), one verified target
+  (`aarch64-apple-darwin`); other targets are **unverified rather than unsupported**.
+  Nothing here says anything yet about the performance claims: `-O0` is the default, no
+  optimisation work has been done, and no measurement has been taken. The executable language is
+  **v0.3**; `docs/user/spec.md` keeps its sections versioned, so a *Settled for v0.1* entry
+  records what v0.1 decided rather than what is true now. Everything under *Open* is
+  undecided.
+- **No concurrency at all.** Not a partial implementation — nothing. `docs/user/spec.md` §6
+  settles the structured-concurrency borrow region as a *design*, and `docs/user/roadmap.md`
+  puts the specification work before any runtime. There is no scheduler and none is being
+  written.
 - **The harness is partial.** Real, tested, and mutation-checked (`cargo xtask mutate`,
-  44 runnable catalogued defects, all caught — plus two defects documented in `mutations.toml` as *uncovered*, which a passing run says nothing about): the accounting and comparison path (`nazm-bench
-  selftest`, 15 cases), per-attempt workspace management, the submission freeze, and the
-  independent acceptance runner. **Not written:** agent drivers, a provider adapter, and
-  all but one task fixture — so the harness cannot produce a benchmark number, and `run`
-  stays disabled.
+  44 runnable catalogued defects, all caught **as of `8d67a7d`** — plus two defects
+  documented in `mutations.toml` as *uncovered*, which a passing run says nothing about):
+  the accounting and comparison path (`nazm-bench selftest`, 19 cases), per-attempt
+  workspace management, the submission freeze, and the independent acceptance runner.
+  **Not written:** agent drivers, a provider adapter, and all but one task fixture — so
+  the harness cannot produce a benchmark number, and `run` stays disabled. The corpus has
+  **never accepted a known-good solution or rejected a known-bad one**: there is no
+  `reference/` or `broken/` directory anywhere, so it has not been shown to discriminate
+  at all.
 - **Confinement works and is tested against a real daemon.** Each attempt gets a private
   internal network, its own PostgreSQL container, and a candidate container with all
   capabilities dropped, no published ports, and no host namespaces. Four tests under
diff --git a/crates/nazm-cli/Cargo.toml b/crates/nazm-cli/Cargo.toml
index fd9c388..699c3b8 100644
--- a/crates/nazm-cli/Cargo.toml
+++ b/crates/nazm-cli/Cargo.toml
@@ -17,10 +17,14 @@ path = "src/main.rs"
 nazm-span = { workspace = true }
 nazm-diag = { workspace = true }
 nazm-core = { workspace = true }
+nazm-lir = { workspace = true }
 clap = { workspace = true }
 
 [dev-dependencies]
 serde_json = { workspace = true }
+# Each build case needs a throwaway directory for its source, its executable and, in two
+# cases, a stub toolchain on PATH.
+tempfile = { workspace = true }
 
 [lints]
 workspace = true
diff --git a/crates/nazm-cli/src/main.rs b/crates/nazm-cli/src/main.rs
index a1276ee..cdb6f0b 100644
--- a/crates/nazm-cli/src/main.rs
+++ b/crates/nazm-cli/src/main.rs
@@ -1,12 +1,31 @@
-//! `nazm` — run a Nazm program.
+//! `nazm` — run, check, or natively compile a Nazm program.
 //!
 //! Exit status is part of the interface, not an afterthought: `0` ran, `1` did not
-//! type-check, `2` failed at runtime, `3` could not read the file. A caller that has to
-//! parse stderr to find out which happened is doing work the process could have told it.
+//! type-check, `2` failed at runtime, `3` could not read the file, `4` the build
+//! toolchain was missing or failed. A caller that has to parse stderr to find out which
+//! happened is doing work the process could have told it.
+//!
+//! # The contract a compiled program keeps
+//!
+//! Decided before the wrapper was written, because these are the parts a user depends on
+//! and they are awkward to change afterwards:
+//!
+//! | | |
+//! | --- | --- |
+//! | **Result** | The full signed 64-bit value, in decimal, on standard output, followed by a newline. **Not** the exit status, which is 8 bits and would silently truncate almost every `Int` |
+//! | **Success** | Exit `0` |
+//! | **Runtime failure** | Exit `2` — the same status `nazm run` uses — with one line on standard error: `nazm: error[CODE]: message at file:line:column` |
+//! | **Diagnostic category** | The same stable code the interpreter reports: `N0400` for overflow, `N0401` for division by zero |
+//! | **Source location** | Line and column of the operator that failed, resolved at compile time and baked into the binary as a constant |
+//! | **`--json`** | Applies to **build-time** diagnostics only. A generated executable has no diagnostic framework linked into it and emits the plain line above; promising JSON there would mean shipping a serialiser in every binary |
+//!
+//! `nazm run` and `nazm check` are unchanged by any of this.
 
 use std::path::PathBuf;
 use std::process::ExitCode;
 
+mod build;
+
 use clap::{Parser, Subcommand};
 use nazm_core::Outcome;
 use nazm_span::SourceFile;
@@ -17,6 +36,10 @@ mod exit {
     pub(crate) const CHECK_FAILED: u8 = 1;
     pub(crate) const RUNTIME_FAILED: u8 = 2;
     pub(crate) const NO_SUCH_FILE: u8 = 3;
+    /// The build toolchain was missing, or failed on IR this compiler produced. Separate
+    /// from `CHECK_FAILED` on purpose: one means the program is wrong, the other means
+    /// the machine could not build a program that is fine.
+    pub(crate) const TOOLCHAIN_FAILED: u8 = 4;
 }
 
 #[derive(Parser, Debug)]
@@ -74,6 +97,30 @@ enum Command {
         #[arg(long)]
         json: bool,
     },
+    /// Compile to a native executable.
+    ///
+    /// A narrow subset for now — see `docs/user/roadmap.md` M1a. Anything outside it is
+    /// refused by name after ordinary checking; nothing is partially compiled.
+    Build {
+        file: PathBuf,
+        /// Where to write the executable. Defaults to the source's stem.
+        #[arg(short, long)]
+        out: Option<PathBuf>,
+        /// Emit diagnostics as one JSON object per line. Build-time only: see the module
+        /// documentation for why a generated executable does not do this.
+        #[arg(long)]
+        json: bool,
+        /// Also write the generated LLVM IR next to the executable, as `<out>.ll`.
+        #[arg(long)]
+        emit_ir: bool,
+        /// Optimisation level handed to `clang`.
+        ///
+        /// Defaults to 0. The specified arithmetic semantics hold at every level by
+        /// construction — overflow is an intrinsic and a branch, never `nsw` — and the
+        /// test suite compiles the same programs at 0 and 2 to keep that true.
+        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
+        opt_level: u8,
+    },
 }
 
 /// Parse `--max-depth`, refusing a budget no machine would grant the stack for.
@@ -102,6 +149,16 @@ fn max_depth(raw: &str) -> Result<usize, String> {
 
 fn main() -> ExitCode {
     let cli = Cli::parse();
+    if let Command::Build {
+        file,
+        out,
+        json,
+        emit_ir,
+        opt_level,
+    } = cli.command
+    {
+        return build::run(&file, out.as_deref(), json, emit_ir, opt_level);
+    }
     let (path, json, limits) = match cli.command {
         Command::Run {
             file,
@@ -117,6 +174,9 @@ fn main() -> ExitCode {
             }),
         ),
         Command::Check { file, json } => (file, json, None),
+        // Handled above; `main` keeps its original shape for the two commands whose
+        // behaviour this increment must not change.
+        Command::Build { .. } => unreachable!("handled above"),
     };
 
     let text = match std::fs::read_to_string(&path) {
diff --git a/crates/nazm-core/src/lib.rs b/crates/nazm-core/src/lib.rs
index ad9b4b6..7016320 100644
--- a/crates/nazm-core/src/lib.rs
+++ b/crates/nazm-core/src/lib.rs
@@ -102,22 +102,64 @@ fn pipeline(source: &str, limits: eval::Limits) -> Outcome {
 /// with the shape of the tree, and neither has anything to do with call depth.
 #[must_use]
 pub fn check_source(source: &str) -> Vec<nazm_diag::Diagnostic> {
-    eval::on_interpreter_stack(0, || {
+    // `()` as the result, so the tree is built, walked and **dropped** inside the closure.
+    // Returning it would move a recursive structure onto whatever stack the caller
+    // happens to have, which is the thing this function exists to avoid.
+    match on_checked(source, |_| Ok::<(), Vec<nazm_diag::Diagnostic>>(())) {
+        Ok(()) => Vec::new(),
+        Err(d) => d,
+    }
+}
+
+/// Parse, check, and run `f` over the tree — all on the interpreter's own stack.
+///
+/// # Why the tree never leaves
+///
+/// `docs/user/spec.md` promises that the whole pipeline runs on a thread whose stack the
+/// interpreter sets itself, because **a bound only means something against a known
+/// stack**: macOS gives the main thread 8 MiB, a spawned thread 2 MiB, and the test
+/// harness 2 MiB per test. Parsing, checking, lowering, emitting and *dropping* the tree
+/// all recurse with its shape.
+///
+/// An earlier version of this file returned the `Program` to the caller. Nothing observed
+/// an abort at `MAX_NESTING` — drop frames are small — but it made the guarantee depend on
+/// the caller's stack and on frames staying small, neither of which the contract mentions.
+/// So `f` runs *here*, and only its result crosses back. `T: Send` is what stops a
+/// borrowed tree escaping with it.
+///
+/// # Errors
+///
+/// The parse and check diagnostics if there are any — in which case `f` is never called,
+/// because running a later pass over a tree known to be ill-typed produces a second, less
+/// useful error about the same mistake. Otherwise whatever `f` returns.
+pub fn on_checked<T, F>(source: &str, f: F) -> Result<T, Vec<nazm_diag::Diagnostic>>
+where
+    T: Send,
+    F: FnOnce(&__syntax::Program) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
+{
+    let run = || {
         let (program, mut diagnostics) = nazm_syntax::parse_source(source);
         let parse_failed = !diagnostics.is_empty();
         let (_, check_errors) = check(&program);
+        // A function that failed to parse was never registered, so "this program has no
+        // `main`" would be reported for a file whose first line is `fn main`.
         diagnostics.extend(
             check_errors
                 .into_iter()
                 .filter(|d| !(parse_failed && d.code == nazm_diag::codes::MISSING_MAIN)),
         );
-        diagnostics
-    })
-    .unwrap_or_else(|e| {
-        vec![nazm_diag::Diagnostic::error(
+        if diagnostics.is_empty() {
+            f(&program)
+        } else {
+            Err(diagnostics)
+        }
+    };
+
+    eval::on_interpreter_stack(0, run).unwrap_or_else(|e| {
+        Err(vec![nazm_diag::Diagnostic::error(
             nazm_diag::codes::INTERPRETER_UNAVAILABLE,
             nazm_span::Span::new(0, 0),
             format!("could not start the checker: {e}"),
-        )]
+        )])
     })
 }
diff --git a/docs/internal/architecture.md b/docs/internal/architecture.md
index 36537f2..e370006 100644
--- a/docs/internal/architecture.md
+++ b/docs/internal/architecture.md
@@ -279,18 +279,29 @@ schedules, so those compare **permitted outcomes and invariants**, never identic
 
 ## 7. Crate layout
 
-Crates are created when their responsibility exists. **Phase 2 is `nazm-bench` and `xtask`
-only** — there is no compiler yet, deliberately.
+Crates are created when their responsibility exists.
 
-| Phase | Crates |
-| --- | --- |
-| **2 (current)** | `nazm-bench` · `xtask` |
-| 4 | `nazm-span` · `nazm-diag` · `nazm-syntax` · `nazm-hir` · `nazm-ty` · `nazm-core` · `nazm-mcp` · `nazm-cli` |
-| 6 | `nazm-db` · `nazm-mir` · `nazm-lsp` |
-| 7 | `nazm-lir` · `nazm-codegen-clif` · `nazm-link` · `nazm-driver` · `nazm-test` |
-| 8 | `nazm-rt` |
-| 9 | `nazm-mono` · `nazm-codegen-llvm` |
-| 10 | `nazm-pkg` · `nazm-std-build` |
+**Corrected 2026-09-20.** This section read "Phase 2 is `nazm-bench` and `xtask` only —
+there is no compiler yet, deliberately", and that is no longer the case: the Phase 4
+front-end crates exist and a `.nz` program executes. What has *not* changed is that
+everything from `nazm-hir` downward is still design — no IR, no backend, no runtime. See
+[`roadmap.md`](roadmap.md) for the order the rest is now planned in.
+
+| Phase | Crates | State |
+| --- | --- | --- |
+| 2 | `nazm-bench` · `xtask` | **exists** |
+| 4 | `nazm-span` · `nazm-diag` · `nazm-syntax` · `nazm-core` · `nazm-cli` | **exists** (as an interpreter; `nazm-core` is checker + evaluator, not the Core IR below) |
+| 4 | `nazm-hir` · `nazm-ty` · `nazm-mcp` | planned |
+| 6 | `nazm-db` · `nazm-mir` · `nazm-lsp` | planned |
+| 7 | `nazm-lir` · `nazm-codegen-clif` · `nazm-link` · `nazm-driver` · `nazm-test` | planned |
+| 8 | `nazm-rt` | planned |
+| 9 | `nazm-mono` · `nazm-codegen-llvm` | planned |
+| 10 | `nazm-pkg` · `nazm-std-build` | planned |
+
+One deliberate departure from this table is already recorded: [`roadmap.md`](roadmap.md)
+M1 brings `nazm-lir` and `nazm-codegen-llvm` forward, and does **not** create the levels
+between them and the AST. Five empty pass-through crates would match the diagram and carry
+no information.
 
 Roles, in dependency order: `span` (files, byte spans, `FileId`, interned identifiers —
 depends on nothing) · `diag` (stable error codes, fixits, JSON schema, renderer) · `syntax`
diff --git a/docs/evaluation.md b/docs/evaluation.md
index 8797513..197822d 100644
--- a/docs/evaluation.md
+++ b/docs/evaluation.md
@@ -2,8 +2,20 @@
 
 **Status: draft. Not yet pre-registered.**
 The decision rule in §6 becomes binding when it is committed and its commit hash is
-recorded in `docs/evaluation-preregistration.md`. Until then it is a proposal and may
-change freely. After that, it may not.
+recorded in `docs/evaluation-preregistration.md`. That file **does not exist**; until it
+does, the rule is a proposal and may change freely. After that, it may not.
+
+> **Scope changed 2026-09-20, contents unchanged.** This protocol no longer decides
+> whether the language project exists — see [`direction.md`](direction.md). It is now
+> **evidence for specific usability and performance claims**, and it is reproduced here
+> exactly as registered-in-draft: the arms, the metrics, the thresholds in §6 (`point
+> R_C ≤ 0.80` **and** `upper CB < 1.00`), the cost policy, and the void-rate rule are
+> untouched. Nothing has been relaxed to fit the new direction, and no earlier result has
+> been reinterpreted.
+>
+> **A2 keeps its role.** If improved tooling on an existing language captures most of a
+> measured benefit, that is a finding about the claim under test and it stays in the
+> record. It is no longer a stop condition.
 
 ---
 
diff --git a/docs/internal/runbook.md b/docs/internal/runbook.md
index ca6084f..37d66ad 100644
--- a/docs/internal/runbook.md
+++ b/docs/internal/runbook.md
@@ -75,3 +75,82 @@ $ rustc --version  → 1.98.1 (pinned in rust-toolchain.toml)
 The local `stable` was 1.93.1 (seven months behind) when this project started; the pin is
 what makes that irrelevant. `cranelift` 0.135 declares MSRV 1.95.0, so the pin is
 load-bearing for Stage 7 even though nothing depends on Cranelift yet.
+
+---
+
+## Recovering an interrupted mutation run
+
+`cargo xtask mutate` mutates the working tree in place and restores each file after
+testing it. If the run is killed in between, the tree is left holding a mutation and a
+write-ahead record at `target/.nazm-mutation-backup` describing how to undo it.
+
+**Recovery is not automatic, and that is deliberate.** Killing a run releases its lock at
+once — the runner's `cargo` and `rustc` descendants never held it, because Rust opens
+files `O_CLOEXEC` — so those descendants keep running and keep writing to the tree. The
+runner has no way to establish that they have stopped:
+
+* a **pid** may have been reused, and the one in the record belongs to a process that is
+  already gone;
+* a **content digest** describes the file at the instant it was read, not what will be
+  written to it next;
+* a **delay** proves nothing at any length.
+
+Restoring the source while one of them is still running, and then injecting the next
+mutation on top, would produce verdicts about a file two processes are editing. So a
+stranded record makes the next run **refuse to start**, and repair is an operator action.
+
+### The procedure
+
+1. **Establish that nothing is still writing to the tree.**
+
+   These commands help you look. **Neither is proof**, and the difference matters:
+
+   ```sh
+   pgrep -g <pgid>        # the interrupted run's process group, if you know it
+   pgrep -fl "$(pwd)"     # build and test processes mentioning this path
+   ```
+
+   A process-group search misses anything that called `setsid` or `setpgid`. A
+   path-or-name search misses a process whose command line does not mention the
+   checkout, one that holds an open descriptor to a file whose path has since changed,
+   and anything that has not started yet. Both are **investigative aids**; what
+   establishes quiescence is your knowledge of what you were running, and nothing here
+   can substitute for it.
+
+2. **Repair.**
+
+   ```sh
+   cargo xtask mutate-recover --confirm-no-writers
+   ```
+
+   The flag is **your attestation that the workspace is quiet** — not a check the tool
+   performs, and not a claim the searches above succeeded. The tool has no way to
+   perform such a check, which is precisely why recovery is not automatic and why no
+   automatic mechanism is being added. Recovery itself is still conservative: it restores the original bytes only
+   if the file still matches what the record describes, reports `already-restored` if the
+   run was killed after restoring but before clearing its record, and refuses outright if
+   the contents match neither. A file you edited by hand since the crash is never
+   overwritten.
+
+3. **Re-run.**
+
+   ```sh
+   cargo xtask mutate
+   ```
+
+### If recovery refuses
+
+It will say the file matches neither the mutation it applied nor the original it saved.
+That means something changed the file after the interruption. The original is still in
+`target/.nazm-mutation-backup`, after its fourth line. Compare it with the working copy,
+resolve by hand, then delete the record.
+
+### Interoperating with an older runner
+
+The lock protocol changed: it was a file whose *existence* was the lock, and it is now an
+advisory lock (`flock`) on `target/.nazm-mutation-lock.v2`, which is never unlinked. **The
+two protocols do not coordinate reliably, and the supported boundary is not to run both
+against one tree.** The new runner takes the old marker while it runs so an old runner
+usually stands down, and refuses to start if the old marker names a live process — but a
+first-protocol runner that has already passed its own check and not yet published is not
+covered by anything. Use one version.
diff --git a/docs/user/spec.md b/docs/user/spec.md
index 49e86d9..634f502 100644
--- a/docs/user/spec.md
+++ b/docs/user/spec.md
@@ -1,7 +1,12 @@
 # Nazm language specification
 
-**Status: v0.1 in progress (Phase 1). This is not yet a specification — it is the list of
-decisions a specification has to make, with the ones already settled marked as such.**
+**Status: this is not yet a specification — it is the list of decisions a specification
+has to make, with the ones already settled marked as such.**
+
+**The current executable language is v0.3.** Sections are versioned and kept in order, so
+a *Settled for v0.1* entry records what v0.1 decided, not what is true now; where a later
+version changed one, the earlier entry says so and points forward. Everything under *Open*
+is undecided.
 
 Publishing the open questions before the answers is deliberate. Every one of them was
 either wrong or unstated in an earlier draft of the design, and the cost of discovering
@@ -82,6 +87,12 @@ One exit point means the value a function produces is always in the same place.
 earns its keep with guard clauses, and it is not ruled out — it is deferred until there is
 something to guard against.
 
+> **Superseded in v0.3.** `return` exists and is implemented — see [`return`](#return)
+> below. This entry is kept rather than edited because it records what v0.1 committed to
+> and why, and the reasoning ("deferred until there is something to guard against") is
+> what the later section answers. **The current executable language is v0.3**; a function's
+> value is still its final expression, and `return` is for the paths that leave early.
+
 ### Types — `Int`, `Bool`, `Str`, and no inference across function boundaries
 
 Parameter and return types are always written. Local `let` bindings infer from their
@@ -419,9 +430,16 @@ Required before `nazm test --backend=all` means anything:
   grammar-constrained decoding is ever used).
 - A cheatsheet under ~3k tokens, to be injected by `nazm init`.
 
-## Non-goals for v0.1
+## Non-goals for the current interpreter milestone
 
-Deliberately out of scope until the gate:
+Deliberately out of scope:
 
 macros · comptime beyond constant folding · general effect handlers · whole-program taint ·
-layout selection · self-hosting · a package manager · a module system beyond files
+layout selection · a package manager · a module system beyond files
+
+**Self-hosting was on this list and has been moved off it.** It is now a stated long-term
+objective — see [`direction.md`](direction.md) — and it remains outside this milestone,
+which is a different thing from being a non-goal. The staged path is
+[`roadmap.md`](roadmap.md) M3, and it is deliberately preceded by M2: the language has no
+collections, no user-defined types, no pattern matching, no string concatenation and no
+I/O, so a compiler cannot be written in it yet at all.
diff --git a/xtask/Cargo.toml b/xtask/Cargo.toml
index 354f322..5a99d82 100644
--- a/xtask/Cargo.toml
+++ b/xtask/Cargo.toml
@@ -3,7 +3,11 @@ name = "xtask"
 description = "Repository automation and architectural gates."
 version.workspace = true
 edition.workspace = true
-rust-version.workspace = true
+# Not `workspace = true`. The workspace floor (1.85) is the oldest compiler the *shipped*
+# crates need; this one is a development tool and needs `std::fs::File::try_lock`, which
+# stabilised in 1.89. Declaring it here keeps the library crates' floor where it belongs
+# instead of raising it for a tool nobody builds against.
+rust-version = "1.89"
 license.workspace = true
 publish = false
 
@@ -12,5 +16,10 @@ serde = { workspace = true }
 serde_json = { workspace = true }
 toml = { workspace = true }
 
+# Only for tests: the restoration and lifecycle tests build throwaway trees to
+# interrupt and repair, and they must never touch the real checkout.
+[dev-dependencies]
+tempfile = { workspace = true }
+
 [lints]
 workspace = true
diff --git a/xtask/mutations/mutations.toml b/xtask/mutations/mutations.toml
index 479da2e..a5da22a 100644
--- a/xtask/mutations/mutations.toml
+++ b/xtask/mutations/mutations.toml
@@ -541,3 +541,39 @@ replace = "            String::new(),"
 #
 # What is not covered: removing the call while `set_permissions` still succeeds. Recorded
 # as uncovered, alongside `checking-runs-on-the-callers-stack`.
+
+# ---------------------------------------------------------------------------
+# The native backend. Added with M1b; each verified individually through a
+# single-entry run in a disposable copy of the checkout, not by a catalogue run.
+# ---------------------------------------------------------------------------
+
+[[mutation]]
+name = "checked-add-loses-its-sign"
+why = """
+`docs/user/spec.md` says overflow traps. The emitter detects it with
+`llvm.sadd.with.overflow.i64`; the unsigned intrinsic is one letter away and is the
+mistake a reader would not see. `Int::MAX + 1` does not carry as an unsigned addition, so
+the guard never fires and the program prints the wrapped value instead of stopping —
+silently wrong arithmetic in a language whose first promise is that it traps.
+
+Caught by `addition_overflow_traps`, which requires exit 2 AND the N0400 code AND the
+message. Exit status alone would not discriminate: a mutant that died on a signal would
+also be non-zero.
+"""
+file = "crates/nazm-lir/src/emit.rs"
+find = 'ArithOp::Add => "sadd",'
+replace = 'ArithOp::Add => "uadd",'
+
+[[mutation]]
+name = "the-conditional-selects-the-wrong-branch"
+why = """
+The one thing an `if` has to get right. Swapping the branch labels compiles, produces
+well-formed IR that clang accepts, and every phi still names a real predecessor — so
+nothing structural catches it. Only running a program whose two branches differ does.
+
+Caught by `both_branch_outcomes_are_reachable` and others, which assert the exact printed
+value rather than merely that the program ran.
+"""
+file = "crates/nazm-lir/src/emit.rs"
+find = '"  br i1 %{c}, label %{then_label}, label %{else_label}"'
+replace = '"  br i1 %{c}, label %{else_label}, label %{then_label}"'
diff --git a/xtask/src/main.rs b/xtask/src/main.rs
index fb8b6ec..15e2f9f 100644
--- a/xtask/src/main.rs
+++ b/xtask/src/main.rs
@@ -16,9 +16,21 @@ fn main() -> ExitCode {
         Some("check") => run_checks(),
         Some("check-layering") => report(&[("layering", check_layering())]),
         Some("check-codegen") => report(&[("codegen reach", check_codegen_reach())]),
+        Some("check-emission") => report(&[("emission reach", check_emission_reach())]),
         Some("check-unsafe") => report(&[("unsafe policy", check_unsafe_policy())]),
         Some("check-docs") => report(&[("doc claims", check_doc_claims())]),
         Some("mutate") => report(&[("mutation coverage", run_mutations())]),
+        // Documented, because it is now the *only* way to recover an interrupted run:
+        // `mutate` refuses rather than repairing, since it cannot establish that the
+        // dead run's descendants have stopped.
+        Some("mutate-recover") => mutate_recover(&mut args),
+        // Hidden, and reachable only from the lifecycle tests. They exist because the
+        // properties they check cannot be observed from inside one process: mutual
+        // exclusion needs two independent processes to contend, and an interruption
+        // needs a process that did not create the state left behind. Kept out of `help`
+        // so they are not mistaken for an interface.
+        Some("mutate-run") => mutate_run(&mut args),
+        Some("mutate-hold") => mutate_hold(&mut args),
         Some("help" | "--help" | "-h") | None => {
             help();
             ExitCode::SUCCESS
@@ -37,10 +49,13 @@ fn help() {
          Tasks:\n  \
            check           run every gate\n  \
            check-layering  crate dependency direction (rules.rs)\n  \
-           check-codegen   no front-end crate reaches a code generator\n  \
+           check-codegen   no front-end crate *depends on* a code generator\n  \
+           check-emission  only the backend crate emits IR or runs clang\n  \
            check-unsafe    the unsafe allowlist and the workspace lint agree\n  \
            check-docs      documented claims that must stay true\n  \
            mutate          inject catalogued defects; each must be caught\n  \
+           mutate-recover  repair a tree an interrupted mutation run left mutated;\n  \
+                           needs --confirm-no-writers, and read what it prints first\n  \
            help            this message\n"
     );
 }
@@ -49,6 +64,7 @@ fn run_checks() -> ExitCode {
     report(&[
         ("layering", check_layering()),
         ("codegen reach", check_codegen_reach()),
+        ("emission reach", check_emission_reach()),
         ("unsafe policy", check_unsafe_policy()),
         ("doc claims", check_doc_claims()),
     ])
@@ -116,6 +132,83 @@ fn check_layering() -> Result<(), Vec<String>> {
 /// The README says the decision rule is not yet pre-registered. The moment it *is*, that
 /// sentence has to change, and the failure mode is that nobody remembers. So: if the
 /// pre-registration file exists, the README must no longer claim otherwise.
+/// Only the crates that are supposed to may emit a backend artefact or run the toolchain.
+///
+/// # Why a source scan rather than a dependency scan
+///
+/// Text emission has no dependency to find. What it does have is a call site, and the
+/// property worth protecting is exactly about call sites: **a check-only path must not
+/// reach native emission or `clang`.** So this looks for the two capabilities by name in
+/// the source of every crate and requires them to appear only where `rules.rs` allows.
+///
+/// Stated precisely, because it is easy to overclaim: this is a **source-level check on
+/// where the capability is mentioned**, not a call-graph proof. It would not catch
+/// emission reached through a trait object or a re-export under another name. What it
+/// does catch is the realistic regression — someone calling `emit_llvm` or spawning
+/// `clang` from the checker because it was convenient — and it fails loudly when a new
+/// crate acquires either capability without being placed.
+///
+/// The behavioural half of the same claim is a test: `nazm check` run with an empty
+/// `PATH` must still succeed, which no amount of source scanning can establish.
+fn check_emission_reach() -> Result<(), Vec<String>> {
+    // The capability, and the text that betrays it.
+    const CAPABILITIES: &[(&str, &str)] = &[
+        ("emits LLVM IR", "emit_llvm"),
+        ("invokes the native toolchain", "\"clang\""),
+    ];
+
+    let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
+        return Err(vec!["could not locate workspace root".to_owned()]);
+    };
+
+    let mut problems = Vec::new();
+    for (crate_dir, allowed) in rules::emission_allowlist() {
+        let src = root.join(crate_dir).join("src");
+        let Ok(files) = collect_rs(&src) else {
+            continue;
+        };
+        for file in files {
+            let Ok(text) = std::fs::read_to_string(&file) else {
+                continue;
+            };
+            for (what, needle) in CAPABILITIES {
+                if text.contains(needle) && !allowed.contains(what) {
+                    problems.push(format!(
+                        "{} mentions `{needle}`, so it {what} — but rules.rs does not \
+                         allow it to. A check-only path that can reach native emission or \
+                         the toolchain is the separation this rule exists to keep.",
+                        file.strip_prefix(root).unwrap_or(&file).display()
+                    ));
+                }
+            }
+        }
+    }
+
+    if problems.is_empty() {
+        Ok(())
+    } else {
+        Err(problems)
+    }
+}
+
+/// Every `.rs` under a directory. Tests live outside `src/`, so assertions about IR
+/// content in `tests/` are not mistaken for the capability itself.
+fn collect_rs(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
+    let mut out = Vec::new();
+    let mut stack = vec![dir.to_path_buf()];
+    while let Some(d) = stack.pop() {
+        for entry in std::fs::read_dir(&d)? {
+            let path = entry?.path();
+            if path.is_dir() {
+                stack.push(path);
+            } else if path.extension().is_some_and(|e| e == "rs") {
+                out.push(path);
+            }
+        }
+    }
+    Ok(out)
+}
+
 fn check_doc_claims() -> Result<(), Vec<String>> {
     let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
         .parent()
@@ -163,11 +256,20 @@ fn check_doc_claims() -> Result<(), Vec<String>> {
     }
 }
 
-/// No crate may reach a code generator unless `rules.rs` says it can.
+/// No crate may **depend on** a code generator unless `rules.rs` says it can.
+///
+/// # What this establishes, and what it does not
 ///
-/// The rule exists before either backend does, because it is cheap to state now and
-/// contentious once someone has a reason to break it. `docs/internal/architecture.md` §1 puts
-/// Cranelift and LLVM behind the LIR contract; this is what keeps that true.
+/// It reads cargo metadata, so it is a statement about the dependency graph and nothing
+/// else. **Emitting LLVM IR as text is code generation and this check cannot see it** —
+/// `nazm-lir` generates a complete LLVM module with no codegen dependency at all, and
+/// passes here trivially. Describing that as architecture validation would be describing
+/// a vacuous result.
+///
+/// The capability question — *who may emit a backend artefact, and who may invoke the
+/// toolchain* — is [`check_emission_reach`], which looks at call sites instead. The two
+/// together are what `docs/internal/architecture.md` §1 means by keeping Cranelift and LLVM behind
+/// the LIR contract; neither is sufficient alone.
 fn check_codegen_reach() -> Result<(), Vec<String>> {
     const CODEGEN_CRATES: &[&str] = &[
         "cranelift",
@@ -262,6 +364,125 @@ fn check_unsafe_policy() -> Result<(), Vec<String>> {
 ///
 /// Not part of `check`: it rebuilds and re-tests the workspace once per mutation, so it is
 /// a scheduled job rather than a pre-commit gate.
+/// `--name value` pairs and bare `--name` switches, in any order.
+///
+/// Deliberately minimal. A switch is a `--name` whose next token is another `--name` or
+/// the end, and it maps to an empty string — the caller asks with `contains_key`. The
+/// first version required a value unconditionally, so a trailing `--confirm-no-writers`
+/// silently vanished and recovery refused a tree it had been told about. A flag a user
+/// typed and the program did not see is worse than no flag.
+fn flags(args: &mut impl Iterator<Item = String>) -> std::collections::HashMap<String, String> {
+    let mut out = std::collections::HashMap::new();
+    let mut args = args.peekable();
+    while let Some(a) = args.next() {
+        let Some(name) = a.strip_prefix("--") else {
+            continue;
+        };
+        let value = match args.peek() {
+            Some(next) if !next.starts_with("--") => args.next().unwrap_or_default(),
+            _ => String::new(),
+        };
+        out.insert(name.to_owned(), value);
+    }
+    out
+}
+
+fn need(f: &std::collections::HashMap<String, String>, k: &str) -> Result<String, ExitCode> {
+    match f.get(k) {
+        Some(v) => Ok(v.clone()),
+        None => {
+            eprintln!("missing --{k}");
+            Err(ExitCode::FAILURE)
+        }
+    }
+}
+
+/// Run the catalogue against an arbitrary root. The lifecycle tests point this at a
+/// disposable project so an interrupted run damages nothing that matters.
+fn mutate_run(args: &mut impl Iterator<Item = String>) -> ExitCode {
+    let f = flags(args);
+    let (Ok(root), Ok(catalogue)) = (need(&f, "root"), need(&f, "catalogue")) else {
+        return ExitCode::FAILURE;
+    };
+    match mutate::run_all(
+        std::path::Path::new(&root),
+        std::path::Path::new(&catalogue),
+    ) {
+        Ok(()) => ExitCode::SUCCESS,
+        Err(problems) => {
+            for p in problems {
+                println!("problem: {p}");
+            }
+            ExitCode::FAILURE
+        }
+    }
+}
+
+/// Take the lock, announce it, and hold it until told to stop.
+///
+/// The announcement is a file rather than a line on stdout because the test has to know
+/// the lock is *held* before it starts competing, and a pipe read is one more thing that
+/// can be buffered. Written after acquisition, so its existence is the fact it claims.
+fn mutate_hold(args: &mut impl Iterator<Item = String>) -> ExitCode {
+    let f = flags(args);
+    let (Ok(root), Ok(ready), Ok(release)) =
+        (need(&f, "root"), need(&f, "ready"), need(&f, "release"))
+    else {
+        return ExitCode::FAILURE;
+    };
+
+    let Ok(lock) = mutate::acquire_lock(std::path::Path::new(&root)) else {
+        eprintln!("hold: could not acquire");
+        return ExitCode::FAILURE;
+    };
+    if std::fs::write(&ready, "held").is_err() {
+        return ExitCode::FAILURE;
+    }
+
+    // Bounded, so a test that fails to signal leaves no process behind for the next one
+    // to trip over.
+    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
+    while !std::path::Path::new(&release).exists() {
+        if std::time::Instant::now() > deadline {
+            eprintln!("hold: released on the deadline, not on request");
+            return ExitCode::FAILURE;
+        }
+        std::thread::sleep(std::time::Duration::from_millis(20));
+    }
+    drop(lock);
+    ExitCode::SUCCESS
+}
+
+/// Startup recovery on its own, through the same function a real run calls.
+fn mutate_recover(args: &mut impl Iterator<Item = String>) -> ExitCode {
+    let f = flags(args);
+    // An operator runs this from the checkout and should not have to say where it is.
+    // The lifecycle tests pass `--root` to point it at a disposable project instead.
+    let root = match f.get("root") {
+        Some(r) => r.clone(),
+        None => match std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
+            Some(p) => p.display().to_string(),
+            None => {
+                eprintln!("could not locate the workspace root");
+                return ExitCode::FAILURE;
+            }
+        },
+    };
+    match mutate::recover(
+        std::path::Path::new(&root),
+        f.contains_key("confirm-no-writers"),
+    ) {
+        Ok(outcome) => {
+            println!("{outcome}");
+            ExitCode::SUCCESS
+        }
+        Err(e) => {
+            println!("error: {e}");
+            ExitCode::FAILURE
+        }
+    }
+}
+
 fn run_mutations() -> Result<(), Vec<String>> {
     let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
         return Err(vec!["could not locate workspace root".to_owned()]);
diff --git a/xtask/src/mutate.rs b/xtask/src/mutate.rs
index 26a6610..fe31608 100644
--- a/xtask/src/mutate.rs
+++ b/xtask/src/mutate.rs
@@ -41,7 +41,21 @@ fn backup_path(root: &Path) -> PathBuf {
     root.join("target").join(".nazm-mutation-backup")
 }
 
+/// The advisory lock the runner holds for the length of a run.
+///
+/// Deliberately **not** the path the first protocol used. That protocol treated the
+/// file's *existence* as the lock and unlinked it on release; this one keeps a file and
+/// locks it. Sharing one path between the two is not a smaller change, it is a broken
+/// one: the v2 file outlives its run, so an old runner's `create_new` would fail, it
+/// would read the file, judge the holder dead or unattributable, and **delete and
+/// recreate it** — at which point two v2 runners lock two different inodes and both
+/// believe they are alone. A separate path makes that impossible.
 fn lock_path(root: &Path) -> PathBuf {
+    root.join("target").join(".nazm-mutation-lock.v2")
+}
+
+/// The first protocol's lock file. Read for interoperability, never locked.
+fn legacy_lock_path(root: &Path) -> PathBuf {
     root.join("target").join(".nazm-mutation-lock")
 }
 
@@ -53,57 +67,311 @@ fn lock_path(root: &Path) -> PathBuf {
 /// exactly like real regressions. Several minutes were spent diagnosing "broken" tests
 /// that were only ever reading a file another process had deliberately damaged.
 ///
-/// Held as a file containing the pid, removed on completion. A stale lock from a killed
-/// run is detected by signalling the pid.
-struct RunLock {
-    path: PathBuf,
+/// # Ownership is the kernel's answer, not a pid's
+///
+/// The first protocol recorded a pid and reasoned about it: create the file exclusively,
+/// and if that failed, decide from the pid inside whether the holder was gone. Two
+/// defects followed from that shape and neither was the last one. `kill -0 0` succeeds,
+/// so a malformed lock read as permanently held. Then reclaiming a dead holder's lock
+/// meant `remove` followed by `create`, which is not atomic: two runners that both saw
+/// the same dead holder could interleave — A removes, A creates, **B removes A's fresh
+/// lock**, B creates — and both would run.
+///
+/// Every repair of that shape is a repair to a race the process cannot actually
+/// arbitrate. So the decision is handed to the kernel: [`fs::File::try_lock`] on a file
+/// that is opened, locked, and **kept open for the whole run**. There is exactly one
+/// winner, the loser is told it lost, and a holder that dies has its lock released by the
+/// kernel with no reclaim path to get wrong. The file itself is never unlinked, so it is
+/// a stable object to lock rather than one whose identity changes under contention.
+///
+/// The pid written inside is **diagnostic only**. Nothing reads it to decide ownership.
+///
+/// # What the lock does not establish, and what nothing here establishes
+///
+/// Not quiescence. Rust opens files `O_CLOEXEC`, so the `cargo` processes this runner
+/// spawns do *not* inherit the lock; a runner killed mid-build has its lock released
+/// immediately while its `cargo` child is still running and still writing to the tree.
+/// Measured on this platform, not assumed.
+///
+/// An earlier version of this comment went on to claim that the write-ahead record
+/// covered the gap, because [`restore_stranded`] re-reads and re-digests the file before
+/// writing. **That was wrong, and it is the same error this project already corrected
+/// once for `(dev, ino)` in the harness snapshot code.** A digest establishes what a file
+/// contained at the instant it was read. It cannot establish that nothing will write the
+/// file afterwards, so it is a check against *prior* drift and not a quiescence proof.
+///
+/// There is no mechanism here that establishes quiescence, and the honest consequence is
+/// that automatic recovery is disabled: a stranded record makes [`run_all`] refuse rather
+/// than repair. See [`restore_stranded`] for the procedure that replaces it.
+#[derive(Debug)]
+pub(crate) struct RunLock {
+    /// Held open for the lifetime of the run. Dropping it releases the kernel's lock;
+    /// that is the whole release protocol, and the file stays on disk.
+    file: fs::File,
+    /// Removed on release so that a first-protocol runner stops seeing a held tree.
+    legacy_marker: PathBuf,
+}
+
+/// Why the tree could not be taken. Every variant fails closed.
+#[derive(Debug)]
+enum AcquireError {
+    /// Another v2 runner holds the lock. The ordinary case, and the only benign one.
+    Contended,
+    /// The filesystem does not implement advisory locking. Not contention, and not a
+    /// licence to continue: without the lock there is nothing to stop a second runner.
+    Unsupported(String),
+    /// The lock file could not be opened or locked for a reason of access, not of
+    /// contention.
+    Permission(String),
+    /// Anything else, reported as itself rather than folded into one of the above.
+    Other(String),
+    /// A first-protocol lock names a process that is alive. The protocols do not
+    /// coordinate, so this refuses rather than guessing.
+    LegacyRunner(u32),
+    /// A first-protocol lock exists and cannot be attributed to any process.
+    LegacyUnattributable(String),
+}
+
+impl AcquireError {
+    fn describe(&self, root: &Path) -> String {
+        match self {
+            Self::Contended => format!(
+                "another mutation run holds {}. Two runs corrupt each other's restores, \
+                 and a concurrent `cargo test` would see a mutated tree and report \
+                 failures that are not real.",
+                lock_path(root).display()
+            ),
+            Self::Unsupported(why) => format!(
+                "advisory locking is not available for {} ({why}). Refusing to run: \
+                 without it nothing prevents a second runner from mutating the same tree. \
+                 Run from a filesystem that supports file locking.",
+                lock_path(root).display()
+            ),
+            Self::Permission(why) => format!(
+                "cannot take the mutation lock at {} ({why}). Refusing rather than \
+                 continuing unlocked.",
+                lock_path(root).display()
+            ),
+            Self::Other(why) => format!(
+                "cannot take the mutation lock at {} ({why}). Refusing rather than \
+                 continuing unlocked.",
+                lock_path(root).display()
+            ),
+            Self::LegacyRunner(pid) => format!(
+                "a mutation run using the previous lock protocol appears to be active \
+                 (pid {pid}, holding {}). The two protocols do not coordinate, so this \
+                 run refuses rather than joining it.",
+                legacy_lock_path(root).display()
+            ),
+            Self::LegacyUnattributable(why) => format!(
+                "a lock file from the previous protocol is present at {} and cannot be \
+                 attributed to a process ({why}). Refusing rather than reclaiming. If no \
+                 mutation run is active, delete the file.",
+                legacy_lock_path(root).display()
+            ),
+        }
+    }
 }
 
 impl RunLock {
-    fn acquire(root: &Path) -> Result<Self, String> {
+    fn acquire(root: &Path) -> Result<Self, AcquireError> {
         let path = lock_path(root);
-        if let Ok(existing) = fs::read_to_string(&path) {
-            let pid = existing.trim();
-            if pid_is_alive(pid) {
-                return Err(format!(
-                    "another mutation run is in progress (pid {pid}). Two runs corrupt \
-                     each other's restores, and a concurrent `cargo test` would see a \
-                     mutated tree and report failures that are not real."
-                ));
-            }
-            // The holder is gone; `restore_stranded` deals with whatever it left.
-            let _ = fs::remove_file(&path);
-        }
         if let Some(parent) = path.parent() {
             let _ = fs::create_dir_all(parent);
         }
-        // `create_new` is the race-free part: read-then-write lets two runs both see no
-        // lock and both create one. The pid check above only handles a *stale* lock.
-        let mut f = fs::OpenOptions::new()
+
+        // Opened read+write and created if absent, never truncated on open: another
+        // runner may be holding a lock on this very file, and truncating it before
+        // asking for the lock would rewrite an object someone else owns.
+        let file = fs::File::options()
+            .read(true)
             .write(true)
-            .create_new(true)
+            .create(true)
+            .truncate(false)
             .open(&path)
-            .map_err(|e| format!("could not take the mutation lock: {e}"))?;
-        use std::io::Write as _;
-        write!(f, "{}", std::process::id())
-            .map_err(|e| format!("could not record the mutation lock holder: {e}"))?;
-        Ok(Self { path })
+            .map_err(|e| classify_io(&e))?;
+
+        match file.try_lock() {
+            Ok(()) => {}
+            Err(fs::TryLockError::WouldBlock) => return Err(AcquireError::Contended),
+            Err(fs::TryLockError::Error(e)) => return Err(classify_io(&e)),
+        }
+
+        // Held from here. Everything below may fail, and dropping `file` on the way out
+        // releases the kernel's lock, so no path leaves the tree marked as taken.
+        let legacy = legacy_lock_path(root);
+        claim_legacy_marker(&legacy)?;
+
+        // Diagnostic only. Written after the lock is held, so its contents can never be
+        // what another runner consults to decide whether it may proceed.
+        let _ = write_pid(&file);
+
+        Ok(Self {
+            file,
+            legacy_marker: legacy,
+        })
     }
 }
 
-impl Drop for RunLock {
-    fn drop(&mut self) {
-        let _ = fs::remove_file(&self.path);
+/// Separate "someone else has it" from "this cannot work here" from "I am not allowed".
+///
+/// `ENOTSUP`/`EOPNOTSUPP` from some network filesystems, and `EINVAL` from a few others,
+/// mean the call will never succeed — silently proceeding there would leave the runner
+/// with no mutual exclusion at all on exactly the filesystems where a shared checkout is
+/// most likely.
+fn classify_io(e: &std::io::Error) -> AcquireError {
+    use std::io::ErrorKind;
+    match e.kind() {
+        ErrorKind::PermissionDenied => AcquireError::Permission(e.to_string()),
+        ErrorKind::Unsupported => AcquireError::Unsupported(e.to_string()),
+        _ => match e.raw_os_error() {
+            // ENOTSUP and EOPNOTSUPP share a value on macOS (45); on Linux EOPNOTSUPP is
+            // 95. EINVAL (22) is what some filesystems return for an unlockable file.
+            Some(45 | 95 | 22) => AcquireError::Unsupported(e.to_string()),
+            _ => AcquireError::Other(e.to_string()),
+        },
+    }
+}
+
+fn write_pid(file: &fs::File) -> std::io::Result<()> {
+    use std::io::{Seek, Write as _};
+    let mut f = file;
+    f.set_len(0)?;
+    f.rewind()?;
+    writeln!(f, "{}", std::process::id())?;
+    f.flush()
+}
+
+/// Take the first protocol's marker so that an old runner is *likely* to refuse to start.
+///
+/// # This is mitigation, not compatibility
+///
+/// **The two protocols do not coordinate, and this does not make them.** One treats a
+/// file's existence as the lock; the other locks a file. There is no sequence of
+/// unprivileged filesystem operations that makes those two agree, because the v1 side's
+/// logic is fixed and does not consult a lock.
+///
+/// The residual race is concrete and worth naming rather than implying away: a v2 runner
+/// reads the marker and finds it absent, a v1 runner publishes its own marker, and the v2
+/// runner then writes over it — both proceed. Creating the marker exclusively (below)
+/// removes exactly that interleaving, but a v1 runner that has already passed its own
+/// existence check and not yet published is still unaccounted for.
+///
+/// **The supported boundary is therefore: do not run a v1 and a v2 runner against the
+/// same tree.** What this code buys is that the common cases — a v1 marker already
+/// present, or a v1 runner starting after this one — are caught, and that a stale v1
+/// marker does not need a human. It is consulted only *after* the real lock is held, so
+/// no v2 runner's ownership decision depends on it.
+fn claim_legacy_marker(path: &Path) -> Result<(), AcquireError> {
+    match fs::read_to_string(path) {
+        Err(_) => {}
+        Ok(existing) => match holder_state(existing.trim()) {
+            // Holding the v2 lock already proves no v2 runner is here, so a live pid is
+            // either a v1 runner or an unrelated process that inherited the number.
+            // Either way it is not this run's to take.
+            Holder::Alive(pid) if pid != std::process::id() => {
+                return Err(AcquireError::LegacyRunner(pid))
+            }
+            Holder::Alive(_) => {}
+            Holder::Dead(pid) => {
+                println!(
+                    "  cleared      a first-protocol lock left by pid {pid}, which is no \
+                     longer running"
+                );
+                let _ = fs::remove_file(path);
+            }
+            Holder::Unknown(why) => return Err(AcquireError::LegacyUnattributable(why)),
+        },
+    }
+    // `create_new`, not `write`: overwriting would clobber a marker a v1 runner published
+    // between the read above and this line, and tell it that it holds a tree we are also
+    // about to mutate. If it lost the create, it is because someone else published first,
+    // and that someone must be judged rather than overwritten.
+    match fs::File::options().write(true).create_new(true).open(path) {
+        Ok(mut f) => {
+            use std::io::Write as _;
+            let _ = writeln!(f, "{}", std::process::id());
+            Ok(())
+        }
+        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
+            match holder_state(fs::read_to_string(path).unwrap_or_default().trim()) {
+                Holder::Alive(pid) if pid != std::process::id() => {
+                    Err(AcquireError::LegacyRunner(pid))
+                }
+                Holder::Alive(_) | Holder::Dead(_) => Ok(()),
+                Holder::Unknown(why) => Err(AcquireError::LegacyUnattributable(why)),
+            }
+        }
+        // The marker is advisory. Failing to write it is not a reason to refuse a tree
+        // whose real lock we hold.
+        Err(_) => Ok(()),
     }
 }
 
-fn pid_is_alive(pid: &str) -> bool {
-    Command::new("kill")
-        .args(["-0", pid])
+/// What can be established about whoever holds the *legacy* marker.
+///
+/// Retained from the first protocol, narrowed to the one job it is still fit for. It is
+/// no longer an ownership decision for this runner — [`RunLock`] gets that from the
+/// kernel — only a judgement about whether a v1 runner may be active.
+#[derive(Debug)]
+enum Holder {
+    Alive(u32),
+    Dead(u32),
+    /// Neither could be established. Not the same as dead, and must not be treated as
+    /// it: a probe that failed says nothing about the process.
+    Unknown(String),
+}
+
+fn holder_state(pid: &str) -> Holder {
+    // Validated before any probe. `kill -0 0` addresses the current process group and
+    // succeeds; a negative number addresses another group. Neither answers the question
+    // being asked, so neither reaches the probe.
+    let Ok(n) = pid.parse::<u32>() else {
+        return Holder::Unknown(if pid.is_empty() {
+            "the lock is empty".to_owned()
+        } else {
+            format!("`{pid}` is not a process id")
+        });
+    };
+    if n == 0 {
+        return Holder::Unknown("0 is not a process id".to_owned());
+    }
+    process_exists(n)
+}
+
+/// Does this process exist?
+///
+/// `ps` rather than `kill -0`, because the answer has to distinguish "no such process"
+/// from "exists but is not yours". `kill -0` fails for both, and through the `kill`
+/// binary there is no errno to tell them apart — so a lock held by another user's live
+/// run would look dead and be stolen.
+fn process_exists(pid: u32) -> Holder {
+    match Command::new("ps")
+        .args(["-p", &pid.to_string(), "-o", "pid="])
         .stdout(std::process::Stdio::null())
         .stderr(std::process::Stdio::null())
         .status()
-        .is_ok_and(|s| s.success())
+    {
+        Ok(s) if s.success() => Holder::Alive(pid),
+        // `ps` ran and found nothing. That is an answer.
+        Ok(_) => Holder::Dead(pid),
+        // `ps` could not run at all. That is not.
+        Err(e) => Holder::Unknown(format!("could not probe pid {pid}: {e}")),
+    }
+}
+
+impl Drop for RunLock {
+    fn drop(&mut self) {
+        // The legacy marker is existence-based, so releasing it means removing it. The
+        // v2 lock file is *not* removed: it is the object the next runner locks, and
+        // unlinking it under a waiting process is how the old protocol lost mutual
+        // exclusion.
+        let _ = fs::remove_file(&self.legacy_marker);
+        // Closing the handle would release the lock anyway. Unlocking explicitly says so
+        // at the point it happens, and keeps the release from depending on exactly when
+        // the last clone of the descriptor goes away.
+        let _ = self.file.unlock();
+    }
 }
 
 /// One catalogued defect. Every field is required: a mutation that cannot say what it
@@ -142,6 +410,13 @@ pub(crate) enum Verdict {
     /// move the case to a suite that runs the program as a child process, where a crash
     /// is an exit code the parent can assert on.
     MutantCrashed { signature: String, missing: usize },
+    /// The mutation was injected and could **not** be taken back out.
+    ///
+    /// Fatal to the run, and the only verdict that is. Every other outcome leaves the
+    /// tree as it was found; this one leaves it mutated, so continuing would inject the
+    /// next defect on top of the last one and measure both at once. The record is kept,
+    /// which is what makes the tree recoverable afterwards.
+    NotRestored(String),
     /// The suite did not run to completion: the runner errored, produced no results, or
     /// produced fewer result lines than the baseline.
     ///
@@ -153,6 +428,35 @@ pub(crate) enum Verdict {
     RunnerFailed(String),
 }
 
+/// Take the tree's lock, for the hidden `mutate-hold` fixture.
+///
+/// The same acquisition a real run performs — the fixture exists so a *second* process
+/// can be shown to lose, which cannot be demonstrated from inside the one that won.
+pub(crate) fn acquire_lock(root: &Path) -> Result<RunLock, String> {
+    RunLock::acquire(root).map_err(|e| e.describe(root))
+}
+
+/// Recover an interrupted run. The operator's action, not the runner's.
+///
+/// `confirmed` is the operator asserting that nothing is still writing to this tree —
+/// the step [`run_all`] cannot take for itself and no longer pretends to. Without it,
+/// this prints the procedure and changes nothing.
+///
+/// The lock is taken first and held across the restore, so a *new* run cannot start
+/// underneath. That bounds v2 runners and nothing else; it says nothing about a dead
+/// run's surviving children, which is exactly why the attestation is required.
+pub(crate) fn recover(root: &Path, confirmed: bool) -> Result<String, String> {
+    let _lock = acquire_lock(root)?;
+    if !confirmed && backup_path(root).exists() {
+        return Err(stranded_refusal(root));
+    }
+    Ok(match restore_stranded(root)? {
+        Recovery::Repaired(f) => format!("repaired {f}"),
+        Recovery::AlreadyRestored(f) => format!("already-restored {f}"),
+        Recovery::Nothing => "nothing to recover".to_owned(),
+    })
+}
+
 pub(crate) fn load(path: &Path) -> Result<Vec<Mutation>, String> {
     let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
     let catalogue: Catalogue =
@@ -169,20 +473,30 @@ pub(crate) fn run_all(root: &Path, catalogue: &Path) -> Result<(), Vec<String>>
         Err(e) => return Err(vec![e]),
     };
 
-    let _lock = match RunLock::acquire(root) {
+    // Bound to a name that lives to the end of this function on purpose. The kernel
+    // lock is released when this handle drops, so it has to outlive recovery, every
+    // injection, every test run, and every restoration — not just acquisition.
+    let run_lock = match RunLock::acquire(root) {
         Ok(l) => l,
-        Err(e) => return Err(vec![e]),
+        Err(e) => return Err(vec![e.describe(root)]),
     };
 
-    // A previous run may have been killed mid-mutation. Repair before measuring anything,
-    // and say so: a silently repaired tree leaves whoever saw the last run's output
-    // wondering why it looked wrong.
-    match restore_stranded(root) {
-        Ok(Some(file)) => {
-            println!("  restored     {file} (a previous run was killed mid-mutation)");
-        }
-        Ok(None) => {}
-        Err(e) => return Err(vec![e]),
+    // A previous run may have been killed mid-mutation. This used to repair it here and
+    // carry on. It no longer does, and the reason is the one thing this runner cannot
+    // establish: **whether the dead run's descendants have stopped.**
+    //
+    // The lock says nothing about them — it is released by the kernel the instant the
+    // coordinator dies, while its `cargo` and `rustc` children keep running and keep
+    // writing. Neither does the record: a digest says what a file held when it was read,
+    // not that nothing will write it next. Repairing under a live writer, and then
+    // injecting the next mutation on top, is how a run produces verdicts about a file
+    // two processes are editing.
+    //
+    // So recovery becomes an operator action, taken once quiescence has actually been
+    // checked. `restore_stranded` is unchanged and still correct about contents; it is
+    // simply no longer called by something that cannot know it is safe to call it.
+    if backup_path(root).exists() {
+        return Err(vec![stranded_refusal(root)]);
     }
 
     // Establish a baseline before touching anything. If the suite is already failing, or
@@ -256,6 +570,16 @@ pub(crate) fn run_all(root: &Path, catalogue: &Path) -> Result<(), Vec<String>>
                     m.name
                 ));
             }
+            Verdict::NotRestored(why) => {
+                println!("  NOT RESTORED {}", m.name);
+                problems.push(format!(
+                    "{}: {why} The run stopped here: every later verdict would have been \
+                     measured against a file still holding this mutation.",
+                    m.name
+                ));
+                // Deliberately not a `continue`. The tree is damaged.
+                break;
+            }
             Verdict::RunnerFailed(why) => {
                 println!("  RUNNER ERROR {}", m.name);
                 problems.push(format!(
@@ -268,6 +592,10 @@ pub(crate) fn run_all(root: &Path, catalogue: &Path) -> Result<(), Vec<String>>
         }
     }
 
+    // Every mutation has been injected and restored by now. Released explicitly so the
+    // extent of the critical section is visible here rather than inferred from scope.
+    drop(run_lock);
+
     if problems.is_empty() {
         Ok(())
     } else {
@@ -275,6 +603,41 @@ pub(crate) fn run_all(root: &Path, catalogue: &Path) -> Result<(), Vec<String>>
     }
 }
 
+/// Why this run will not start, and exactly what to do about it.
+///
+/// Deliberately a procedure rather than a suggestion. "Resolve by hand" without saying
+/// how is what makes an operator reach for `rm` on whichever file looks suspicious.
+fn stranded_refusal(root: &Path) -> String {
+    format!(
+        "a previous mutation run was interrupted and left a recovery record at {}. \n\
+         \n\
+         This run will not start, and recovery is not automatic. Killing a run releases \n\
+         its lock immediately, but its `cargo` and `rustc` descendants are not killed \n\
+         with it and keep writing to this tree. Nothing here can establish that they \n\
+         have stopped: a pid may have been reused, a matching digest only describes the \n\
+         instant it was read, and a delay proves nothing at all. Restoring the source \n\
+         while one of them is still running, and then injecting the next mutation on \n\
+         top, would produce verdicts about a file two processes are editing.\n\
+         \n\
+         To recover:\n\
+         \n\
+         1. Establish that nothing is still writing to this tree. `pgrep -g <pgid>` for \n\
+            the interrupted run's process group, or `pgrep -fl '{}'`, will *help you \n\
+            look* — but neither is proof. A process may have changed its group, been \n\
+            re-parented, hold a descriptor under a path that does not match, or be about \n\
+            to start. Treat them as investigative aids.\n\
+         2. Run `cargo xtask mutate-recover --confirm-no-writers`, which restores the \n\
+            original bytes only if the file still matches what the record describes, and \n\
+            refuses if it does not.\n\
+         3. Re-run `cargo xtask mutate`.\n\
+         \n\
+         The flag in step 2 is **your attestation that the workspace is quiet**, not a \n\
+         check this tool performs. It has no way to perform one.",
+        backup_path(root).display(),
+        root.display()
+    )
+}
+
 fn apply_and_test(root: &Path, m: &Mutation, baseline_results: usize) -> Verdict {
     let path: PathBuf = root.join(&m.file);
     let Ok(original) = fs::read_to_string(&path) else {
@@ -304,7 +667,12 @@ fn apply_and_test(root: &Path, m: &Mutation, baseline_results: usize) -> Verdict
     // overwrite whatever is on disk — including edits made after the crash, by someone
     // who had no idea a mutation was stranded there.
     let backup = backup_path(root);
-    let record = format!("{}\n{}\n{original}", m.file, digest(&mutated));
+    let record = format!(
+        "{RECORD_VERSION}\n{}\n{}\n{}\n{original}",
+        m.file,
+        digest(&mutated),
+        digest(&original)
+    );
     if let Some(parent) = backup.parent() {
         let _ = fs::create_dir_all(parent);
     }
@@ -341,47 +709,147 @@ fn apply_and_test(root: &Path, m: &Mutation, baseline_results: usize) -> Verdict
         Verdict::DidNotBuild
     };
 
-    let _ = fs::write(&path, &original);
-    let _ = fs::remove_file(&backup);
-    verdict
+    finish(&path, &original, &backup, &m.file, verdict, |p, c| {
+        fs::write(p, c)
+    })
+}
+
+/// Put the original back, then clear the record — in that order, and only in that order.
+///
+/// The write is taken as a parameter so a test can fail it at the exact boundary that
+/// matters. That is a smaller seam than a filesystem trait: there is one call, its
+/// failure has one consequence, and the consequence is what needs a regression.
+///
+/// Clearing the record regardless of whether the restore succeeded — which is what this
+/// did — converts the one recoverable failure into an unrecoverable one. The file stays
+/// mutated and the write-ahead record that would have repaired it is gone, so the next
+/// run measures a damaged tree with no way to know.
+fn finish(
+    path: &Path,
+    original: &str,
+    backup: &Path,
+    file: &str,
+    verdict: Verdict,
+    write: impl FnOnce(&Path, &str) -> std::io::Result<()>,
+) -> Verdict {
+    match write(path, original) {
+        Ok(()) => {
+            let _ = fs::remove_file(backup);
+            verdict
+        }
+        Err(e) => Verdict::NotRestored(format!(
+            "{file} was mutated and could not be restored ({e}). Its original is \
+             preserved at {} and has deliberately been kept. Recover with \
+             `cargo xtask mutate-recover --confirm-no-writers` once nothing is still \
+             writing to this tree.",
+            backup.display()
+        )),
+    }
+}
+
+/// Marks the write-ahead record's layout, so a record this runner did not write is
+/// reported rather than misread.
+///
+/// The first layout was `file\ndigest(mutated)\noriginal`, whose first line is a path —
+/// distinguishable from this marker, which is the point. A record left by an older runner
+/// is a state this one cannot safely interpret, and guessing at it would risk writing the
+/// wrong bytes over a source file.
+const RECORD_VERSION: &str = "nazm-mutation-backup 2";
+
+/// What startup recovery found.
+#[derive(Debug, PartialEq, Eq)]
+pub(crate) enum Recovery {
+    /// A stranded mutation was found and the original restored.
+    Repaired(String),
+    /// The source was already the original; only the record was stranded. The run was
+    /// killed in the window between restoring the file and clearing its record.
+    AlreadyRestored(String),
+    /// Nothing to do.
+    Nothing,
 }
 
 /// Restore a mutation a killed run left applied.
 ///
-/// Returns what it restored, so the caller can say so rather than silently repairing and
+/// Returns what it did, so the caller can say so rather than silently repairing and
 /// leaving the operator wondering why the previous run's results looked wrong.
-fn restore_stranded(root: &Path) -> Result<Option<String>, String> {
+///
+/// # Why the already-restored case is not drift
+///
+/// `apply_and_test` restores the file and *then* removes the record, and a process killed
+/// between those two writes leaves an intact tree with a record still describing it. The
+/// first version of this function compared the file against the mutated digest only, so
+/// that state failed the comparison and was reported as "its contents have changed since
+/// … resolve by hand" — a demand for manual intervention on a tree that was already
+/// correct. Recording the original's digest as well is what separates "already restored"
+/// from "somebody edited this".
+fn restore_stranded(root: &Path) -> Result<Recovery, String> {
     let backup = backup_path(root);
     let Ok(record) = fs::read_to_string(&backup) else {
-        return Ok(None);
+        return Ok(Recovery::Nothing);
     };
-    let Some((file, rest)) = record.split_once('\n') else {
-        return Err(format!("the backup at {} is malformed", backup.display()));
+
+    let mut lines = record.splitn(5, '\n');
+    let malformed = || {
+        format!(
+            "the backup at {} is malformed. It was not written by this runner, or it was \
+             truncated. Nothing has been restored; inspect it and the working tree by hand.",
+            backup.display()
+        )
     };
-    let Some((expected_mutated, original)) = rest.split_once('\n') else {
-        return Err(format!("the backup at {} is malformed", backup.display()));
+
+    if record.trim().is_empty() {
+        return Err(malformed());
+    }
+
+    match lines.next() {
+        Some(RECORD_VERSION) => {}
+        Some(_) => {
+            return Err(format!(
+                "the backup at {} was written by an earlier version of this runner, whose \
+                 record layout this one cannot safely interpret. It names a file and holds \
+                 its original contents after the second line. Restore it by hand, then \
+                 delete it.",
+                backup.display()
+            ))
+        }
+        None => return Err(malformed()),
+    }
+
+    let (Some(file), Some(expected_mutated), Some(expected_original), Some(original)) =
+        (lines.next(), lines.next(), lines.next(), lines.next())
+    else {
+        return Err(malformed());
     };
 
     let path = root.join(file);
     let current = fs::read_to_string(&path)
         .map_err(|e| format!("cannot read {file} to check it before restoring: {e}"))?;
+    let current_digest = digest(&current);
 
-    // Only restore over the exact mutated content the backup expects. Anything else means
+    // Already correct: clear the record and say so. Writing the original again would be
+    // harmless but would also report a repair that did not happen.
+    if current_digest == expected_original {
+        let _ = fs::remove_file(&backup);
+        return Ok(Recovery::AlreadyRestored(file.to_owned()));
+    }
+
+    // Only restore over the exact mutated content the record expects. Anything else means
     // the file changed after the crash — by an editor, a rebase, another tool — and
     // overwriting it would destroy work to repair a problem the operator may not know
     // exists. Stop, keep both, and let a human decide.
-    if digest(&current) != expected_mutated {
+    if current_digest != expected_mutated {
         return Err(format!(
-            "a previous mutation run left {file} mutated, but its contents have changed \
-             since. Refusing to restore over them. The original is preserved at {}; \
-             compare it with the working copy and resolve by hand.",
+            "a previous mutation run left {file} mutated, but its contents match neither \
+             the mutation it applied nor the original it saved. Refusing to restore over \
+             them. The original is preserved at {}; compare it with the working copy and \
+             resolve by hand.",
             backup.display()
         ));
     }
 
     fs::write(&path, original).map_err(|e| format!("could not restore {file}: {e}"))?;
     let _ = fs::remove_file(&backup);
-    Ok(Some(file.to_owned()))
+    Ok(Recovery::Repaired(file.to_owned()))
 }
 
 /// FNV-1a, hex. Detects accidental divergence between a backup and a working copy, which
@@ -530,10 +998,467 @@ fn cargo() -> String {
 }
 
 #[cfg(test)]
-#[allow(clippy::expect_used)]
+#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
 mod tests {
     use super::crash_signature;
 
+    use super::{
+        backup_path, digest, legacy_lock_path, lock_path, restore_stranded, AcquireError, Recovery,
+        RunLock,
+    };
+    use std::fs;
+    use std::path::Path;
+    use std::process::Command;
+
+    /// A root with one source file, mutated, and the write-ahead record describing it —
+    /// the state a run that was killed mid-mutation leaves behind.
+    fn stranded(original: &str, mutated: &str) -> (tempfile::TempDir, std::path::PathBuf) {
+        let root = tempfile::tempdir().expect("tempdir");
+        let src = root.path().join("src.rs");
+        fs::write(&src, mutated).expect("write mutated");
+        fs::create_dir_all(root.path().join("target")).expect("target");
+        fs::write(backup_path(root.path()), record_for(original, mutated)).expect("record");
+        (root, src)
+    }
+
+    fn record_for(original: &str, mutated: &str) -> String {
+        format!(
+            "{}\nsrc.rs\n{}\n{}\n{original}",
+            super::RECORD_VERSION,
+            digest(mutated),
+            digest(original)
+        )
+    }
+
+    #[test]
+    fn an_interrupted_run_is_repaired_at_startup() {
+        // The interruption case, which happened twice in one afternoon: a run killed
+        // between injecting and restoring leaves the tree damaged, and the next thing to
+        // read it — a test suite, a reviewer, an editor — sees a defect nobody wrote.
+        let (root, src) = stranded("fn f() -> u8 { 1 }\n", "fn f() -> u8 { 2 }\n");
+
+        assert_eq!(
+            restore_stranded(root.path()).expect("must repair"),
+            Recovery::Repaired("src.rs".to_owned()),
+            "and must say what it repaired"
+        );
+        assert_eq!(fs::read_to_string(&src).unwrap(), "fn f() -> u8 { 1 }\n");
+    }
+
+    #[test]
+    fn repairing_removes_the_record_it_created() {
+        // "Including removal of files created by the run": a record left behind makes
+        // the *next* run refuse to start, having already done its job.
+        let (root, _src) = stranded("original\n", "mutated\n");
+        assert!(backup_path(root.path()).exists(), "precondition");
+
+        restore_stranded(root.path()).expect("must repair");
+        assert!(
+            !backup_path(root.path()).exists(),
+            "the write-ahead record outlived the repair it described"
+        );
+    }
+
+    #[test]
+    fn an_already_restored_source_with_a_stranded_record_is_not_a_conflict() {
+        // The crash window between `fs::write(original)` and `fs::remove_file(backup)`.
+        // The tree is already correct and only the record is stale — but the first
+        // version of this function compared against the *mutated* digest alone, so it
+        // read the original as "changed since" and demanded a hand resolution on a tree
+        // that needed none. Recording the original's digest too is what tells the two
+        // apart.
+        let (root, src) = stranded("original\n", "mutated\n");
+        fs::write(&src, "original\n").expect("simulate the completed restore");
+
+        assert_eq!(
+            restore_stranded(root.path()).expect("must not be an error"),
+            Recovery::AlreadyRestored("src.rs".to_owned()),
+            "an already-repaired tree is not a conflict"
+        );
+        assert_eq!(
+            fs::read_to_string(&src).unwrap(),
+            "original\n",
+            "and nothing should have been rewritten"
+        );
+        assert!(
+            !backup_path(root.path()).exists(),
+            "the stale record must be cleared, or the next run trips on it again"
+        );
+    }
+
+    #[test]
+    fn a_file_edited_since_the_interruption_is_never_clobbered() {
+        // The exact incident this guard was written for. After a killed run the file was
+        // restored by hand; the record then no longer matched, and the runner refused
+        // rather than overwriting the repair. Losing someone's work to fix a problem
+        // they had already fixed is worse than stopping.
+        //
+        // "By hand" has to mean *differently* from the recorded original, or it is the
+        // already-restored case above rather than drift.
+        let (root, src) = stranded("original\n", "mutated\n");
+        fs::write(&src, "repaired by hand\n").expect("hand repair");
+
+        let err = restore_stranded(root.path()).expect_err("must refuse");
+        assert!(err.contains("match neither"), "{err}");
+        assert!(
+            err.contains("resolve by hand"),
+            "it must say what to do: {err}"
+        );
+
+        assert_eq!(
+            fs::read_to_string(&src).unwrap(),
+            "repaired by hand\n",
+            "the refusal overwrote the very work it was protecting"
+        );
+        assert!(
+            backup_path(root.path()).exists(),
+            "and the original must be kept for the comparison it asks for"
+        );
+    }
+
+    #[test]
+    fn a_clean_tree_needs_no_repair_and_says_nothing() {
+        // No record, nothing to do — and no noise implying something was wrong.
+        let root = tempfile::tempdir().expect("tempdir");
+        assert_eq!(
+            restore_stranded(root.path()).expect("clean"),
+            Recovery::Nothing
+        );
+    }
+
+    #[test]
+    fn a_malformed_record_is_reported_not_ignored() {
+        // A truncated record — killed mid-write — means a file may still be mutated with
+        // no way to know which. Silence would let the next run measure a damaged tree.
+        let v = super::RECORD_VERSION;
+        for bad in [
+            String::new(),
+            v.to_owned(),
+            format!("{v}\nsrc.rs"),
+            format!("{v}\nsrc.rs\nonedigest"),
+        ] {
+            let root = tempfile::tempdir().expect("tempdir");
+            fs::create_dir_all(root.path().join("target")).expect("target");
+            fs::write(backup_path(root.path()), &bad).expect("write");
+            match restore_stranded(root.path()) {
+                Err(e) => assert!(e.contains("malformed"), "{bad:?}: {e}"),
+                // An empty file is indistinguishable from none by `read_to_string`, so
+                // `Nothing` is acceptable there and only there.
+                Ok(Recovery::Nothing) => assert!(bad.is_empty(), "silently ignored {bad:?}"),
+                Ok(other) => panic!("claimed {other:?} from a malformed record {bad:?}"),
+            }
+        }
+    }
+
+    #[test]
+    fn a_record_from_the_previous_protocol_is_reported_not_guessed_at() {
+        // The first layout was `file\ndigest(mutated)\noriginal`. Read as the current one
+        // it would take the *original's first line* for a digest and the rest for the
+        // file's contents — writing truncated bytes over a source file. Refusing is the
+        // only safe reading of a record this version did not write.
+        let root = tempfile::tempdir().expect("tempdir");
+        fs::create_dir_all(root.path().join("target")).expect("target");
+        fs::write(root.path().join("src.rs"), "mutated\n").expect("src");
+        fs::write(
+            backup_path(root.path()),
+            format!("src.rs\n{}\noriginal\n", digest("mutated\n")),
+        )
+        .expect("write v1 record");
+
+        let err = restore_stranded(root.path()).expect_err("must refuse");
+        assert!(err.contains("earlier version"), "{err}");
+        assert_eq!(
+            fs::read_to_string(root.path().join("src.rs")).unwrap(),
+            "mutated\n",
+            "a record it cannot read must not cause a write"
+        );
+    }
+
+    #[test]
+    fn a_record_naming_a_missing_file_reports_rather_than_panicking() {
+        let root = tempfile::tempdir().expect("tempdir");
+        fs::create_dir_all(root.path().join("target")).expect("target");
+        fs::write(
+            backup_path(root.path()),
+            format!(
+                "{}\ngone.rs\n{}\n{}\noriginal\n",
+                super::RECORD_VERSION,
+                digest("mutated"),
+                digest("original\n")
+            ),
+        )
+        .expect("write");
+
+        let err = restore_stranded(root.path()).expect_err("must report");
+        assert!(err.contains("gone.rs"), "{err}");
+    }
+
+    #[test]
+    fn a_restore_that_fails_keeps_the_record_and_stops_the_run() {
+        // The one failure that turns a recoverable tree into an unrecoverable one. The
+        // record must survive it — clearing the record after a failed restore leaves the
+        // file mutated with nothing left to repair it from.
+        use super::{finish, Verdict};
+
+        let (root, src) = stranded("original\n", "mutated\n");
+        let backup = backup_path(root.path());
+
+        let verdict = finish(
+            &src,
+            "original\n",
+            &backup,
+            "src.rs",
+            Verdict::Survived,
+            |_, _| {
+                Err(std::io::Error::new(
+                    std::io::ErrorKind::PermissionDenied,
+                    "injected at the restoration boundary",
+                ))
+            },
+        );
+
+        // The failure propagates, and as the one verdict that is fatal to a run rather
+        // than a result about a mutation.
+        match &verdict {
+            Verdict::NotRestored(why) => {
+                assert!(why.contains("src.rs"), "{why}");
+                assert!(why.contains("could not be restored"), "{why}");
+                assert!(
+                    why.contains("mutate-recover"),
+                    "it must say how to fix it: {why}"
+                );
+            }
+            _ => panic!("a failed restore must not be reported as a mutation result"),
+        }
+
+        assert!(
+            backup.exists(),
+            "the record was cleared after a failed restore"
+        );
+        assert_eq!(
+            fs::read_to_string(&src).unwrap(),
+            "mutated\n",
+            "precondition: the file is still mutated"
+        );
+
+        // And the tree is still recoverable, to the exact original bytes.
+        assert_eq!(
+            restore_stranded(root.path()).expect("must repair"),
+            Recovery::Repaired("src.rs".to_owned())
+        );
+        assert_eq!(fs::read_to_string(&src).unwrap(), "original\n");
+        assert!(!backup.exists());
+    }
+
+    #[test]
+    fn a_successful_restore_clears_the_record() {
+        // The other half, so the test above is not satisfied by a `finish` that never
+        // clears anything.
+        use super::{finish, Verdict};
+
+        let (root, src) = stranded("original\n", "mutated\n");
+        let backup = backup_path(root.path());
+
+        let verdict = finish(
+            &src,
+            "original\n",
+            &backup,
+            "src.rs",
+            Verdict::Caught { failing: 1 },
+            |p, c| fs::write(p, c),
+        );
+
+        assert!(matches!(verdict, Verdict::Caught { failing: 1 }));
+        assert_eq!(fs::read_to_string(&src).unwrap(), "original\n");
+        assert!(
+            !backup.exists(),
+            "a successful restore must clear its record"
+        );
+    }
+
+    #[test]
+    fn a_second_acquisition_is_refused_while_one_is_held() {
+        // Two runners corrupt each other's restores, and a concurrent `cargo test` sees
+        // a mutated tree and reports failures that are not real — which is how the need
+        // for this lock was found in the first place.
+        //
+        // An advisory lock belongs to the open file description, not to the process, so
+        // a second `open` + `try_lock` here contends exactly as another process would.
+        // The cross-*process* case is proved in `tests/lifecycle.rs`, because only a
+        // separate process can show that a killed holder's lock is released.
+        let root = tempfile::tempdir().expect("tempdir");
+        let held = RunLock::acquire(root.path()).expect("first acquires");
+
+        match RunLock::acquire(root.path()) {
+            Err(AcquireError::Contended) => {}
+            Err(other) => panic!("wrong reason: {other:?}"),
+            Ok(_) => panic!("two runners acquired the same tree"),
+        }
+
+        drop(held);
+        RunLock::acquire(root.path()).expect("released, so the next may start");
+    }
+
+    #[test]
+    fn releasing_the_lock_keeps_its_file_and_lets_the_next_run_take_it() {
+        // Deliberately the opposite of the assertion this replaces. The first protocol
+        // unlinked its lock on release, and that was the defect: a file whose identity
+        // changes under contention cannot be the thing two processes agree on. The v2
+        // file is a stable object to lock, so what must be asserted is *release* — that
+        // a later acquisition succeeds — not absence.
+        let root = tempfile::tempdir().expect("tempdir");
+        let lock = RunLock::acquire(root.path()).expect("acquire");
+        assert!(lock_path(root.path()).exists(), "precondition");
+        drop(lock);
+
+        assert!(
+            lock_path(root.path()).exists(),
+            "the lock file is the coordination object and must survive release"
+        );
+        let again = RunLock::acquire(root.path()).expect("the lock must be free again");
+        drop(again);
+    }
+
+    #[test]
+    fn the_pid_inside_the_lock_is_diagnostic_only() {
+        // It is written *after* the lock is held, and nothing consults it to decide
+        // ownership. Corrupting it must therefore change nothing about who may run —
+        // which is the property the first protocol did not have, where this same byte
+        // string *was* the decision.
+        let root = tempfile::tempdir().expect("tempdir");
+        let lock = RunLock::acquire(root.path()).expect("acquire");
+        let written = fs::read_to_string(lock_path(root.path())).expect("read");
+        assert_eq!(written.trim(), std::process::id().to_string());
+        drop(lock);
+
+        for garbage in ["", "0", "-1", "not-a-pid", "99999999999999999999"] {
+            fs::write(lock_path(root.path()), garbage).expect("corrupt");
+            let taken = RunLock::acquire(root.path())
+                .unwrap_or_else(|e| panic!("{garbage:?} changed the verdict: {e:?}"));
+            drop(taken);
+        }
+    }
+
+    #[test]
+    fn a_first_protocol_lock_naming_a_live_process_is_refused() {
+        // The migration case. The two protocols cannot coordinate — one locks a file,
+        // the other treats existence as the lock — so the only safe reading of a v1
+        // marker naming a live process is that a v1 runner may be in the tree.
+        let root = tempfile::tempdir().expect("tempdir");
+        fs::create_dir_all(root.path().join("target")).expect("target");
+        // A process that is certainly alive and certainly not us, owned and reaped here
+        // so the test leaves nothing running.
+        let mut helper = Command::new("sleep").arg("30").spawn().expect("helper");
+        fs::write(legacy_lock_path(root.path()), helper.id().to_string()).expect("v1 lock");
+
+        let verdict = RunLock::acquire(root.path());
+
+        let _ = helper.kill();
+        let _ = helper.wait();
+
+        match verdict {
+            Err(AcquireError::LegacyRunner(_)) => {}
+            Err(other) => panic!("wrong reason: {other:?}"),
+            Ok(_) => panic!("joined a run using the previous protocol"),
+        }
+    }
+
+    #[test]
+    fn an_unattributable_first_protocol_lock_is_refused_not_stolen() {
+        // Reclaiming an unattributable marker is fail-open: if its holder is in fact
+        // alive, two runners end up on one tree. Refusing is fail-safe and costs an
+        // operator one `rm`.
+        for garbage in ["", "0", "-1", "not-a-pid", "99999999999999999999"] {
+            let root = tempfile::tempdir().expect("tempdir");
+            fs::create_dir_all(root.path().join("target")).expect("target");
+            fs::write(legacy_lock_path(root.path()), garbage).expect("write");
+
+            match RunLock::acquire(root.path()) {
+                Err(AcquireError::LegacyUnattributable(_)) => {}
+                Err(other) => panic!("{garbage:?}: wrong reason: {other:?}"),
+                Ok(_) => panic!("a v1 marker holding {garbage:?} was stolen"),
+            }
+            assert!(
+                legacy_lock_path(root.path()).exists(),
+                "{garbage:?}: refusing must not remove the marker it refused to judge"
+            );
+        }
+    }
+
+    #[test]
+    fn a_first_protocol_lock_from_a_dead_process_does_not_block() {
+        // The other half: a marker left by a killed v1 run must not need a human before
+        // anything can run again.
+        let root = tempfile::tempdir().expect("tempdir");
+        fs::create_dir_all(root.path().join("target")).expect("target");
+
+        let mut child = Command::new("true").spawn().expect("spawn");
+        let dead = child.id();
+        child.wait().expect("reap");
+        fs::write(legacy_lock_path(root.path()), dead.to_string()).expect("write");
+
+        let lock = RunLock::acquire(root.path()).expect("a dead v1 marker must not block");
+        assert_eq!(
+            fs::read_to_string(legacy_lock_path(root.path()))
+                .unwrap()
+                .trim(),
+            std::process::id().to_string(),
+            "and the marker must now name us, so a v1 runner stands down"
+        );
+        drop(lock);
+        assert!(
+            !legacy_lock_path(root.path()).exists(),
+            "the v1 marker is existence-based, so releasing it means removing it"
+        );
+    }
+
+    #[test]
+    fn a_dead_holder_is_distinguished_from_an_unprobeable_one() {
+        use super::{holder_state, Holder};
+
+        let mut child = Command::new("true").spawn().expect("spawn");
+        let dead = child.id();
+        child.wait().expect("reap");
+
+        assert!(matches!(holder_state(&dead.to_string()), Holder::Dead(_)));
+        assert!(matches!(
+            holder_state(&std::process::id().to_string()),
+            Holder::Alive(_)
+        ));
+        // Not dead. Nothing was established at all, and the two must not be conflated:
+        // "the probe failed" is not evidence the process is gone.
+        assert!(matches!(holder_state(""), Holder::Unknown(_)));
+        assert!(matches!(holder_state("0"), Holder::Unknown(_)));
+        assert!(matches!(holder_state("-1"), Holder::Unknown(_)));
+    }
+
+    #[test]
+    fn a_live_holder_is_still_recognised() {
+        // The other half: the fix must not make every marker reclaimable, which would
+        // remove the protection entirely.
+        use super::{holder_state, Holder};
+        assert!(
+            matches!(
+                holder_state(&std::process::id().to_string()),
+                Holder::Alive(_)
+            ),
+            "this very process should read as alive"
+        );
+    }
+
+    #[test]
+    fn the_digest_distinguishes_the_contents_it_is_asked_about() {
+        // A regression test on these specific contents, and nothing more. It does not
+        // establish collision resistance and is not meant to: FNV-1a has none, and the
+        // function's own doc says so. What it pins is that the refusal above actually
+        // discriminates for the inputs it will see in practice — a mutated file versus
+        // a hand-repaired one.
+        assert_eq!(digest("mutated\n"), digest("mutated\n"));
+        assert_ne!(digest("mutated\n"), digest("repaired by hand\n"));
+        assert_ne!(digest(""), digest("\n"));
+        let _ = Path::new("");
+    }
+
     #[test]
     fn a_stack_overflow_is_recognised_as_a_crash() {
         // Captured verbatim from a run where removing the parser's recursion bound
diff --git a/xtask/src/rules.rs b/xtask/src/rules.rs
index 0202dac..bb12e33 100644
--- a/xtask/src/rules.rs
+++ b/xtask/src/rules.rs
@@ -22,9 +22,14 @@ pub(crate) fn allowed_internal_deps(crate_name: &str) -> Option<&'static [&'stat
         "nazm-diag" => &["nazm-span"],
         "nazm-syntax" => &["nazm-span", "nazm-diag"],
         "nazm-core" => &["nazm-span", "nazm-diag", "nazm-syntax"],
+        // Level 7 of the table in `docs/internal/architecture.md` §2, and the only level built.
+        // It reads the syntax tree and produces LLVM IR *as text*, so it needs no
+        // code-generator dependency — which is why `may_depend_on_codegen` below stays
+        // false for it.
+        "nazm-lir" => &["nazm-span", "nazm-diag", "nazm-syntax"],
         // The CLI deliberately does NOT depend on nazm-syntax: it goes through nazm-core,
         // so there is one entry point into the front end rather than two that can drift.
-        "nazm-cli" => &["nazm-span", "nazm-diag", "nazm-core"],
+        "nazm-cli" => &["nazm-span", "nazm-diag", "nazm-core", "nazm-lir"],
 
         // The measurement instrument answers to nothing. It must stay usable — and
         // auditable — with no compiler present, because it has to run against arms A1 and
@@ -54,11 +59,38 @@ pub(crate) fn unsafe_allowed(crate_name: &str) -> Option<&'static str> {
         .map(|(_, why)| *why)
 }
 
+/// Which crates may emit a backend artefact or invoke the native toolchain.
+///
+/// Separate from [`may_depend_on_codegen`] because they establish different things.
+/// That one reads the dependency graph; **this one is about capability**, which is what
+/// actually matters once a backend emits LLVM IR as *text* and therefore has no codegen
+/// dependency to find.
+///
+/// `nazm-lir` emits IR and nothing else — it never runs a tool. `nazm-cli` is the driver
+/// and runs `clang`, but does not itself generate IR. Splitting them this way is what
+/// keeps `nazm check` structurally unable to reach either.
+pub(crate) fn emission_allowlist() -> &'static [(&'static str, &'static [&'static str])] {
+    &[
+        ("crates/nazm-span", &[]),
+        ("crates/nazm-diag", &[]),
+        ("crates/nazm-syntax", &[]),
+        // The checker and evaluator. Empty on purpose: this is the check-only path.
+        ("crates/nazm-core", &[]),
+        ("crates/nazm-lir", &["emits LLVM IR"]),
+        (
+            "crates/nazm-cli",
+            &["emits LLVM IR", "invokes the native toolchain"],
+        ),
+        ("crates/nazm-bench", &[]),
+    ]
+}
+
 /// Crates that may depend on a code generator.
 ///
-/// The check this enables: nothing in the front end or the checker may reach Cranelift or
-/// LLVM. It is written before either exists, because the rule is cheap now and contentious
-/// later.
+/// The check this enables: nothing in the front end or the checker may **depend on**
+/// Cranelift or LLVM. It is written before either exists, because the rule is cheap now
+/// and contentious later. It says nothing about text emission — see
+/// [`emission_allowlist`] for that.
 pub(crate) fn may_depend_on_codegen(crate_name: &str) -> bool {
     matches!(
         crate_name,
@@ -95,6 +127,29 @@ mod tests {
         }
     }
 
+    #[test]
+    fn the_check_only_path_may_neither_emit_ir_nor_run_the_toolchain() {
+        // The separation the emission gate exists to keep. `nazm-core` is where `nazm
+        // check` lives, and it must not be able to reach either capability.
+        let list = emission_allowlist();
+        let find = |name: &str| {
+            list.iter()
+                .find(|(c, _)| *c == name)
+                .map(|(_, a)| *a)
+                .expect("every crate must be placed")
+        };
+        assert!(find("crates/nazm-core").is_empty());
+        assert!(find("crates/nazm-syntax").is_empty());
+        assert!(find("crates/nazm-diag").is_empty());
+        assert!(find("crates/nazm-bench").is_empty());
+        assert!(find("crates/nazm-lir").contains(&"emits LLVM IR"));
+        assert!(
+            !find("crates/nazm-lir").contains(&"invokes the native toolchain"),
+            "the emitter should produce text, not run tools"
+        );
+        assert!(find("crates/nazm-cli").contains(&"invokes the native toolchain"));
+    }
+
     #[test]
     fn the_front_end_may_not_reach_a_code_generator() {
         for c in ["nazm-syntax", "nazm-ty", "nazm-mir", "nazm-bench"] {
```

## New files — complete contents

### `crates/nazm-cli/src/build.rs`

```
//! `nazm build` — source to a native executable.
//!
//! The pipeline is the existing front end plus two new steps: `nazm_lir::lower` turns a
//! *checked* tree into the low-level representation, `nazm_lir::emit_llvm` turns that into
//! LLVM IR text, and `clang` turns that into a binary. No LLVM is linked into `nazm` —
//! `docs/internal/architecture.md` §1 says not to in year one, and `clang` reads IR from a file.
//!
//! # Publishing the result
//!
//! Everything is built under temporary names in the output's own directory and the
//! executable is moved into place only after `clang` succeeds. So a failed build leaves
//! an existing binary exactly as it was, rather than deleting it and then failing — which
//! is the behaviour that costs someone a working artefact at the worst moment.
//!
//! The output path is refused if it is the input, because a build that overwrote its own
//! source would be unrecoverable.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nazm_span::SourceFile;

use crate::{emit, exit};

/// Compile, or explain what stopped it.
pub(crate) fn run(
    input: &Path,
    out: Option<&Path>,
    json: bool,
    emit_ir: bool,
    opt_level: u8,
) -> ExitCode {
    let text = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: cannot read {}: {e}", input.display());
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let file = SourceFile::new(input.display().to_string(), text);

    let output = match out {
        Some(p) => p.to_path_buf(),
        // `prog.nz` → `prog`. A stem with no extension would collide with its source, and
        // the check below catches that rather than this guessing around it.
        None => PathBuf::from(input.file_stem().unwrap_or_default()),
    };

    if same_file(input, &output) {
        eprintln!(
            "nazm: refusing to write the executable over its own source ({}). \
             Pass -o with a different path.",
            output.display()
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }

    // Ordinary validation, lowering and emission, all on the interpreter's own stack —
    // `nazm_core::on_checked` never lets the tree out. Every one of those passes recurses
    // with the shape of the syntax tree, and so does dropping it and dropping the IR, so
    // running any of them on the caller's stack would make a documented bound depend on
    // whoever called us. Only the emitted text comes back, which is flat.
    //
    // A program that does not type-check is not a question about the backend's coverage,
    // so those diagnostics are reported first and lowering never runs.
    let ir = match nazm_core::on_checked(file.text(), |program| {
        nazm_lir::lower(program)
            .map(|module| nazm_lir::emit_llvm(&module, &input.display().to_string(), file.text()))
            .map_err(|e| e.diagnostics)
    }) {
        Ok(ir) => ir,
        Err(diagnostics) => {
            // `N0101` is this backend's "not yet", and it reads differently from a
            // program being wrong. Distinguished for the summary line only; both are a
            // check failure and neither produces output.
            let unsupported = diagnostics
                .iter()
                .any(|d| d.code == nazm_diag::codes::UNSUPPORTED_CONSTRUCT);
            emit(&diagnostics, &file, json);
            let n = diagnostics.len();
            if unsupported {
                eprintln!(
                    "nazm: {n} construct{} outside the native subset; nothing was compiled",
                    if n == 1 { "" } else { "s" }
                );
            } else {
                eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
            }
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };

    // Temporaries beside the output, so the final move is a rename within one filesystem
    // rather than a copy that can half-succeed.
    let stamp = std::process::id();
    let tmp_ll = sibling(&output, &format!(".nazm-{stamp}.ll"));
    let tmp_exe = sibling(&output, &format!(".nazm-{stamp}.out"));

    if let Err(e) = std::fs::write(&tmp_ll, &ir) {
        eprintln!("nazm: cannot write {}: {e}", tmp_ll.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }

    let status = Command::new("clang")
        .arg(format!("-O{opt_level}"))
        // The module carries no target triple, so clang supplies the host's and would
        // otherwise warn about overriding nothing.
        .arg("-Wno-override-module")
        .arg("-o")
        .arg(&tmp_exe)
        .arg(&tmp_ll)
        .output();

    let cleanup = |keep_ir: bool| {
        if !keep_ir {
            let _ = std::fs::remove_file(&tmp_ll);
        }
        let _ = std::fs::remove_file(&tmp_exe);
    };

    match status {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            cleanup(false);
            eprintln!(
                "nazm: `clang` was not found on PATH, and `nazm build` needs it to \
                 assemble the LLVM IR it generates.\n\
                 nazm: install the Xcode command line tools (`xcode-select --install`) \
                 or your platform's clang package. `nazm run` needs none of this."
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Err(e) => {
            cleanup(false);
            eprintln!("nazm: could not run `clang`: {e}");
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Ok(o) if !o.status.success() => {
            // Keep the IR: clang's complaint is about a file the user cannot see
            // otherwise, and a compiler bug is much easier to report with it in hand.
            cleanup(true);
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            eprintln!(
                "nazm: `clang` rejected the generated IR (exit {:?}). This is a bug in \
                 `nazm build`, not in your program — the IR has been kept at {} to \
                 report it with.",
                o.status.code(),
                tmp_ll.display()
            );
            ExitCode::from(exit::TOOLCHAIN_FAILED)
        }
        Ok(_) => {
            if emit_ir {
                let beside = with_suffix(&output, "ll");
                if let Err(e) = std::fs::rename(&tmp_ll, &beside) {
                    eprintln!("nazm: cannot write {}: {e}", beside.display());
                }
            } else {
                let _ = std::fs::remove_file(&tmp_ll);
            }
            // Published last, so an existing executable survives every failure above.
            if let Err(e) = std::fs::rename(&tmp_exe, &output) {
                let _ = std::fs::remove_file(&tmp_exe);
                eprintln!("nazm: cannot write {}: {e}", output.display());
                return ExitCode::from(exit::TOOLCHAIN_FAILED);
            }
            println!("{}", output.display());
            ExitCode::from(exit::OK)
        }
    }
}

/// Whether two paths name the same file, falling back to a textual comparison.
///
/// `canonicalize` fails on a path that does not exist yet, which is the normal case for
/// an output — so a plain comparison is the fallback rather than an error.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn sibling(output: &Path, suffix: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    output.with_file_name(name)
}

fn with_suffix(output: &Path, ext: &str) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    output.with_file_name(name)
}
```

### `crates/nazm-cli/tests/build.rs`

```
//! `nazm build`, end to end: source → executable → run it.
//!
//! Every case here compiles a real `.nz` file with the real `nazm` binary and executes
//! the result. A hand-written `.ll` proves `clang` exists and nothing about this compiler.
//!
//! # Two oracles, not one
//!
//! Each arithmetic case is checked against **an explicitly written expected result** and
//! against **the interpreter**. Either alone is weak in a different way: an expectation
//! can be written wrong, and differential agreement is satisfied by a bug both
//! implementations share — which, since one was written from the other's semantics, is the
//! likely kind.
//!
//! # Both optimisation levels
//!
//! Everything arithmetic is built at `-O0` and `-O2`. The specified behaviour must not
//! depend on the optimiser, and the way to get that wrong is `nsw`: it does not check for
//! overflow, it asserts overflow cannot happen, so `-O2` would be entitled to delete the
//! very branch meant to catch it. Running both levels is what would catch that.
//!
//! At `-O0` the locals are genuinely stored to and loaded from stack slots, so the
//! arithmetic executes on loaded values rather than being folded. At `-O2` LLVM may fold
//! it — and the *outcome must still be the specified one*, which is the stronger claim of
//! the two. M1a has no way to produce a value unknown at compile time: there are no
//! parameters and no input. That arrives with M1b.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

struct Built {
    dir: tempfile::TempDir,
    exe: PathBuf,
}

/// What running something produced.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn write_program(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("prog.nz");
    std::fs::write(&src, body).expect("write source");
    (dir, src)
}

fn build_at(body: &str, opt: u8) -> Result<Built, Ran> {
    let (dir, src) = write_program(body);
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .arg("--opt-level")
        .arg(opt.to_string())
        .output()
        .expect("run nazm build");
    if out.status.success() {
        Ok(Built { dir, exe })
    } else {
        Err(Ran {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

fn run_exe(b: &Built) -> Ran {
    let out = Command::new(&b.exe).output().expect("run the executable");
    let _ = &b.dir;
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn interpret(body: &str) -> Ran {
    let (dir, src) = write_program(body);
    let out = Command::new(NAZM)
        .args(["run", src.to_str().unwrap()])
        .output()
        .expect("run nazm run");
    let _ = dir;
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn program(body: &str) -> String {
    format!("fn main() -> Int {{\n{body}\n}}\n")
}

/// A program that must succeed, checked against its expected value and the interpreter,
/// at both optimisation levels.
fn expect_value(body: &str, expected: &str) {
    let src = program(body);
    let interp = interpret(&src);
    assert_eq!(
        interp.code,
        Some(0),
        "the interpreter did not accept this program: {}",
        interp.stderr
    );
    assert_eq!(
        interp.stdout.trim(),
        expected,
        "the *expectation* disagrees with the interpreter — one of them is wrong, and \
         differential agreement alone would have hidden it"
    );

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).unwrap_or_else(|e| panic!("-O{opt} build failed: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(0), "-O{opt}: {ran:?}");
        assert_eq!(ran.stdout.trim(), expected, "-O{opt}");
        assert_eq!(
            ran.stdout.trim(),
            interp.stdout.trim(),
            "-O{opt}: the native program and the interpreter disagree"
        );
    }
}

/// A program that must fail at run time with a given code, at both optimisation levels.
fn expect_runtime_error(body: &str, code: &str, message: &str) {
    let src = program(body);
    let interp = interpret(&src);
    assert_eq!(
        interp.code,
        Some(2),
        "the interpreter did not fail: {interp:?}"
    );
    assert!(
        interp.stderr.contains(code) && interp.stderr.contains(message),
        "the expectation disagrees with the interpreter: {}",
        interp.stderr
    );

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).unwrap_or_else(|e| panic!("-O{opt} build failed: {e:?}"));
        let ran = run_exe(&built);
        assert_eq!(
            ran.code,
            Some(2),
            "-O{opt}: a specified runtime failure must exit 2, not {:?}. stderr: {}",
            ran.code,
            ran.stderr
        );
        assert!(
            ran.stderr.contains(code),
            "-O{opt}: missing diagnostic code {code}: {}",
            ran.stderr
        );
        assert!(
            ran.stderr.contains(message),
            "-O{opt}: missing message {message:?}: {}",
            ran.stderr
        );
        assert!(
            ran.stderr.contains("prog.nz:"),
            "-O{opt}: a runtime error must say where: {}",
            ran.stderr
        );
        assert!(ran.stdout.is_empty(), "-O{opt}: nothing should be printed");
    }
}

// ---------------------------------------------------------------------------
// Successful arithmetic
// ---------------------------------------------------------------------------

#[test]
fn ordinary_arithmetic_agrees_with_the_interpreter() {
    expect_value("    1 + 2 * 3 - 4", "3");
    expect_value("    (1 + 2) * (3 - 4)", "-3");
    expect_value("    100 / 7", "14");
    expect_value("    100 % 7", "2");
    expect_value("    -(3 * 4)", "-12");
}

#[test]
fn boundary_values_survive_the_round_trip() {
    // The full 64-bit range has to reach standard output. An exit status is 8 bits, so a
    // compiler that encoded the result there would be wrong for almost every `Int`.
    expect_value("    9223372036854775807", "9223372036854775807");
    expect_value("    -9223372036854775808", "-9223372036854775808");
    expect_value("    9223372036854775807 - 1", "9223372036854775806");
    expect_value("    -9223372036854775808 + 1", "-9223372036854775807");
}

#[test]
fn negative_division_and_remainder_truncate_toward_zero() {
    // The behaviour Nazm specifies, and the one that differs between languages — Python
    // floors. Written out rather than derived, so the expectation is a claim about the
    // specification and not about Rust.
    expect_value("    -7 / 2", "-3");
    expect_value("    -7 % 2", "-1");
    expect_value("    7 / -2", "-3");
    expect_value("    7 % -2", "1");
    expect_value("    -7 / -2", "3");
    expect_value("    -7 % -2", "-1");
}

#[test]
fn the_minimum_divided_into_by_minus_one_is_zero_for_remainder() {
    // `docs/user/spec.md`: `%` has exactly one failure case, a zero divisor. `Int::MIN % -1`
    // is mathematically 0 and 0 is representable, so it succeeds — even though the
    // hardware instruction that computes it faults on this exact pair, because it
    // computes the quotient. A backend that reached `srem` here would die.
    expect_value(
        "    let a = -9223372036854775808;\n    let b = -1;\n    a % b",
        "0",
    );
}

#[test]
fn local_bindings_resolve_and_nest() {
    expect_value("    let x = 10;\n    let y = 32;\n    x + y", "42");
    // Later bindings may read earlier ones.
    expect_value(
        "    let a = 2;\n    let b = a * 3;\n    let c = b + a;\n    c",
        "8",
    );
    // A binding's initialiser sees the *outer* binding of the same name, which is what
    // the checker says and what a slot-based resolver could easily get wrong.
    expect_value("    let x = 1;\n    let y = x + 10;\n    y", "11");
}

// ---------------------------------------------------------------------------
// Each exceptional case, separately
// ---------------------------------------------------------------------------

#[test]
fn addition_overflow_traps() {
    expect_runtime_error(
        "    let m = 9223372036854775807;\n    m + 1",
        "N0400",
        "this addition overflows `Int`",
    );
}

#[test]
fn subtraction_overflow_traps() {
    expect_runtime_error(
        "    let m = -9223372036854775808;\n    m - 1",
        "N0400",
        "this subtraction overflows `Int`",
    );
}

#[test]
fn multiplication_overflow_traps() {
    expect_runtime_error(
        "    let m = 9223372036854775807;\n    m * 2",
        "N0400",
        "this multiplication overflows `Int`",
    );
}

#[test]
fn negating_the_minimum_traps() {
    expect_runtime_error(
        "    let m = -9223372036854775808;\n    -m",
        "N0400",
        "this negation overflows `Int`",
    );
}

#[test]
fn division_by_zero_traps() {
    expect_runtime_error("    let z = 0;\n    7 / z", "N0401", "division by zero");
}

#[test]
fn remainder_by_zero_traps() {
    expect_runtime_error("    let z = 0;\n    7 % z", "N0401", "division by zero");
}

#[test]
fn the_minimum_divided_by_minus_one_traps() {
    // The quotient is one past the largest `Int`, so it is an overflow rather than a
    // division error — and the instruction would fault in hardware if it were reached.
    expect_runtime_error(
        "    let a = -9223372036854775808;\n    let b = -1;\n    a / b",
        "N0400",
        "this division overflows `Int`",
    );
}

#[test]
fn the_leftmost_failure_is_the_one_reported() {
    // Evaluation order is specified left to right, and here it is observable: both
    // operands would fail, and which diagnostic appears is the evidence that the order
    // was honoured.
    let src = program("    let z = 0;\n    let m = 9223372036854775807;\n    (m + 1) / z");
    let interp = interpret(&src);
    assert!(interp.stderr.contains("N0400"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).expect("build");
        let ran = run_exe(&built);
        assert!(
            ran.stderr.contains("N0400"),
            "-O{opt}: the right operand's failure was reported first: {}",
            ran.stderr
        );
    }
}

#[test]
fn the_two_implementations_report_the_same_source_position() {
    // A runtime error from a compiled program should point where the same error from
    // `nazm run` points. The compiled one has no syntax tree to consult — the position
    // is resolved at compile time and baked in as a constant — so agreement here is a
    // real check on that arithmetic, not a tautology.
    let src = "fn main() -> Int {\n    let z = 0;\n    7 / z\n}\n";

    let interp = interpret(src);
    assert_eq!(interp.code, Some(2));
    assert!(
        interp.stderr.contains("prog.nz:3:7"),
        "the interpreter moved: {}",
        interp.stderr
    );

    let built = build_at(src, 0).expect("build");
    let ran = run_exe(&built);
    assert!(
        ran.stderr.contains("prog.nz:3:7"),
        "the compiled program points elsewhere: {}",
        ran.stderr
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Everything outside the subset, refused by name and with nothing written.
#[test]
fn constructs_outside_the_subset_are_refused_and_nothing_is_emitted() {
    // `if` and the comparison operators were on this list in M1a and are not any more:
    // M1b compiles them. What is left is what the backend still cannot lower.
    let cases: &[(&str, &str)] = &[
        (
            "fn main() -> Int {\n    let mut x = 1;\n    x = 2;\n    x\n}\n",
            "`let mut`",
        ),
        (
            "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 { i = i + 1; }\n    i\n}\n",
            "`while`",
        ),
        ("fn main() -> Int {\n    return 1;\n}\n", "`return`"),
        (
            "fn f() -> Int { 1 }\nfn main() -> Int {\n    f()\n}\n",
            "a function call",
        ),
        ("fn main() -> Str {\n    \"hi\"\n}\n", "a `Str` value"),
        (
            "fn main() -> Int {\n    { 1 }\n}\n",
            "a block used as an expression",
        ),
        (
            "fn main() -> Int {\n    if true { 1 } else { 2 };\n    3\n}\n",
            "an expression statement",
        ),
    ];

    for (src, expected) in cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prog.nz");
        std::fs::write(&path, src).expect("write");
        let exe = dir.path().join("prog");

        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(1), "{expected}: {stderr}");
        assert!(stderr.contains("N0101"), "{expected}: {stderr}");
        assert!(
            stderr.contains(expected),
            "expected {expected:?} in: {stderr}"
        );
        assert!(
            stderr.contains("nazm run"),
            "{expected}: a refusal should say what still works: {stderr}"
        );
        assert!(
            !exe.exists(),
            "{expected}: an unsupported program produced an executable"
        );

        // And the same program still runs under the interpreter, which is the point of
        // calling it unsupported rather than invalid.
        let ran = Command::new(NAZM)
            .args(["run", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{expected}: `nazm run` should still accept it: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
}

#[test]
fn a_program_that_does_not_check_is_a_check_failure_not_a_subset_failure() {
    // The distinction matters: one says "your program is wrong", the other says "this
    // compiler cannot do that yet". Reporting the first as the second would be confusing
    // and the reverse would be a lie.
    let (dir, src) = write_program("fn main() -> Int {\n    undefined_name\n}\n");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr.contains("N0200"), "{stderr}");
    assert!(
        !stderr.contains("N0101"),
        "reported as unsupported: {stderr}"
    );
}

#[test]
fn build_diagnostics_are_available_as_json() {
    let (dir, src) = write_program("fn main() -> Int {\n    let mut x = 1;\n    x\n}\n");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--json"])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let first = stderr.lines().next().unwrap_or_default();
    assert!(first.starts_with('{'), "not JSON: {first}");
    assert!(first.contains("N0101"), "{first}");
}

// ---------------------------------------------------------------------------
// The toolchain, and the output path
// ---------------------------------------------------------------------------

#[test]
fn a_missing_clang_is_reported_as_a_toolchain_problem_not_a_program_problem() {
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        // Nothing on PATH, so `clang` cannot be found.
        .env("PATH", "")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(4),
        "a missing tool must not be reported as a bad program: {stderr}"
    );
    assert!(stderr.contains("clang"), "{stderr}");
    assert!(
        stderr.contains("nazm run"),
        "it should say what still works without clang: {stderr}"
    );
}

#[test]
fn a_failing_clang_is_reported_with_its_own_output() {
    // A stub earlier on PATH than the real one. This exercises the path where the
    // toolchain runs and refuses, which is different from it being absent.
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let stub = bin.join("clang");
    std::fs::write(&stub, "#!/bin/sh\necho 'stub: refusing' >&2\nexit 1\n").expect("stub");
    set_executable(&stub);

    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .env("PATH", &bin)
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(4), "{stderr}");
    assert!(
        stderr.contains("stub: refusing"),
        "clang's own output was lost: {stderr}"
    );
    assert!(
        stderr.contains("bug in `nazm build`"),
        "IR the compiler generated being rejected is a compiler bug, and should say so: {stderr}"
    );
}

#[test]
fn an_existing_executable_survives_a_failed_build() {
    // The failure that costs someone a working artefact: deleting the output, then
    // failing. Everything is built under a temporary name and moved into place last.
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let exe = dir.path().join("prog");
    std::fs::write(&exe, "an earlier, working build").expect("seed");

    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .env("PATH", "")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(
        std::fs::read_to_string(&exe).expect("read"),
        "an earlier, working build",
        "a failed build destroyed the previous executable"
    );
}

#[test]
fn the_output_may_not_overwrite_the_input() {
    let (_dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let before = std::fs::read_to_string(&src).expect("read");

    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap()])
        .arg("-o")
        .arg(&src)
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_ne!(out.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("own source"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&src).expect("read"),
        before,
        "the build overwrote its own source"
    );
}

#[test]
fn the_generated_ir_can_be_kept_for_inspection() {
    let (dir, src) = write_program("fn main() -> Int {\n    1 + 1\n}\n");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(0));

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("the IR");
    assert!(ir.contains("@llvm.sadd.with.overflow.i64"), "{ir}");
    assert!(!ir.contains(" nsw "), "emitted nsw: {ir}");
    assert!(!ir.contains("llvm.trap"), "emitted llvm.trap: {ir}");
}

#[test]
fn run_and_check_are_unchanged_by_the_new_subcommand() {
    // M1a must not alter either. The examples are the same ones the README quotes.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    for (file, expected) in [
        ("examples/max.nz", "25"),
        ("examples/sum.nz", "6074"),
        ("examples/primes.nz", "1060"),
        ("examples/search.nz", "1723"),
    ] {
        let path = root.join(file);
        let out = Command::new(NAZM)
            .args(["run", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(out.status.code(), Some(0), "{file}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            expected,
            "{file}"
        );

        let out = Command::new(NAZM)
            .args(["check", path.to_str().unwrap()])
            .output()
            .expect("check");
        assert_eq!(out.status.code(), Some(0), "{file}");
    }
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let mut p = std::fs::metadata(path).expect("metadata").permissions();
    p.set_mode(0o755);
    std::fs::set_permissions(path, p).expect("chmod");
}

/// The native toolchain is present, asserted separately and first.
///
/// Without this, a machine with no `clang` fails every compiling test in this file, and a
/// mutation run reads that as *every mutant caught* — a coverage claim manufactured by a
/// missing tool. Asserted on its own so the baseline check in `cargo xtask mutate` refuses
/// to start instead: "tests already fail before any mutation" is the truth, and "caught"
/// is not.
#[test]
fn the_native_toolchain_is_available() {
    let out = Command::new("clang").arg("--version").output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => panic!("`clang --version` exited {:?}", o.status.code()),
        Err(e) => panic!(
            "`clang` is not usable ({e}). Every compiling test in this file needs it, and \
             a run without it measures the toolchain rather than the compiler."
        ),
    }
}

// ---------------------------------------------------------------------------
// M1b: Bool, comparisons, and value-position if/else
// ---------------------------------------------------------------------------

#[test]
fn every_comparison_operator_agrees_with_the_interpreter() {
    // Each of the six, with operands on either side of the answer and on it.
    for (op, a, b, expect) in [
        ("<", 1, 2, 1),
        ("<", 2, 2, 0),
        ("<", 3, 2, 0),
        ("<=", 1, 2, 1),
        ("<=", 2, 2, 1),
        ("<=", 3, 2, 0),
        (">", 3, 2, 1),
        (">", 2, 2, 0),
        (">", 1, 2, 0),
        (">=", 3, 2, 1),
        (">=", 2, 2, 1),
        (">=", 1, 2, 0),
        ("==", 2, 2, 1),
        ("==", 2, 3, 0),
        ("!=", 2, 3, 1),
        ("!=", 2, 2, 0),
    ] {
        expect_value(
            &format!("    if {a} {op} {b} {{ 1 }} else {{ 0 }}"),
            &expect.to_string(),
        );
    }
}

#[test]
fn comparisons_are_signed_at_the_boundaries() {
    // The case an unsigned predicate gets backwards: as bit patterns, `-1` is the
    // largest 64-bit value. `Int` is signed, so `-1 < 1`.
    expect_value("    if -1 < 1 { 1 } else { 0 }", "1");
    expect_value(
        "    if -9223372036854775808 < 9223372036854775807 { 1 } else { 0 }",
        "1",
    );
    expect_value("    if -9223372036854775808 >= 0 { 1 } else { 0 }", "0");
    // Equality at the extremes, where a truncating comparison would also go wrong.
    expect_value(
        "    let m = -9223372036854775808;\n    if m == -9223372036854775808 { 1 } else { 0 }",
        "1",
    );
}

#[test]
fn bool_locals_and_bool_equality_work() {
    expect_value("    let t = true;\n    if t { 7 } else { 8 }", "7");
    expect_value("    let f = false;\n    if f { 7 } else { 8 }", "8");
    // `==` on two `Bool`s: the checker admits equality on any single type.
    expect_value("    if true == true { 1 } else { 0 }", "1");
    expect_value("    if true != false { 1 } else { 0 }", "1");
    expect_value(
        "    let a = 1 < 2;\n    let b = 3 < 2;\n    if a == b { 1 } else { 0 }",
        "0",
    );
}

#[test]
fn both_branch_outcomes_are_reachable() {
    expect_value(
        "    let n = 5;\n    if n > 0 { n * 2 } else { 0 - n }",
        "10",
    );
    expect_value(
        "    let n = 0 - 5;\n    if n > 0 { n * 2 } else { 0 - n }",
        "5",
    );
}

#[test]
fn conditionals_nest_in_conditions_and_in_branches() {
    // An `if` producing a `Bool`, used as another `if`'s condition — a `Bool`
    // intermediate result that never becomes the program's value.
    expect_value(
        "    let a = 12;\n    let b = 5;\n    \
         if if a > b { a % b == 2 } else { false } { 1 } else { 0 }",
        "1",
    );
    // And nested in a branch.
    expect_value(
        "    let n = 7;\n    if n > 0 { if n > 5 { 2 } else { 1 } } else { 0 }",
        "2",
    );
    expect_value(
        "    let n = 3;\n    if n > 0 { if n > 5 { 2 } else { 1 } } else { 0 }",
        "1",
    );
    // Three deep, so the phi in the middle has a nested join as its predecessor.
    expect_value(
        "    let n = 9;\n    \
         if n > 0 { if n > 5 { if n > 8 { 3 } else { 2 } } else { 1 } } else { 0 }",
        "3",
    );
}

#[test]
fn branch_local_bindings_and_shadowing() {
    // A binding made inside a branch, used only there.
    expect_value(
        "    let n = 4;\n    if n > 0 { let double = n * 2; double + 1 } else { 0 }",
        "9",
    );
    // Shadowing an outer name inside a branch. `docs/user/spec.md` allows this in a nested
    // scope and refuses it in the same one, so the branch is exactly where it is legal.
    expect_value(
        "    let x = 1;\n    if true { let x = 100; x } else { x }",
        "100",
    );
    // And the outer binding is unaffected on the other path.
    expect_value(
        "    let x = 1;\n    if false { let x = 100; x } else { x }",
        "1",
    );
    // Both branches may bind the same name without colliding: distinct slots.
    expect_value(
        "    let c = true;\n    if c { let v = 10; v } else { let v = 20; v }",
        "10",
    );
}

#[test]
fn conditionals_compose_with_arithmetic_and_comparisons() {
    // An `if` as an operand, which is where a wrong phi type or a missing join shows up.
    expect_value("    1 + if true { 10 } else { 20 }", "11");
    expect_value("    (if false { 10 } else { 20 }) * 2", "40");
    expect_value(
        "    if (if true { 3 } else { 4 }) < 4 { 1 } else { 0 }",
        "1",
    );
    // Arithmetic inside a branch, so the branch ends in a guard's continuation block
    // rather than in the block it started in — the phi-predecessor case.
    expect_value(
        "    let a = 6;\n    if a > 0 { a * 7 + 1 } else { 0 }",
        "43",
    );
}

#[test]
fn a_failure_in_the_branch_not_taken_does_not_run() {
    // The property a conditional branch gives for free, and the one most worth pinning:
    // only the selected branch is evaluated.
    expect_value("    if true { 1 } else { 1 / 0 }", "1");
    expect_value("    if false { 1 / 0 } else { 2 }", "2");
    expect_value(
        "    let m = 9223372036854775807;\n    if false { m + 1 } else { 5 }",
        "5",
    );
    expect_value(
        "    let z = 0;\n    let m = -9223372036854775808;\n    \
         if 1 > 2 { m / -1 } else { if 2 > 1 { 42 } else { 7 / z } }",
        "42",
    );
}

#[test]
fn a_failure_in_the_branch_taken_reports_correctly() {
    expect_runtime_error(
        "    if true { 1 / 0 } else { 1 }",
        "N0401",
        "division by zero",
    );
    expect_runtime_error(
        "    let m = 9223372036854775807;\n    if 1 < 2 { m + 1 } else { 0 }",
        "N0400",
        "this addition overflows `Int`",
    );
    expect_runtime_error(
        "    let m = -9223372036854775808;\n    if false { 0 } else { m / -1 }",
        "N0400",
        "this division overflows `Int`",
    );
}

#[test]
fn the_failing_branch_reports_its_own_source_position() {
    // Not the `if`'s position, and not the other branch's: the operator that failed.
    let src = "fn main() -> Int {\n    let z = 0;\n    if true {\n        7 / z\n    } else {\n        1\n    }\n}\n";
    let interp = interpret(src);
    assert_eq!(interp.code, Some(2));
    assert!(interp.stderr.contains("prog.nz:4:11"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(src, opt).expect("build");
        let ran = run_exe(&built);
        assert_eq!(ran.code, Some(2), "-O{opt}: {ran:?}");
        assert!(
            ran.stderr.contains("prog.nz:4:11"),
            "-O{opt}: wrong position: {}",
            ran.stderr
        );
    }
}

#[test]
fn among_evaluated_operands_the_first_failure_still_wins() {
    // Left-to-right, and here both operands are on the taken path. The condition is
    // evaluated before either branch, so a failing condition beats a failing branch too.
    let src = program(
        "    let z = 0;\n    let m = 9223372036854775807;\n    \
         if (m + 1) > 0 { 7 / z } else { 0 }",
    );
    let interp = interpret(&src);
    assert!(interp.stderr.contains("N0400"), "{}", interp.stderr);

    for opt in [0u8, 2] {
        let built = build_at(&src, opt).expect("build");
        let ran = run_exe(&built);
        assert!(
            ran.stderr.contains("N0400"),
            "-O{opt}: the branch's failure was reported before the condition's: {}",
            ran.stderr
        );
    }
}

#[test]
fn the_condition_is_evaluated_exactly_once() {
    // A condition that fails must fail once, and reaching the branch at all would mean
    // it had been evaluated twice or not at all. Observable here because the failure is
    // in the condition and both branches are fine.
    expect_runtime_error(
        "    let z = 0;\n    if (7 / z) > 0 { 1 } else { 2 }",
        "N0401",
        "division by zero",
    );
}

#[test]
fn front_end_type_errors_are_not_reported_as_backend_limits() {
    // A non-`Bool` condition and mismatched branch types are the checker's business.
    // Reporting them as "not in the subset" would tell the user to wait for a release
    // that will never make their program correct.
    for (src, code) in [
        (
            "fn main() -> Int {\n    if 1 { 2 } else { 3 }\n}\n",
            "N0301",
        ),
        (
            "fn main() -> Int {\n    if true { 1 } else { false }\n}\n",
            "N0302",
        ),
    ] {
        let (dir, path) = write_program(src);
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(dir.path().join("prog"))
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains(code), "expected {code}: {stderr}");
        assert!(
            !stderr.contains("N0101"),
            "a type error was reported as a backend limit: {stderr}"
        );
    }
}

#[test]
fn unsupported_constructs_inside_a_branch_are_refused_by_the_backend() {
    let cases: &[(&str, &str)] = &[
        (
            "fn main() -> Int {\n    if true { let mut x = 1; x = 2; x } else { 0 }\n}\n",
            "`let mut`",
        ),
        (
            "fn main() -> Int {\n    if true { return 1; } else { 0 }\n}\n",
            "`return`",
        ),
        (
            "fn f() -> Int { 1 }\nfn main() -> Int {\n    if true { f() } else { 0 }\n}\n",
            "a function call",
        ),
        (
            "fn main() -> Int {\n    if \"a\" == \"b\" { 1 } else { 0 }\n}\n",
            "a `Str` value",
        ),
    ];

    for (src, expected) in cases {
        let (dir, path) = write_program(src);
        let exe = dir.path().join("prog");
        let out = Command::new(NAZM)
            .args(["build", path.to_str().unwrap()])
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(1), "{expected}: {stderr}");
        assert!(stderr.contains("N0101"), "{expected}: {stderr}");
        assert!(stderr.contains(expected), "expected {expected:?}: {stderr}");
        assert!(
            !exe.exists(),
            "{expected}: an unsupported program produced an executable"
        );
    }
}

#[test]
fn a_statement_position_if_is_still_refused() {
    // The one `if` this backend compiles is the one in value position. A statement `if`
    // arrives as an expression statement, which stays refused — deliberately, so that
    // adding value-position `if` did not quietly also add the other one.
    let src = "fn main() -> Int {\n    let x = 1;\n    if x > 0 { let y = 2; }\n    x\n}\n";
    let (dir, path) = write_program(src);
    let out = Command::new(NAZM)
        .args(["build", path.to_str().unwrap()])
        .arg("-o")
        .arg(dir.path().join("prog"))
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("an expression statement"), "{stderr}");

    // And `nazm run` still executes it, which is what makes this a backend limit.
    let ran = Command::new(NAZM)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("run");
    assert_eq!(
        ran.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

#[test]
fn the_emitted_ir_for_a_conditional_is_well_formed() {
    // clang accepting the module is the real check — it verifies the IR, including that
    // every phi names actual predecessors. These assertions add *which* construct was
    // used, which a successful compile alone does not say.
    let (dir, src) =
        write_program("fn main() -> Int {\n    let a = 6;\n    if a > 5 { a * 7 } else { 0 }\n}\n");
    let exe = dir.path().join("prog");
    let out = Command::new(NAZM)
        .args(["build", src.to_str().unwrap(), "--emit-ir"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join("prog.ll")).expect("the IR");
    assert!(ir.contains("icmp sgt i64"), "{ir}");
    assert!(ir.contains("br i1 %"), "{ir}");
    assert!(ir.contains("phi i64"), "{ir}");
    assert!(!ir.contains(" nsw "), "{ir}");
    assert!(!ir.contains("llvm.trap"), "{ir}");
}

// ---------------------------------------------------------------------------
// Stack safety, out of process
// ---------------------------------------------------------------------------

/// Deep-but-accepted and over-limit syntax, through the real binary.
///
/// The failure mode being guarded against is **the process dying**, which cannot be
/// observed from inside the process it kills — so this runs `nazm` as a child and asserts
/// on its exit status. `status.code()` is `None` when a child is killed by a signal, and
/// that is the assertion that matters.
///
/// Exercised on: `aarch64-apple-darwin`, against the child's main thread. Other platforms
/// and other thread stack sizes are unverified.
#[test]
fn deep_syntax_completes_or_diagnoses_but_never_aborts() {
    // A long binary chain is the deep shape the compiled subset can actually express:
    // it builds one tree level per operator while the parser never recurses.
    let chain = |n: usize| {
        format!(
            "fn main() -> Int {{\n    {}\n}}\n",
            vec!["1"; n].join(" + ")
        )
    };

    for (n, what) in [(511, "at the accepted edge"), (600, "past the limit")] {
        let (dir, src) = write_program(&chain(n));
        for cmd in ["check", "build"] {
            let mut c = Command::new(NAZM);
            c.args([cmd, src.to_str().unwrap()]);
            if cmd == "build" {
                c.arg("-o").arg(dir.path().join("prog"));
            }
            let out = c.output().expect("run");

            let code = out.status.code();
            assert!(
                code.is_some(),
                "`nazm {cmd}` on a {n}-term chain ({what}) was killed by a signal — a \
                 depth bound that aborts is not a bound. stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let code = code.unwrap_or(-1);
            assert!(
                code == 0 || code == 1,
                "`nazm {cmd}` on a {n}-term chain exited {code}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            if n > 511 {
                assert_eq!(code, 1, "a {n}-term chain should be refused");
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("N0102"),
                    "expected the nesting diagnostic"
                );
            }
        }
    }
}

/// The same for nesting that only `nazm check` can reach.
///
/// Nested blocks are the most expensive shape per level, and they are outside the native
/// subset — so this is about the checker's inherited contract rather than the backend's.
#[test]
fn deeply_nested_blocks_diagnose_rather_than_abort() {
    for n in [255usize, 400] {
        let src = format!(
            "fn main() -> Int {{\n{}1{}\n}}\n",
            "{ ".repeat(n),
            " }".repeat(n)
        );
        let (_dir, path) = write_program(&src);
        let out = Command::new(NAZM)
            .args(["check", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert!(
            out.status.code().is_some(),
            "`nazm check` on {n} nested blocks was killed by a signal"
        );
    }
}

/// `nazm check` must not need the native toolchain.
///
/// The behavioural half of the emission-reach gate in `xtask/src/rules.rs`: that gate is a
/// source-level check on where `clang` may be mentioned, and this is the part no source
/// scan can establish — that the check-only path does not actually invoke it.
#[test]
fn check_does_not_need_clang() {
    let (_dir, src) = write_program("fn main() -> Int {\n    if 1 < 2 { 3 } else { 4 }\n}\n");
    let out = Command::new(NAZM)
        .args(["check", src.to_str().unwrap()])
        .env("PATH", "")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "`nazm check` failed with no toolchain on PATH: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
```

### `crates/nazm-lir/Cargo.toml`

```rust
[package]
name = "nazm-lir"
description = "The typed low-level representation, and the LLVM IR text a backend reads."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false

[dependencies]
nazm-span = { workspace = true }
nazm-diag = { workspace = true }
nazm-syntax = { workspace = true }

[lints]
workspace = true
```

### `crates/nazm-lir/src/emit.rs`

```rust
//! [`crate::ir`] to LLVM IR text.
//!
//! # The contract this file exists to keep
//!
//! `docs/user/spec.md` says overflow **traps, identically in every build**. The obvious way to
//! write signed arithmetic in LLVM is `add nsw`, and it is exactly wrong: `nsw` does not
//! check for overflow, it *promises overflow cannot happen* and makes the result poison
//! if it does. An optimiser is then entitled to assume the promise and delete the branch
//! that was meant to catch it — so `nsw` would turn a specified trap into whatever
//! `-O2` felt like. Every fallible operation here goes through
//! `llvm.{sadd,ssub,smul}.with.overflow.i64` and an explicit branch, which is a value the
//! optimiser has to respect.
//!
//! `llvm.trap` is not used either. It raises `SIGILL`, and a process killed by a signal
//! is not the runtime-error contract Nazm promises: the spec says the program *stops with
//! a diagnostic*. So a failing operation calls a generated helper that writes the
//! diagnostic to standard error and exits 2 — the same status `nazm run` uses for a
//! runtime failure.
//!
//! The four cases that are easy to get wrong, and what is emitted for each:
//!
//! | Case | Emitted |
//! | --- | --- |
//! | `a / 0`, `a % 0` | divisor compared to zero *before* any division instruction |
//! | `Int::MIN / -1` | both operands compared, and the failure reported as an overflow — the quotient has no representation |
//! | `Int::MIN % -1` | **`0`**, via a `phi`. Never reaches `srem`, which faults in hardware on exactly this pair |
//! | `-Int::MIN` | `ssub.with.overflow(0, x)`, the only negation without a representable result |
//!
//! `sdiv` and `srem` are reached only on operands already proved safe, so the guards are
//! not belt-and-braces — they are what stops the hardware fault that `docs/user/spec.md`
//! describes.
//!
//! # Blocks, and why the current one is tracked
//!
//! Emission is linear: every instruction is appended, and opening a label continues into
//! it. That works for straight-line code and for the guards above, and it would quietly
//! produce *wrong* `phi` nodes for `if` without [`Emitter::current`].
//!
//! A `phi` names the block each incoming value came from. The naive answer is the label
//! the branch started at — and that is wrong whenever the branch emitted any block of its
//! own, which happens for a nested `if`, and for any checked arithmetic, since a guard
//! opens a continuation block. The predecessor is the block holding the terminator that
//! jumps to the join, which is whatever block emission *ended* in. So [`Emitter::open`]
//! records it and the `phi` reads it back after each branch.

use std::fmt::Write as _;

use crate::ir::{ArithOp, Block, Expr, Function, Module, Stmt, Ty};

/// What this emitter has been verified against, recorded so a reader does not have to
/// infer portability from silence.
pub const TARGET_NOTE: &str = "verified on aarch64-apple-darwin with Apple clang; other \
                               targets are unverified rather than unsupported";

/// The diagnostic a failing operation reports, matching the interpreter's wording.
struct Failure {
    code: &'static str,
    message: String,
}

struct Emitter {
    out: String,
    /// Next SSA value number. LLVM requires unnamed values to be numbered consecutively
    /// within a function, so this also numbers blocks to keep them distinct.
    next: u32,
    /// The block instructions are currently being appended to — the predecessor a `phi`
    /// must name. See the module documentation.
    current: String,
    /// Error strings, interned so a program with the same failure twice carries it once.
    strings: Vec<(String, String)>,
    /// Where the source came from, for the runtime message.
    file: String,
    /// Byte offset of the start of each line, for turning a span into a position.
    lines: Vec<usize>,
    source_len: usize,
}

impl Emitter {
    fn fresh(&mut self) -> u32 {
        let n = self.next;
        self.next += 1;
        n
    }

    /// Start a block and make it the current one.
    fn open(&mut self, label: &str) {
        let _ = writeln!(self.out, "{label}:");
        self.current = label.to_owned();
    }

    /// `line:column`, 1-based, from a byte offset. Mirrors the renderer's convention so a
    /// runtime error from a compiled program points where the same error from
    /// `nazm run` would.
    fn position(&self, offset: u32) -> (usize, usize) {
        let offset = (offset as usize).min(self.source_len);
        let line = self.lines.partition_point(|&start| start <= offset);
        let line_start = self.lines.get(line.saturating_sub(1)).copied().unwrap_or(0);
        (line.max(1), offset - line_start + 1)
    }

    /// Intern a NUL-terminated constant and return `(symbol, byte length)`.
    fn intern(&mut self, text: &str) -> (String, usize) {
        if let Some((sym, _)) = self.strings.iter().find(|(_, t)| t == text) {
            return (sym.clone(), text.len() + 1);
        }
        let sym = format!("@.msg{}", self.strings.len());
        self.strings.push((sym.clone(), text.to_owned()));
        (sym, text.len() + 1)
    }

    /// Emit the branch that reports `failure` and never returns, then continue after it.
    ///
    /// The failure block ends in `unreachable`, so it is never a predecessor of anything:
    /// a `phi` downstream cannot receive an incoming value from a path that terminated.
    fn guard(&mut self, cond: u32, failure: &Failure, span_offset: u32) {
        let (line, col) = self.position(span_offset);
        let text = format!(
            "nazm: error[{}]: {} at {}:{line}:{col}\n",
            failure.code, failure.message, self.file
        );
        let (sym, len) = self.intern(&text);
        let fail = self.fresh();
        let cont = self.fresh();
        let _ = writeln!(self.out, "  br i1 %{cond}, label %b{fail}, label %b{cont}");
        self.open(&format!("b{fail}"));
        let _ = writeln!(self.out, "  call void @nazm_fail(ptr {sym}, i64 {len})");
        let _ = writeln!(self.out, "  unreachable");
        self.open(&format!("b{cont}"));
    }

    /// Statements, then the block's value.
    fn block(&mut self, b: &Block) -> u32 {
        for stmt in &b.stmts {
            match stmt {
                Stmt::Let { slot, value } => {
                    let v = self.expr(value);
                    let ty = value.ty().llvm();
                    let _ = writeln!(self.out, "  store {ty} %{v}, ptr %slot{slot}");
                }
            }
        }
        self.expr(&b.result)
    }

    fn expr(&mut self, e: &Expr) -> u32 {
        match e {
            Expr::Int(v) => {
                // Materialised through an `add` rather than used inline so that every
                // expression has a value number and the emitter has one shape to reason
                // about. `-O2` folds it; `-O0` does not, which is what makes the
                // arithmetic below genuinely execute during testing.
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = add i64 {v}, 0");
                n
            }
            Expr::Bool(v) => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = or i1 {v}, false");
                n
            }
            Expr::Load { slot, ty } => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = load {}, ptr %slot{slot}", ty.llvm());
                n
            }
            Expr::Neg { operand, span } => {
                let v = self.expr(operand);
                let r = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{r} = call {{i64, i1}} @llvm.ssub.with.overflow.i64(i64 0, i64 %{v})"
                );
                let ov = self.fresh();
                let _ = writeln!(self.out, "  %{ov} = extractvalue {{i64, i1}} %{r}, 1");
                self.guard(
                    ov,
                    &Failure {
                        code: "N0400",
                        message: "this negation overflows `Int`".to_owned(),
                    },
                    span.start,
                );
                let out = self.fresh();
                let _ = writeln!(self.out, "  %{out} = extractvalue {{i64, i1}} %{r}, 0");
                out
            }
            Expr::Arith { op, lhs, rhs, span } => {
                // Left, then right. `docs/user/spec.md` specifies the order, and here it is
                // observable: it decides which diagnostic a program with two failing
                // operands reports.
                let a = self.expr(lhs);
                let b = self.expr(rhs);
                match op {
                    ArithOp::Add | ArithOp::Sub | ArithOp::Mul => {
                        self.checked(*op, a, b, span.start)
                    }
                    ArithOp::Div => self.divide(a, b, span.start),
                    ArithOp::Rem => self.remainder(a, b, span.start),
                }
            }
            Expr::Compare {
                op,
                operand_ty,
                lhs,
                rhs,
            } => {
                // Same left-to-right order, for the same reason: either operand may be
                // arithmetic that fails.
                let a = self.expr(lhs);
                let b = self.expr(rhs);
                let out = self.fresh();
                // No guard, because no comparison can fail: `icmp` on two `i64`s or two
                // `i1`s is total.
                let _ = writeln!(
                    self.out,
                    "  %{out} = icmp {} {} %{a}, %{b}",
                    op.predicate(),
                    operand_ty.llvm()
                );
                out
            }
            Expr::If {
                cond,
                then_block,
                else_block,
                ty,
            } => self.conditional(cond, then_block, else_block, *ty),
        }
    }

    /// `if`, as a conditional branch and a join with a `phi`.
    ///
    /// The condition is emitted once, before either branch, and only the selected
    /// branch's block is ever entered — so a failure in the branch not taken cannot run.
    /// That is not an optimisation; it is what a conditional branch means.
    fn conditional(&mut self, cond: &Expr, then_b: &Block, else_b: &Block, ty: Ty) -> u32 {
        let c = self.expr(cond);
        let then_label = format!("b{}", self.fresh());
        let else_label = format!("b{}", self.fresh());
        let join_label = format!("b{}", self.fresh());
        let _ = writeln!(
            self.out,
            "  br i1 %{c}, label %{then_label}, label %{else_label}"
        );

        self.open(&then_label);
        let then_v = self.block(then_b);
        // Read **after** lowering the branch, not before: the branch may have opened
        // blocks of its own — a nested `if`, or any checked arithmetic, whose guard opens
        // a continuation. The predecessor the `phi` must name is where emission ended.
        let then_pred = self.current.clone();
        let _ = writeln!(self.out, "  br label %{join_label}");

        self.open(&else_label);
        let else_v = self.block(else_b);
        let else_pred = self.current.clone();
        let _ = writeln!(self.out, "  br label %{join_label}");

        self.open(&join_label);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = phi {} [ %{then_v}, %{then_pred} ], [ %{else_v}, %{else_pred} ]",
            ty.llvm()
        );
        out
    }

    fn checked(&mut self, op: ArithOp, a: u32, b: u32, offset: u32) -> u32 {
        let intrinsic = match op {
            ArithOp::Add => "sadd",
            ArithOp::Sub => "ssub",
            ArithOp::Mul => "smul",
            // Division is guarded by comparison, not by an intrinsic: there is no
            // `sdiv.with.overflow`.
            ArithOp::Div | ArithOp::Rem => unreachable!("guarded separately"),
        };
        let r = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{r} = call {{i64, i1}} @llvm.{intrinsic}.with.overflow.i64(i64 %{a}, i64 %{b})"
        );
        let ov = self.fresh();
        let _ = writeln!(self.out, "  %{ov} = extractvalue {{i64, i1}} %{r}, 1");
        self.guard(
            ov,
            &Failure {
                code: "N0400",
                message: format!("this {} overflows `Int`", op.overflow_noun()),
            },
            offset,
        );
        let out = self.fresh();
        let _ = writeln!(self.out, "  %{out} = extractvalue {{i64, i1}} %{r}, 0");
        out
    }

    /// Both of division's failures, in the order the interpreter checks them.
    fn divide(&mut self, a: u32, b: u32, offset: u32) -> u32 {
        let zero = self.fresh();
        let _ = writeln!(self.out, "  %{zero} = icmp eq i64 %{b}, 0");
        self.guard(
            zero,
            &Failure {
                code: "N0401",
                message: "division by zero".to_owned(),
            },
            offset,
        );

        let overflows = self.min_over_minus_one(a, b);
        self.guard(
            overflows,
            &Failure {
                code: "N0400",
                message: "this division overflows `Int`".to_owned(),
            },
            offset,
        );

        let out = self.fresh();
        let _ = writeln!(self.out, "  %{out} = sdiv i64 %{a}, %{b}");
        out
    }

    /// Remainder, whose *only* failure is a zero divisor.
    ///
    /// `Int::MIN % -1` is mathematically zero and zero is representable, so `docs/user/spec.md`
    /// specifies it as `0`. The `srem` instruction faults on that pair anyway — it
    /// computes the quotient — so the pair is answered with a constant and never reaches
    /// the instruction. Getting this wrong would give `%` a second failure case the
    /// language says it does not have.
    fn remainder(&mut self, a: u32, b: u32, offset: u32) -> u32 {
        let zero = self.fresh();
        let _ = writeln!(self.out, "  %{zero} = icmp eq i64 %{b}, 0");
        self.guard(
            zero,
            &Failure {
                code: "N0401",
                message: "division by zero".to_owned(),
            },
            offset,
        );

        let special = self.min_over_minus_one(a, b);
        let is_zero = format!("b{}", self.fresh());
        let compute = format!("b{}", self.fresh());
        let join = format!("b{}", self.fresh());
        let _ = writeln!(
            self.out,
            "  br i1 %{special}, label %{is_zero}, label %{compute}"
        );
        self.open(&compute);
        let r = self.fresh();
        let _ = writeln!(self.out, "  %{r} = srem i64 %{a}, %{b}");
        let _ = writeln!(self.out, "  br label %{join}");
        self.open(&is_zero);
        let _ = writeln!(self.out, "  br label %{join}");
        self.open(&join);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = phi i64 [ %{r}, %{compute} ], [ 0, %{is_zero} ]"
        );
        out
    }

    /// `a == Int::MIN && b == -1` — the one operand pair `sdiv` and `srem` fault on.
    fn min_over_minus_one(&mut self, a: u32, b: u32) -> u32 {
        let is_min = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{is_min} = icmp eq i64 %{a}, -9223372036854775808"
        );
        let is_m1 = self.fresh();
        let _ = writeln!(self.out, "  %{is_m1} = icmp eq i64 %{b}, -1");
        let both = self.fresh();
        let _ = writeln!(self.out, "  %{both} = and i1 %{is_min}, %{is_m1}");
        both
    }
}

/// Emit a complete, self-contained LLVM IR module.
///
/// The result is text on purpose: `docs/internal/architecture.md` §1 says not to link against LLVM
/// in year one, and `clang` reads IR directly, so this needs no LLVM dependency at all.
#[must_use]
pub fn emit_llvm(module: &Module, file: &str, source: &str) -> String {
    let mut line_starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let mut e = Emitter {
        out: String::new(),
        next: 0,
        current: "entry".to_owned(),
        strings: Vec::new(),
        file: file.to_owned(),
        lines: line_starts,
        source_len: source.len(),
    };

    let Function { locals, body } = &module.main;

    // Slots are named rather than numbered so that the numbered SSA values stay
    // consecutive, which LLVM requires of unnamed values. All allocas sit in `entry`,
    // which is where LLVM expects them; a slot belonging to a branch is still only
    // *stored to* on that branch's path.
    let _ = writeln!(e.out, "define i32 @main() {{");
    let _ = writeln!(e.out, "entry:");
    for (i, local) in locals.iter().enumerate() {
        let _ = writeln!(
            e.out,
            "  %slot{i} = alloca {}            ; {} : {}",
            local.ty.llvm(),
            local.name,
            local.ty.name()
        );
    }
    let v = e.block(body);
    let _ = writeln!(
        e.out,
        "  %print = call i32 (ptr, ...) @printf(ptr @.fmt, i64 %{v})"
    );
    let _ = writeln!(e.out, "  ret i32 0");
    let _ = writeln!(e.out, "}}");

    let mut head = String::new();
    let _ = writeln!(head, "; Generated by nazm build. Do not edit.");
    let _ = writeln!(head, "; {TARGET_NOTE}");
    let _ = writeln!(head, "source_filename = \"{file}\"");
    head.push('\n');
    let _ = writeln!(
        head,
        "declare {{i64, i1}} @llvm.sadd.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        head,
        "declare {{i64, i1}} @llvm.ssub.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        head,
        "declare {{i64, i1}} @llvm.smul.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(head, "declare i32 @printf(ptr noundef, ...)");
    let _ = writeln!(head, "declare i64 @write(i32, ptr, i64)");
    let _ = writeln!(head, "declare void @exit(i32) noreturn");
    head.push('\n');
    // `%lld\n`: the full signed 64-bit value in decimal. Encoding the result in the exit
    // status instead would truncate it to 8 bits, which for a language whose `Int` is
    // 64-bit would be a lie in the common case.
    let _ = writeln!(
        head,
        "@.fmt = private unnamed_addr constant [6 x i8] c\"%lld\\0A\\00\""
    );
    for (sym, text) in &e.strings {
        let _ = writeln!(
            head,
            "{sym} = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            text.len() + 1,
            escape(text)
        );
    }
    head.push('\n');
    // Not `llvm.trap`: a process killed by SIGILL is not "stops with a diagnostic".
    let _ = writeln!(
        head,
        "define internal void @nazm_fail(ptr %m, i64 %n) noreturn {{"
    );
    let _ = writeln!(head, "  %w = call i64 @write(i32 2, ptr %m, i64 %n)");
    let _ = writeln!(head, "  call void @exit(i32 2)");
    let _ = writeln!(head, "  unreachable");
    let _ = writeln!(head, "}}");
    head.push('\n');

    head + &e.out
}

/// LLVM's constant-string escaping: `\XX` for anything not printable ASCII.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b == b'"' || b == b'\\' || !(0x20..0x7f).contains(&b) {
            let _ = write!(out, "\\{b:02X}");
        } else {
            out.push(b as char);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ir::{Block, CmpOp, Expr, Function, Local, Module, Stmt};
    use nazm_span::Span;

    fn block(result: Expr) -> Block {
        Block {
            stmts: Vec::new(),
            result: Box::new(result),
        }
    }

    fn module(result: Expr, locals: Vec<Local>, stmts: Vec<Stmt>) -> Module {
        Module {
            main: Function {
                locals,
                body: Block {
                    stmts,
                    result: Box::new(result),
                },
            },
        }
    }

    fn ir_of(m: &Module) -> String {
        emit_llvm(m, "t.nz", "a source line\nand another\n")
    }

    #[test]
    fn overflow_is_an_intrinsic_and_a_branch_never_nsw() {
        // The single most important property of this file. `nsw` does not check for
        // overflow — it promises overflow cannot happen, which licenses the optimiser to
        // delete the very branch that was supposed to catch it.
        let m = module(
            Expr::Arith {
                op: ArithOp::Add,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(2)),
                span: Span::new(0, 1),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        assert!(ir.contains("@llvm.sadd.with.overflow.i64"), "{ir}");
        assert!(ir.contains("br i1 %"), "{ir}");
        assert!(!ir.contains(" nsw "), "emitted nsw: {ir}");
        assert!(!ir.contains("llvm.trap"), "emitted llvm.trap: {ir}");
    }

    #[test]
    fn division_is_guarded_before_the_instruction_that_would_fault() {
        let m = module(
            Expr::Arith {
                op: ArithOp::Div,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(0)),
                span: Span::new(2, 3),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        let guard = ir.find("icmp eq i64").expect("a zero check");
        let sdiv = ir.find("sdiv").expect("the division");
        assert!(guard < sdiv, "the division precedes its guard:\n{ir}");
        assert!(ir.contains("-9223372036854775808"), "no MIN/-1 check: {ir}");
        assert!(ir.contains("N0401"), "{ir}");
    }

    #[test]
    fn remainder_answers_min_by_minus_one_with_a_constant() {
        let m = module(
            Expr::Arith {
                op: ArithOp::Rem,
                lhs: Box::new(Expr::Int(i64::MIN)),
                rhs: Box::new(Expr::Int(-1)),
                span: Span::new(0, 1),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        assert!(
            ir.contains("phi i64"),
            "no constant path for MIN % -1: {ir}"
        );
        assert!(ir.contains("srem"), "{ir}");
        assert!(ir.contains("N0401"), "{ir}");
        assert!(
            !ir.contains("remainder overflows"),
            "`%` was given an overflow case it does not have: {ir}"
        );
    }

    #[test]
    fn a_runtime_message_carries_its_code_and_position() {
        let m = module(
            Expr::Arith {
                op: ArithOp::Div,
                lhs: Box::new(Expr::Int(1)),
                rhs: Box::new(Expr::Int(0)),
                // The `/` on the third line: 18 bytes of preceding lines plus six into
                // this one, which is column 7.
                span: Span::new(24, 25),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = emit_llvm(&m, "prog.nz", "line one\nline two\n    1 / 0\n");
        assert!(ir.contains("prog.nz:3:7"), "{ir}");
        assert!(ir.contains("N0401"), "{ir}");
    }

    #[test]
    fn locals_are_slots_and_carry_their_type() {
        let m = module(
            Expr::Load {
                slot: 0,
                ty: Ty::Bool,
            },
            vec![Local {
                name: "flag".to_owned(),
                ty: Ty::Bool,
            }],
            vec![Stmt::Let {
                slot: 0,
                value: Expr::Bool(true),
            }],
        );
        let ir = ir_of(&m);
        assert!(ir.contains("%slot0 = alloca i1"), "{ir}");
        assert!(ir.contains("store i1 %"), "{ir}");
        assert!(ir.contains("load i1, ptr %slot0"), "{ir}");
    }

    #[test]
    fn a_comparison_is_signed_and_needs_no_guard() {
        // `slt`, not `ult`: `Int` is signed, so `-1 < 1` must hold. An unsigned predicate
        // would compare bit patterns and get it backwards.
        let m = module(
            Expr::Compare {
                op: CmpOp::Lt,
                operand_ty: Ty::Int,
                lhs: Box::new(Expr::Int(-1)),
                rhs: Box::new(Expr::Int(1)),
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);
        assert!(ir.contains("icmp slt i64"), "{ir}");
        assert!(!ir.contains("icmp ult"), "unsigned comparison: {ir}");
    }

    #[test]
    fn a_phi_names_blocks_that_exist_and_reach_the_join() {
        // The bug this test exists for: naming the branch's *entry* label. A branch
        // containing checked arithmetic ends in the continuation block its guard opened,
        // not in the block it started in, so an entry-label `phi` would name a
        // predecessor that does not branch to the join — which LLVM rejects.
        //
        // Structural, not a snapshot: block numbers are an emission detail and pinning
        // them would fail on any unrelated change.
        let m = module(
            Expr::If {
                cond: Box::new(Expr::Bool(true)),
                // Arithmetic in the branch, so the branch opens blocks of its own.
                then_block: Box::new(block(Expr::Arith {
                    op: ArithOp::Add,
                    lhs: Box::new(Expr::Int(1)),
                    rhs: Box::new(Expr::Int(2)),
                    span: Span::new(0, 1),
                })),
                else_block: Box::new(block(Expr::Int(0))),
                ty: Ty::Int,
            },
            Vec::new(),
            Vec::new(),
        );
        let ir = ir_of(&m);

        let phi = ir
            .lines()
            .find(|l| l.contains("phi i64") && l.contains("], ["))
            .expect("a phi for the if")
            .to_owned();

        let preds: Vec<String> = phi
            .split(", %")
            .skip(1)
            .filter_map(|s| s.split([' ', ']', ',']).next())
            .map(ToOwned::to_owned)
            .collect();
        assert_eq!(
            preds.len(),
            2,
            "a two-way phi needs two predecessors: {phi}"
        );

        for pred in preds {
            assert!(
                ir.contains(&format!("\n{pred}:")),
                "phi names `{pred}`, which is not a block:\n{ir}"
            );
            // And that block must actually branch to the join the phi is in.
            let after = ir.split(&format!("\n{pred}:")).nth(1).unwrap_or_default();
            let terminator = after
                .lines()
                .find(|l| l.trim_start().starts_with("br "))
                .unwrap_or_default();
            assert!(
                terminator.contains("br label %"),
                "`{pred}` does not jump to a join:\n{ir}"
            );
        }
    }

    #[test]
    fn the_result_is_printed_in_full_not_returned_as_a_status() {
        // An exit status is 8 bits. Returning a 64-bit `Int` through it would be wrong
        // for almost every value a program produces.
        let m = module(Expr::Int(i64::MIN), Vec::new(), Vec::new());
        let ir = ir_of(&m);
        assert!(ir.contains("@printf"), "{ir}");
        assert!(ir.contains("%lld"), "{ir}");
        assert!(ir.contains("ret i32 0"), "{ir}");
    }
}
```

### `crates/nazm-lir/src/ir.rs`

```rust
//! The representation itself.
//!
//! Locals are slots, not names: resolution finishes in [`crate::lower`], so the emitter
//! never sees an identifier and cannot accidentally reintroduce a scoping rule. Every
//! operation that can fail carries the span the failure should be reported at, because a
//! runtime error with no position is the class of error this project exists to stop
//! producing — and the generated program has no syntax tree to consult.
//!
//! # Types are carried, not inferred
//!
//! Every expression can state its own type through [`Expr::ty`]. The emitter needs the
//! type of an `if` to write its `phi`, and the type of an `==` to choose how to compare —
//! and re-deriving either from surrounding context is how a backend acquires a second,
//! subtly different type system. So `Load`, `If` and `Compare` record what the checker
//! already established, and nothing downstream guesses.
//!
//! # Why arithmetic and comparison are separate operators
//!
//! The syntax tree has one `BinOp` covering both. Splitting them here is deliberate: they
//! have different operand rules (`+` is `Int` only; `==` admits any single type), different
//! result types, and different failure modes (`+` can overflow, `<` cannot). One enum
//! would let a `match` arm written for arithmetic silently start accepting comparisons.

use nazm_span::Span;

/// The value types this backend can represent. Not the language's — `Str` exists and is
/// refused by name in [`crate::lower`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
}

impl Ty {
    /// The LLVM type name.
    #[must_use]
    pub fn llvm(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Bool => "i1",
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
        }
    }
}

/// One compiled program. M1b has exactly one function, and the type says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub main: Function,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Slot `i` is `locals[i]`. Slots are unique for the whole function even where names
    /// are not: a branch-local binding and an outer one that shadows it get different
    /// slots, so the emitter never has to know about scopes.
    pub locals: Vec<Local>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    /// Kept for `--emit-ir` to be readable by a person. Never used to resolve anything.
    pub name: String,
    pub ty: Ty,
}

/// Statements, then the value the block produces.
///
/// The same shape for a function body and for an `if` branch, because they are the same
/// thing in the language: `docs/user/spec.md` has one `Block`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub result: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Bind `value` to a slot. Immutable: M1b has no assignment, so a slot is written
    /// exactly once and the emitter needs no notion of reassignment.
    Let { slot: usize, value: Expr },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Load {
        slot: usize,
        ty: Ty,
    },
    /// Fails on `Int::MIN`, which is the only negation without a representable result.
    Neg {
        operand: Box<Expr>,
        span: Span,
    },
    /// `Int, Int -> Int`. Every one of these can fail.
    Arith {
        op: ArithOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// `T, T -> Bool`. None of these can fail.
    Compare {
        op: CmpOp,
        /// The **operand** type, not the result — the result is always `Bool`. The
        /// emitter needs this to pick the comparison width.
        operand_ty: Ty,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Value-position `if`. Both branches are present and produce `ty`.
    ///
    /// There is no statement-position variant, and that is the point: this backend
    /// compiles the `if` that produces a value and refuses the one that does not, rather
    /// than having one node whose meaning depends on where it sits.
    If {
        cond: Box<Expr>,
        then_block: Box<Block>,
        else_block: Box<Block>,
        ty: Ty,
    },
}

impl Expr {
    /// What this expression produces.
    ///
    /// Total, and derived from what the node recorded rather than from context — the
    /// emitter calls it to size a `phi` and to pick a comparison, and a wrong answer
    /// there is a miscompile rather than a diagnostic.
    #[must_use]
    pub fn ty(&self) -> Ty {
        match self {
            Self::Int(_) | Self::Neg { .. } | Self::Arith { .. } => Ty::Int,
            Self::Bool(_) | Self::Compare { .. } => Ty::Bool,
            Self::Load { ty, .. } | Self::If { ty, .. } => *ty,
        }
    }
}

/// The five arithmetic operators, each of which can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl ArithOp {
    /// The word the interpreter uses in its overflow diagnostic, so the two
    /// implementations report the same sentence for the same failure.
    #[must_use]
    pub fn overflow_noun(self) -> &'static str {
        match self {
            Self::Add => "addition",
            Self::Sub => "subtraction",
            Self::Mul => "multiplication",
            Self::Div => "division",
            // `%` never overflows: see `emit`.
            Self::Rem => "remainder",
        }
    }
}

/// The six comparisons, none of which can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl CmpOp {
    /// The LLVM `icmp` predicate.
    ///
    /// Signed on purpose. `Int` is signed 64-bit, so `-1 < 1` must hold; `ult` would
    /// compare the two's-complement bit patterns and make it false. For `Bool` only `eq`
    /// and `ne` are reachable — ordering `Bool` is refused by the checker — and those two
    /// predicates carry no signedness.
    #[must_use]
    pub fn predicate(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::NotEq => "ne",
            Self::Lt => "slt",
            Self::LtEq => "sle",
            Self::Gt => "sgt",
            Self::GtEq => "sge",
        }
    }

    /// Whether this comparison orders its operands, and so needs `Int`.
    #[must_use]
    pub fn is_ordering(self) -> bool {
        !matches!(self, Self::Eq | Self::NotEq)
    }
}
```

### `crates/nazm-lir/src/lib.rs`

```rust
//! The one representation between the syntax tree and a backend, and the LLVM IR it emits.
//!
//! # Why there is one level here and not eight
//!
//! `docs/internal/architecture.md` §2 describes eight IR levels. This crate is level 7 and nothing
//! else. The levels in between exist to carry information M1a has none of: there are no
//! generics to monomorphise, no traits to lower to dictionaries, no regions to infer, no
//! effects to track, and one scalar type whose layout is not selectable. Building them
//! now would produce a chain of pass-throughs that matches the diagram and says nothing.
//!
//! What this level *does* carry is the thing the AST does not: which operations can fail,
//! and where the source said so. That is the whole reason for a separate representation
//! at this stage — the emitter must not have to re-derive from syntax whether a `/` needs
//! two guards or one.
//!
//! # Scope
//!
//! M1b: a single `fn main() -> Int`; `Int` and `Bool` literals; immutable `let` bindings
//! of either and references to them; unary negation; `+ - * / %`; the comparisons
//! `== != < <= > >=`; parentheses; and `if`/`else` **used as a value**, with branch-local
//! bindings and nesting. Everything else is rejected by name in [`lower`], after the
//! program has passed ordinary checking — never partially compiled.
//!
//! The accepted/rejected table is in `docs/user/roadmap.md` under *M1b subset*.

pub mod emit;
pub mod ir;
pub mod lower;

pub use emit::{emit_llvm, TARGET_NOTE};
pub use ir::{ArithOp, Block, CmpOp, Expr, Function, Local, Module, Stmt, Ty};
pub use lower::{lower, LowerError};
```

### `crates/nazm-lir/src/lower.rs`

```rust
//! Syntax tree to [`crate::ir`], with everything outside the native subset refused by name.
//!
//! # Refusal, not partial compilation
//!
//! Lowering runs *after* `nazm_core::check` has passed, so a program reaching here is
//! well-typed. That is exactly why the refusals below are about the backend's coverage
//! and not about the program being wrong: `while` is a perfectly good Nazm construct that
//! this increment cannot compile, and saying so is different from saying it is invalid.
//! Every refusal points at `nazm run`, which still executes all of it.
//!
//! Every unsupported construct produces a diagnostic and **no output**. A compiler that
//! emits something for a program it did not fully understand is worse than one that
//! stops, because the something runs.
//!
//! # The one `if`
//!
//! `docs/user/spec.md` has a single `if` whose meaning is decided by checking, not by syntax.
//! This backend compiles it **only in value position**, and the way that stays true is
//! that a statement-position `if` never arrives here as an `if` at all — it arrives as
//! `Stmt::Expr`, which is refused. There is no arm below that handles an `if` generically
//! and then discovers what else it covered.

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use nazm_syntax::{
    BinOp as AstOp, Block as AstBlock, Expr as AstExpr, Function as AstFn, Program, Stmt as AstStmt,
};

use crate::ir::{ArithOp, Block, CmpOp, Expr, Function, Local, Module, Stmt, Ty};

/// What the subset is, in one sentence, for every refusal to point at.
const SUBSET: &str = "`nazm build` currently compiles a single `fn main() -> Int` \
                      containing `Int` and `Bool` literals, immutable `let` bindings of \
                      either, references to them, unary `-`, the operators `+ - * / %`, \
                      the comparisons `== != < <= > >=`, parentheses, and `if`/`else` \
                      used as a value. Everything else still runs under `nazm run`";

/// Constructs outside the native subset, with the spans that named them.
#[derive(Debug)]
pub struct LowerError {
    pub diagnostics: Vec<Diagnostic>,
}

struct Ctx {
    /// Innermost-last. Each `if` branch pushes one, so a binding made inside a branch is
    /// invisible after the join — which is what stops a value that exists on only one
    /// path being read on both.
    scopes: Vec<Vec<(String, usize)>>,
    locals: Vec<Local>,
    errors: Vec<Diagnostic>,
}

impl Ctx {
    /// `what` is a short noun phrase. The sentence supplies the rest, and the help
    /// supplies the subset — a phrase carrying its own clause reads as two sentences
    /// spliced together, which is how the first version of these messages read.
    fn unsupported(&mut self, span: Span, what: &str) {
        self.errors.push(
            Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                span,
                format!("{what} is not in `nazm build`'s subset yet"),
            )
            .primary_label("not in the native subset")
            .help(SUBSET),
        );
    }

    fn resolve(&self, name: &str) -> Option<(usize, Ty)> {
        self.scopes.iter().rev().find_map(|s| {
            s.iter()
                .rev()
                .find(|(n, _)| n == name)
                .and_then(|(_, i)| self.locals.get(*i).map(|l| (*i, l.ty)))
        })
    }

    /// A fresh slot. Never reused, even for a name that shadows another: two bindings
    /// that are live at once must not share storage, and "is the old one still live?" is
    /// a question the emitter should never have to ask.
    fn bind(&mut self, name: &str, ty: Ty) -> usize {
        let slot = self.locals.len();
        self.locals.push(Local {
            name: name.to_owned(),
            ty,
        });
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_owned(), slot));
        }
        slot
    }

    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(Vec::new());
        let out = f(self);
        self.scopes.pop();
        out
    }
}

/// Lower a checked program, or explain what stopped it.
///
/// # Errors
///
/// *Every* construct outside the subset, not just the first: one run of `nazm build`
/// should say everything that would have to change, which is the same
/// one-mistake-one-error discipline the rest of the front end holds to.
pub fn lower(program: &Program) -> Result<Module, LowerError> {
    let mut ctx = Ctx {
        scopes: vec![Vec::new()],
        locals: Vec::new(),
        errors: Vec::new(),
    };

    let Some(main) = program.functions.iter().find(|f| f.name == "main") else {
        // Unreachable after a successful check, which reports N0204 itself. A diagnostic
        // rather than a panic: an unreachable branch that panics is a crash waiting for
        // the day it becomes reachable.
        return Err(LowerError {
            diagnostics: vec![Diagnostic::error(
                codes::MISSING_MAIN,
                Span::new(0, 0),
                "this program has no `main` to compile",
            )],
        });
    };

    for f in &program.functions {
        if f.name != "main" {
            ctx.unsupported(f.span, "a program with more than one function");
        }
    }

    let lowered = lower_main(&mut ctx, main);

    match (ctx.errors.is_empty(), lowered) {
        (true, Some(body)) => Ok(Module {
            main: Function {
                locals: ctx.locals,
                body,
            },
        }),
        (_, _) if !ctx.errors.is_empty() => Err(LowerError {
            diagnostics: ctx.errors,
        }),
        _ => Err(LowerError {
            diagnostics: vec![Diagnostic::error(
                codes::UNSUPPORTED_CONSTRUCT,
                main.span,
                "`nazm build` could not lower this program and did not say why",
            )
            .help("this is a bug in the compiler; `nazm run` still works")],
        }),
    }
}

fn lower_main(ctx: &mut Ctx, f: &AstFn) -> Option<Block> {
    if !f.params.is_empty() {
        ctx.unsupported(f.span, "a `main` with parameters");
    }
    if f.return_type.name != "Int" {
        ctx.unsupported(
            f.return_type.span,
            &format!("a `main` returning `{}`", f.return_type.name),
        );
    }
    let body = f.body.as_ref()?;
    // The body uses the scope already on the stack, so `main`'s top-level bindings are
    // not treated as branch-local.
    let block = lower_block(ctx, body)?;
    if block.result.ty() != Ty::Int {
        ctx.unsupported(
            body.span,
            &format!("a `main` whose value is `{}`", block.result.ty().name()),
        );
        return None;
    }
    Some(block)
}

/// A block used as a value: statements, then a tail.
///
/// Reached for `main`'s body and for an `if` branch, and for nothing else. A bare
/// `{ … }` in expression position is refused in [`lower_expr`] rather than routed here,
/// so "which blocks may this backend compile" has one answer written in one place.
fn lower_block(ctx: &mut Ctx, b: &AstBlock) -> Option<Block> {
    let mut stmts = Vec::new();
    for s in &b.statements {
        match s {
            AstStmt::Let(l) if !l.mutable => {
                // The initialiser is lowered *before* the name is bound, so `let x = x;`
                // reads the outer `x` exactly as the checker says it does.
                let value = lower_expr(ctx, &l.value);
                let ty = value.as_ref().map_or(Ty::Int, Expr::ty);
                let slot = ctx.bind(&l.name, ty);
                if let Some(value) = value {
                    stmts.push(Stmt::Let { slot, value });
                }
            }
            AstStmt::Let(l) => {
                ctx.unsupported(l.span, "`let mut`");
                // Bound anyway. The binding is real in the source, and leaving it out
                // would make every later mention of the name report a second error about
                // a name that is perfectly well defined — one mistake, several errors,
                // which is the thing this front end is built to avoid.
                let value = lower_expr(ctx, &l.value);
                let ty = value.as_ref().map_or(Ty::Int, Expr::ty);
                ctx.bind(&l.name, ty);
            }
            AstStmt::Assign(a) => ctx.unsupported(a.span, "assignment"),
            AstStmt::While(w) => ctx.unsupported(w.span, "`while`"),
            AstStmt::Return(r) => ctx.unsupported(r.span, "`return`"),
            AstStmt::Break { span } => ctx.unsupported(*span, "`break`"),
            AstStmt::Continue { span } => ctx.unsupported(*span, "`continue`"),
            // Where a statement-position `if` lands, and it stays refused.
            AstStmt::Expr(e) => ctx.unsupported(e.span(), "an expression statement"),
        }
    }

    let Some(tail) = &b.tail else {
        ctx.unsupported(b.span, "a block with no final result expression");
        return None;
    };

    let result = lower_expr(ctx, tail)?;
    Some(Block {
        stmts,
        result: Box::new(result),
    })
}

fn lower_expr(ctx: &mut Ctx, e: &AstExpr) -> Option<Expr> {
    match e {
        AstExpr::Int { value, .. } => Some(Expr::Int(*value)),
        AstExpr::Bool { value, .. } => Some(Expr::Bool(*value)),
        AstExpr::Str { span, .. } => {
            ctx.unsupported(*span, "a `Str` value");
            None
        }
        AstExpr::Var { name, span } => match ctx.resolve(name) {
            Some((slot, ty)) => Some(Expr::Load { slot, ty }),
            None => {
                // Checking already rejects a genuinely undefined name, so reaching here
                // means the resolver above disagrees with the checker — worth reporting
                // as itself rather than as "unsupported".
                ctx.errors.push(
                    Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not a local the native backend can see"),
                    )
                    .help("this is a bug in the compiler; `nazm run` still works"),
                );
                None
            }
        },
        AstExpr::Neg {
            operand, op_span, ..
        } => {
            let operand = lower_expr(ctx, operand)?;
            if operand.ty() != Ty::Int {
                // Unreachable after checking, which rejects `-true`. Reported rather than
                // assumed away: the emitter has no `i1` negation to fall back on.
                ctx.unsupported(*op_span, "negation of a non-`Int` value");
                return None;
            }
            Some(Expr::Neg {
                operand: Box::new(operand),
                span: *op_span,
            })
        }
        AstExpr::Binary {
            op,
            lhs,
            rhs,
            op_span,
            ..
        } => lower_binary(ctx, *op, lhs, rhs, *op_span),
        AstExpr::Call { span, .. } => {
            ctx.unsupported(*span, "a function call");
            None
        }
        AstExpr::If {
            cond,
            then_block,
            else_block,
            span,
        } => lower_if(ctx, cond, then_block, else_block.as_ref(), *span),
        // Only an `if` branch may be a block. A bare one is a different construct with a
        // different scoping question, and this increment does not answer it.
        AstExpr::Block(b) => {
            ctx.unsupported(b.span, "a block used as an expression");
            None
        }
    }
}

fn lower_binary(
    ctx: &mut Ctx,
    op: AstOp,
    lhs: &AstExpr,
    rhs: &AstExpr,
    op_span: Span,
) -> Option<Expr> {
    // Left, then right, always — including when the operator itself turns out to be
    // unsupported, so a program with problems on both sides reports both.
    let lhs = lower_expr(ctx, lhs);
    let rhs = lower_expr(ctx, rhs);
    let (lhs, rhs) = (lhs?, rhs?);

    if let Some(op) = arithmetic(op) {
        if lhs.ty() != Ty::Int || rhs.ty() != Ty::Int {
            // Unreachable after checking: arithmetic is `Int` only.
            ctx.unsupported(op_span, "arithmetic on a non-`Int` value");
            return None;
        }
        return Some(Expr::Arith {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: op_span,
        });
    }

    let cmp = comparison(op)?;
    if lhs.ty() != rhs.ty() {
        // Unreachable after checking, which requires both sides of `==` to be one type.
        ctx.unsupported(op_span, "a comparison of two different types");
        return None;
    }
    if cmp.is_ordering() && lhs.ty() != Ty::Int {
        ctx.unsupported(op_span, &format!("ordering `{}` values", lhs.ty().name()));
        return None;
    }
    Some(Expr::Compare {
        op: cmp,
        operand_ty: lhs.ty(),
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    })
}

/// Value-position `if`, and only that.
fn lower_if(
    ctx: &mut Ctx,
    cond: &AstExpr,
    then_block: &AstBlock,
    else_block: Option<&AstBlock>,
    span: Span,
) -> Option<Expr> {
    // The condition is lowered first, and outside both branch scopes: it is evaluated
    // before either branch and cannot see a binding made inside one.
    let cond = lower_expr(ctx, cond);

    let Some(else_block) = else_block else {
        // In value position the checker rejects this already; in statement position it
        // never reaches here. Refused rather than assumed unreachable.
        ctx.unsupported(span, "an `if` with no `else` used as a value");
        return None;
    };

    // Each branch gets its own scope, so a binding made in one is invisible in the other
    // and after the join. Both are lowered even when the condition failed, so all the
    // problems in the program are reported at once.
    let then_lowered = ctx.scoped(|ctx| lower_block(ctx, then_block));
    let else_lowered = ctx.scoped(|ctx| lower_block(ctx, else_block));

    let cond = cond?;
    if cond.ty() != Ty::Bool {
        // Unreachable after checking, which requires a `Bool` condition — there is no
        // truthiness in this language.
        ctx.unsupported(span, "an `if` whose condition is not a `Bool`");
        return None;
    }
    let (then_lowered, else_lowered) = (then_lowered?, else_lowered?);

    let ty = then_lowered.result.ty();
    if ty != else_lowered.result.ty() {
        // Also unreachable after checking, which requires the branches to agree unless
        // one diverges — and every diverging construct is refused above.
        ctx.unsupported(span, "an `if` whose branches produce different types");
        return None;
    }

    Some(Expr::If {
        cond: Box::new(cond),
        then_block: Box::new(then_lowered),
        else_block: Box::new(else_lowered),
        ty,
    })
}

fn arithmetic(op: AstOp) -> Option<ArithOp> {
    match op {
        AstOp::Add => Some(ArithOp::Add),
        AstOp::Sub => Some(ArithOp::Sub),
        AstOp::Mul => Some(ArithOp::Mul),
        AstOp::Div => Some(ArithOp::Div),
        AstOp::Rem => Some(ArithOp::Rem),
        AstOp::Eq | AstOp::NotEq | AstOp::Lt | AstOp::LtEq | AstOp::Gt | AstOp::GtEq => None,
    }
}

fn comparison(op: AstOp) -> Option<CmpOp> {
    match op {
        AstOp::Eq => Some(CmpOp::Eq),
        AstOp::NotEq => Some(CmpOp::NotEq),
        AstOp::Lt => Some(CmpOp::Lt),
        AstOp::LtEq => Some(CmpOp::LtEq),
        AstOp::Gt => Some(CmpOp::Gt),
        AstOp::GtEq => Some(CmpOp::GtEq),
        AstOp::Add | AstOp::Sub | AstOp::Mul | AstOp::Div | AstOp::Rem => None,
    }
}
```

### `docs/direction.md`

```
# Decision record: language development is the primary track

**Date:** 2026-09-20 · **Status:** accepted · **Supersedes:** the framing in
`docs/evaluation.md` §1 under which the three-arm experiment decided whether the language
would exist at all.

---

## What changed

Until this record, the repository was organised around a single question: *does a new
language reduce agent cost enough to justify building one?* The benchmark harness came
first deliberately, and `README.md` said in as many words that the answer might be "build
better TypeScript tooling instead."

The founder has decided otherwise. **Nazm is a language project.** The goal is a fast,
memory-efficient, concurrent language suited to AI-native software, with a compiler
eventually written in Nazm itself.

## What this record does *not* change

Listed explicitly, because the easy failure here is to quietly relax a standard while
changing a goal.

| Preserved | Where |
| --- | --- |
| The evaluation protocol, unaltered | `docs/evaluation.md` |
| The decision rule and its thresholds — `point R_C ≤ 0.80` **and** `upper CB < 1.00`, `Δp`, the void-rate policy | `docs/evaluation.md` §6 |
| The three arms, including **A2 as the control** | `docs/evaluation.md` §2 |
| Every historical result and the harness that produced them | `crates/nazm-bench`, `nazm-bench selftest` |
| The protocol's own status: **draft, not pre-registered** | `docs/evaluation.md` top |

No threshold has been moved. No earlier run has been reinterpreted. `nazm-bench run` is
still disabled, and the harness still cannot produce a benchmark number.

## What the evaluation is now for

Not a gate on the project's existence. **Evidence for specific usability and performance
claims** — the ones that would otherwise be asserted without support:

- that agents spend less effort per accepted change in Nazm than in TypeScript with
  comparable tooling;
- that structured diagnostics and a canonical formatter reduce rework, which is the A2
  arm's actual subject and remains worth measuring on its own;
- eventually, the runtime claims in `docs/internal/performance-log.md`, which need a backend before
  they mean anything.

A2 keeps its role. If improved TypeScript tooling captures most of a measured benefit,
that is a finding about *that claim*, and it stays in the record. It is no longer a stop
condition for the project.

## Self-hosting

**A long-term objective, and explicitly outside the current interpreter milestone.**

Before this record, self-hosting appeared in the repository three times: once in
`docs/user/spec.md` as a *non-goal for v0.1*, once in `README.md` as a caveat about what a
reproducible bootstrap does not establish, and once in `docs/internal/architecture.md` §8 as a
reason to prefer pure-Rust dependencies. It was never planned.

It is now a stated objective with a staged path in `docs/user/roadmap.md` (M3). The gap is
large and worth naming rather than minimising: Nazm today has no collections, no
user-defined types, no pattern matching, no string concatenation, and no I/O. A compiler
cannot be written in it in any form, and no amount of backend work changes that. The
roadmap therefore puts the language facilities a compiler needs (M2) before the bootstrap
(M3), and a native vertical slice (M1) before either.

**The Rust implementation is retained until a replacement is justified.** It is the
bootstrap toolchain, the semantic oracle for differential testing, and the thing that
keeps working while the self-hosted compiler does not.

## Four kinds of statement, kept apart

The repository already separates these well; this record names the distinction so it
survives the change of direction.

| Kind | Meaning | Where it lives |
| --- | --- | --- |
| **Implemented behaviour** | Runs today, with a test that fails if it stops | `README.md` "What is actually here"; `docs/user/spec.md` *Settled for v0.x*; the test suites |
| **Accepted design** | Decided, not built. Changing one is a re-founding decision | `docs/internal/architecture.md`; `docs/user/spec.md` *Settled* |
| **Open question** | Known to need an answer, answer not chosen | `docs/user/spec.md` *Open*; `docs/internal/architecture.md` where it says so |
| **Research proposal** | Speculative, and not entitled to be described as a plan | Automatic layout selection (`docs/internal/architecture.md` §4 — "a **proposed research direction**, pending a literature review that has not been done") |

A document, a stub, or a passing test does not by itself move a statement up this table.

## Backend ordering

**Initial decision, 2026-09-20, supported by the adviser: LLVM IR emitted as text and
assembled by `clang` is the first backend.** Verified end to end before adoption — zero
new Rust dependencies, no MSRV exposure, and it is what `architecture.md` §1 already
prescribes for year one.

**This is an ordering decision, not a rejection of Cranelift.** `architecture.md` §1 still
holds: Cranelift is the intended dev-tier backend because one component does four jobs —
JIT, comptime evaluator, REPL, hot reload. None of those jobs exists yet, and a second
backend before then would be a second set of semantics bugs to keep in agreement for no
present benefit. The decision to revisit is Stage 7's.

## Consequences

1. `docs/user/roadmap.md` is added and is the ordering authority for language work.
2. `docs/internal/architecture.md` §7 and the `Cargo.toml` header, which both said "Phase 2 is
   `nazm-bench` and `xtask` only — there is no compiler yet", are corrected: the Phase 4
   front-end crates exist and a `.nz` program runs.
3. `docs/user/spec.md` keeps its version history rather than being rewritten. The v0.1 section
   still records that v0.1 had no `return`; a pointer marks where that was superseded.
4. The harness is maintained, not extended, until the claims in M1–M3 need it.
```

### `docs/user/roadmap.md`

```
# Roadmap: native compilation, then self-hosting

**Status: M1a is implemented; M1b onward, M2 and M3 are plan.** The direction this serves
is recorded in [`direction.md`](direction.md); the design it must stay consistent with is
[`architecture.md`](architecture.md); the semantics it must preserve are in
[`spec.md`](spec.md).

Ordering principle: **a trustworthy core before a bootstrap.** Self-hosting is the
long-term objective and it is also the easiest possible excuse for a rewrite. It appears
here as M3, after the language can actually express a compiler (M2), which is after
anything at all can be compiled (M1).

---

## Where the line is today

Established by inspection and execution on 2026-09-20, not by document:

| | |
| --- | --- |
| `nazm run` | **Interpreted.** `source → tokens → AST → check → tree-walking eval`. The whole language below |
| `nazm build` | **Native, for arithmetic only (M1a).** `source → tokens → AST → check → LIR → LLVM IR text → clang → executable` |
| Language | Functions, recursion, `let`/`let mut`, assignment, `while` + `break`/`continue`, `return`, `if`/`else`, arithmetic and comparison over `Int`, `Bool`, `Str` |
| Absent | Collections, user-defined types, pattern matching, string concatenation, modules, I/O, generics, concurrency, floats |
| Toolchain | Rust 1.98.1 pinned, host `aarch64-apple-darwin`; Apple clang 21.0.0; `cranelift` is **not** a dependency |

The two commands cover different amounts of the language, deliberately: `nazm build`
compiles a narrow subset and refuses the rest **by name**, while `nazm run` continues to
execute everything. A program `nazm build` refuses is not a wrong program.

---

## M1 — First native vertical slice

> **M1a and M1b are implemented (2026-09-20).** A `.nz` program in the subset below
> compiles to a native executable that runs with no interpreter present and agrees with
> the interpreter on every case tested. M1a was integer arithmetic; M1b added `Bool`, the
> six comparisons, and value-position `if`/`else` with branch-local bindings and nesting.
> **M1 as a whole is not complete**: there are still no user functions, no loops, no
> mutation, and no `Str`. See *M1b subset* below for the accepted/rejected table and
> *M1a/M1b status* at the end of this section.

### Objective

A `.nz` program in a defined subset compiles to a native executable that runs with no
interpreter present, and produces observably the same result as the interpreter.

### Existing components reused

Everything in front of the AST, unchanged:

| Reused | Path |
| --- | --- |
| Lexer, parser, two depth bounds | `crates/nazm-syntax/src/{lexer,parser}.rs` |
| AST and spans | `crates/nazm-syntax/src/ast.rs`, `crates/nazm-span` |
| Checker, `Completion::{Value,Unit,Diverges}` | `crates/nazm-core/src/check.rs` |
| Diagnostics, stable codes, `--json` | `crates/nazm-diag` |
| **The interpreter, as the differential oracle** | `crates/nazm-core/src/eval.rs` |
| **The 98-case behavioural suite, as the conformance corpus** | `crates/nazm-core/tests/language.rs` |

The interpreter is not replaced and not deprecated. It becomes the reference that the
backend is tested against, which is the role `architecture.md` §1 already assigns to a
reference interpreter.

### The representation

**One** new IR. Not eight.

`nazm-lir` — a typed, flattened, control-flow-explicit form:

- functions with typed parameters and a return type;
- **slot-indexed locals**, so name resolution is finished before lowering and the backend
  never sees a string;
- basic blocks with explicit terminators (`Br`, `CondBr`, `Ret`);
- instructions over `Int` / `Bool` / `Str` only.

What is deliberately *not* built: HIR, Core, Core′, MIR, Mono MIR. There are no generics
to monomorphise, no traits to turn into dictionaries, no regions to infer, and no layouts
to select — scaffolding those layers now would produce five empty pass-throughs and a
diagram that matches. `architecture.md` §2 stays the target shape; M1 does not pretend to
have reached it.

Lowering `AST → LIR` lives in `nazm-lir`, because it needs the checker's output and
nothing below it. Structured control flow becomes blocks here: `while` becomes
header/body/exit, `if` becomes then/else/join, and `break`/`continue`/`return` become
branches to blocks already on a stack — which is exactly the information `Completion`
already computes.

### Backend and target

**One backend: LLVM IR emitted as text, assembled by `clang`. One target:
`aarch64-apple-darwin`.**

Justified against the existing decisions rather than chosen by taste:

- `architecture.md` §1 says *"Do not link against LLVM in year one. Emit LLVM IR as text
  and shell out to `clang`/`opt`."* This is that, and nothing more.
- It adds **zero Rust dependencies**. `Cargo.toml` already states the rule — *"adding a
  dependency before the crate that needs it exists is how a workspace acquires a build it
  cannot explain"* — and `xtask check`'s codegen-reach gate exists to keep backends behind
  the LIR contract. Text emission needs no entry on that allowlist.
- It avoids an MSRV question. `cranelift` 0.135 declares 1.95.0 against a 1.98.1 pin, so
  it would fit — but `architecture.md` §8 is explicit that declared MSRVs are *"not that
  the set resolves and builds together"*, and M1 does not need to find out.
- `architecture.md` §1 also says *"two backends is two sets of semantics bugs"* and
  *"release is **always** LLVM"*. Starting with the backend that release must use anyway
  means the semantics being nailed down are the ones that ship.

Cranelift arrives when the jobs only it does — JIT, comptime evaluation, hot reload —
actually exist. That is a Stage 7 question, not an M1 one.

**Verified locally, 2026-09-20**, before committing to the path: `clang` 21 compiles a
hand-written `.ll` using `@llvm.sadd.with.overflow.i64`, branches to a trap block, writes
a diagnostic to fd 2, and exits 2. No `llc` or `opt` binary is needed; `clang` consumes IR
text directly. **A hand-written probe establishes only that the tool works**; what M1a is
tested on is `.nz` files compiled through `nazm build` and executed.

The adviser's ordering decision, recorded: **LLVM IR text → `clang` is the initial
backend, not a permanent rejection of Cranelift.** Cranelift arrives when the jobs only it
does — JIT, comptime evaluation, hot reload — exist to justify a second set of semantics
to keep in agreement.

### Semantics that must survive lowering

These are commitments in `spec.md`, and each has a way of being got wrong quietly:

| Commitment | The wrong way to lower it |
| --- | --- |
| Overflow **traps** in every build | `add nsw` — which makes overflow *undefined*, the exact opposite. Must be `llvm.sadd/ssub/smul.with.overflow.i64` plus a branch |
| Division by zero is an error | Emitting bare `sdiv`, which faults |
| `Int::MIN / -1` traps | Also a hardware fault on `sdiv`; needs its own check |
| **`Int::MIN % -1` is `0`, not a trap** | Inheriting `srem`'s fault, which would give `%` a second failure case the language says it does not have |
| `-Int::MIN` traps | `sub 0, x` without a check |
| Argument order is left to right | Letting the emitter reorder operand evaluation |
| `return`/`break`/`continue` leave from value position | Requiring a value from a block that diverges |
| Overflow checks in release too | `[profile.release]` already sets `overflow-checks = true` for the *host*; the emitted program needs the same property by construction |

`Str` in M1 is deliberately tiny: there is no concatenation and no I/O in the language, so
no operation produces a new string. A `Str` is a pointer and a length to a constant in the
binary; `==`/`!=` is a length check and `memcmp`; `main` returning `Str` writes it to fd 1.
No allocator, no ownership question, no cleanup. That is why `Str` is affordable now and
growable collections are not.

### CLI

`nazm run` **does not change.** It stays the tree-walking interpreter, with the same exit
codes (`0` ran, `1` did not type-check, `2` failed at runtime, `3` no such file) and the
same `--max-iterations` / `--max-depth` budgets.

Added:

```sh
nazm build <file> [-o <path>]   # → native executable; same exit codes for check failure
```

A native program has no interpreter budgets, so `--max-depth` and `--max-iterations` are
rejected on `build` by name rather than accepted and ignored. Whether `nazm run` ever
gains a `--backend` flag is deferred; two ways to run one program is a decision to make
once there is something to compare.

### Prerequisites

- None in the language. M1 compiles the subset that already runs.
- `clang` on `PATH`. `nazm build` must detect its absence and say so, with the same
  discipline as `N0404` (*"could not start the evaluator"*) — an environment failure
  reported as one, not blamed on the program.
- A placement for the new crates in `xtask/src/rules.rs`, or the layering gate fails —
  which is the gate working.

### Explicit exclusions

Scaffolding the other IR layers · a second backend · any target but `aarch64-apple-darwin`
· optimisation passes (`-O0`, and the emitted IR is not expected to be fast) · separate
compilation · a linker crate · `nazm-rt` · incremental compilation · `salsa` · `cstree` ·
replacing the interpreter · any new language feature.

### Acceptance criteria

1. `nazm build examples/max.nz -o /tmp/max && /tmp/max` exits 0 and prints `25` — the same
   as `nazm run examples/max.nz`, with no `nazm` process involved.
2. All four `examples/*.nz` do the same.
3. **Differential conformance:** for every case in `crates/nazm-core/tests/language.rs`
   inside the native subset, the native binary and the interpreter agree on the observable
   result — value printed, exit code, and for a runtime failure the diagnostic code.
   Disagreement fails the suite.
4. The trapping cases are checked natively and individually, not as a group: overflow on
   `+`, `-`, `*`, unary negation of `Int::MIN`, `/` by zero, `%` by zero, `Int::MIN / -1`
   traps, and **`Int::MIN % -1` is `0`**.
5. Anything outside the subset is refused by name at `build` time, in the style the parser
   already uses for `loop` and `struct` — never silently miscompiled.
6. `cargo lint`, `cargo xtask check`, `cargo test --workspace` stay green.

### Evidence required for completion

- The differential runner's output, naming how many cases ran natively and how many were
  excluded from the native subset **and why** — an exclusion list is part of the result,
  not a footnote.
- The emitted `.ll` for one arithmetic function, in the commit message or a doc, so a
  reviewer can see that the overflow path is an intrinsic and a branch rather than `nsw`.
- Catalogued mutations for the traps: at minimum, one that replaces a checked add with a
  wrapping one and one that drops the `Int::MIN / -1` check. Both must be caught.
- A statement of what M1 does **not** establish: no performance claim, one target, one
  backend, no optimisation.

### M1b status — implemented

**Added:** `true`/`false`; immutable `Bool` locals; `< <= > >=` on `Int, Int`; `== !=` on
`Int, Int` and on `Bool, Bool`; value-position `if`/`else` with branch-local bindings,
shadowing inside a branch, and arbitrary nesting in conditions and branches.

**IR:** `Ty::{Int, Bool}` carried on `Load`, `If` and `Compare`, so emission never infers
a type from context. `BinOp` was split into `ArithOp` (five, all fallible) and `CmpOp`
(six, none fallible) — one enum would let a `match` arm written for arithmetic silently
start accepting comparisons. `Block { stmts, result }` is shared by `main`'s body and by
each branch, because `docs/user/spec.md` has one `Block`.

**The phi-predecessor rule:** a `phi`'s incoming labels name the block each branch
*ended* in, read after lowering that branch — not the label it started at. A branch
containing a nested `if`, or any checked arithmetic (whose guard opens a continuation
block), ends somewhere other than where it began, and an entry-label `phi` would name a
block that does not jump to the join. `clang` rejects that, so the error is loud, but the
fix is structural rather than a patch.

**Evaluation semantics:** the condition is emitted once, before either branch; only the
selected branch's block is entered, so a failure in the branch not taken cannot run; and
left-to-right order is preserved in enclosing arithmetic and comparisons. A guard's
failure block ends in `unreachable`, so a terminated path is never a `phi` predecessor.

**Correction to an earlier claim.** It was suggested that comparisons would give the
arithmetic something the optimiser could not fold. **That is wrong.** Every program in
the M1b subset has constant inputs — there are no parameters and no I/O — so `-O2` may
evaluate the whole thing at compile time, comparisons and branches included. What the
`-O0`/`-O2` pairs establish is that **the specified outcome does not depend on the
optimisation level**. They do *not* establish that any branch survived optimisation, and
nothing here should be read as claiming it. A value unknown at compile time arrives with
function parameters.

### M1a status — implemented

**Subset compiled:** a single `fn main() -> Int`; integer literals including
`-9223372036854775808`; immutable `let` bindings of `Int` and references to them; unary
`-`; `+ - * / %`; parentheses; a final result expression.

**Excluded, and refused by name with `N0101` after ordinary checking:** user functions and
calls, `if`, `while`, `break`, `continue`, `return`, `let mut` and assignment, `Bool`,
`Str`, comparison operators, nested blocks, expression statements, and any `main` with
parameters or a non-`Int` return type. Nothing is partially compiled.

**Crates:** `crates/nazm-lir` (`ir.rs`, `lower.rs`, `emit.rs`) and
`crates/nazm-cli/src/build.rs`. One IR level, as planned — no HIR, Core, MIR or Mono MIR.

**Target:** `aarch64-apple-darwin`, Apple clang 21.0.0, host `rustc` 1.98.1. Other targets
are **unverified rather than unsupported**: nothing in the emitted IR is target-specific,
but nothing has been run anywhere else, and `nazm-lir::TARGET_NOTE` says so in the
generated file's header.

**Not done in M1a:** catalogued mutations for the new checked-arithmetic paths. The
catalogue was not re-run this increment (see *Verification*), and adding entries that have
not been verified to be caught would weaken it rather than strengthen it. This is the
first item of the next task.

### M1b subset — the accepted/rejected table

Written before implementation, because the way to broaden a backend by accident is to
handle an AST node generically and discover afterwards what else that node covers.
Anything `nazm build` refuses **still runs under `nazm run`**: a backend limitation is not
a statement that the program is invalid.

| Construct | `nazm build` | Note |
| --- | --- | --- |
| `fn main() -> Int`, the only function | accept | M1a |
| `Int` literals, incl. `-9223372036854775808` | accept | M1a |
| **`true` / `false`** | **accept** | M1b |
| immutable `let` of `Int` | accept | M1a |
| **immutable `let` of `Bool`** | **accept** | M1b |
| reference to a local | accept | M1a |
| unary `-` on `Int` | accept | M1a |
| `+ - * / %` on `Int, Int` | accept | M1a |
| **`< <= > >=` on `Int, Int`** | **accept** | M1b; the checker admits `Int` only |
| **`== !=` on `Int, Int`** | **accept** | M1b |
| **`== !=` on `Bool, Bool`** | **accept** | M1b |
| `== !=` on `Str, Str` | **refuse** | Well-typed — the checker allows equality on any single type — but `Str` is not a native value type yet. Refused by name, not miscompiled |
| parentheses | accept | build no node |
| **`if c { a } else { b }` in value position** | **accept** | M1b; both branches, same type, `Int` or `Bool` |
| **`let` inside a branch, and shadowing there** | **accept** | M1b; branch-local scope |
| **`if` nested in a condition or in a branch** | **accept** | M1b |
| `if` with no `else` | refuse | the checker rejects it in value position anyway |
| `if` in **statement** position | refuse | it arrives as an expression statement, which stays refused — value-position `if` is the *only* `if` this backend handles |
| bare `{ … }` as an expression | refuse | only an `if` branch may be a block |
| `let mut`, assignment | refuse | |
| `while`, `break`, `continue`, `return` | refuse | |
| user functions and calls | refuse | |
| `Str` values | refuse | |
| `main` with parameters or a non-`Int` return | refuse | |
| expression statements | refuse | |

There are no logical operators in the language (`&&`, `||`, `!` do not exist), so there is
nothing for `if` to broaden into by way of one.

### Risks and effort

| Risk | Note |
| --- | --- |
| The trap paths are where the bugs will be | Mitigated by criterion 4 being per-case, and by the interpreter already being right about them |
| `Str` scope creeps | It is affordable only because nothing produces a new one; the moment concatenation appears it needs an allocator and a cleanup rule, which is M2's job |
| Apple clang's IR dialect drifting from upstream LLVM | Pin nothing yet, but record the clang version with any generated artifact, as `performance.md` §5 already requires |

Rough effort: **2–4 weeks**, assuming the subset stays exactly as it is and the
differential runner reuses the existing test corpus rather than growing a new one.

---

## M2 — The facilities needed to write a compiler

### Objective

Grow the language, in dependency order, until a compiler for a Nazm subset can be
*written* in Nazm. Not until it is bootstrapped — that is M3.

### Derived from what a compiler actually does

Each row states its semantic prerequisite, what the interpreter needs, what the backend
needs, and a small program that demonstrates it. The ordering is forced by the
prerequisites, not chosen.

| # | Compiler operation | Semantic prerequisite | Interpreter | Native lowering | Acceptance program |
| --- | --- | --- | --- | --- | --- |
| 1 | **Value/copy/move behaviour and cleanup** | The MVS parameter conventions (`let`/`inout`/`sink`/`set`) decided intraprocedurally, and a drop rule | Value clone at binding; drop at scope end | Stack slots, drop calls at block exits | A value passed `inout`, mutated, observed by the caller; a `sink` value not usable afterwards |
| 2 | **Bytes and strings** | Owned `Str` with concatenation, slicing, length, indexing by byte, `Int`↔`Str`, UTF-8 decode | Heap-backed `String` | An allocator interface; drop for `Str` | A program that reads a byte, builds a string, and compares it |
| 3 | **Tagged alternatives and products** | `struct` and `enum` with exhaustive `match` | Tagged runtime values | Tagged unions with explicit discriminants; **field access stays symbolic** (`architecture.md` §2) | A `Token` enum with a payload, matched exhaustively |
| 4 | **Growable collections** | Generics, or one monomorphic escape hatch | `Vec<Value>` | Monomorphised per element type | `List[Token]` built by a loop and iterated |
| 5 | **Recursive structures** | **Arena indices**, not heap-recursive values | An index into a `List` | Nothing new — an index is an `Int` | An AST as `List[Node]` where a node holds `u32` child ids |
| 6 | **Symbols and diagnostics** | A map, or a sorted list with binary search; string interning | — | — | Interning 10k identifiers and resolving them |
| 7 | **Reading source files** | The I/O authority contract below | A real read | `read(2)` behind the runtime | `wc -c` in Nazm |
| 8 | **Multiple source files** | File-as-module with explicit imports and a visibility rule | Multi-file loading | Separate compilation, or whole-program for now | A two-file program |
| 9 | **Writing output and errors** | Same authority contract | Real writes | `write(2)` | A program that writes a file and reports a failure to stderr |

**Rows 1 and 2 are in that order deliberately.** Growing a mutable collection before the
copy/move and cleanup rules are settled is how a language acquires a memory model by
accident. `spec.md` already lists exact copy semantics, closure capture, and copy elision
as *Open*; row 1 is where they stop being open.

**Row 5 is the load-bearing simplification.** An AST is the recursive structure a compiler
is made of, and heap-recursive values under mutable value semantics is the hardest problem
on this list. Arena indices sidestep it entirely: a node holds `u32` children, the arena is
a `List`, and recursion is an index lookup. It is also what the existing design already
leans toward — `architecture.md` §2 has `DefId` as *"path-based, never positional"* and
every IR level indexed rather than pointer-chased. Heap-recursive values can come later, or
never.

### The I/O authority contract

Narrow, and deliberately not the general effect system.

**I/O requires an unforgeable capability value, passed explicitly.**

```nazm
fn main(io: IO) -> Int {
    let text = read_file(io, "input.nz");
    ...
}
```

- `IO` has no constructor in the language. Only the runtime makes one, and it makes
  exactly one, at entry to `main`.
- A function that performs I/O takes it as a parameter. A function that does not, cannot —
  which is checkable today, with the type checker that already exists, and needs no effect
  rows, no inference, and no handlers.
- It composes forward rather than being thrown away: when effect rows arrive, holding an
  `IO` is what discharges the `io` effect, and the row can be *inferred* from the
  parameter. `architecture.md` §5 already treats effects, capabilities and provenance as
  three separate problems; this is the capability one, alone.
- It does not bypass the planned model. There is no ambient `print`, no global handle, and
  no `unsafe` escape. The thing `spec.md` refuses — *"a builtin that bypassed the effect
  system before it exists would be the wrong shape to unpick later"* — is exactly what an
  ambient `print` would be, and a capability parameter is not that shape.

Finer-grained authority (`FileRead` vs `FileWrite` vs `Net`) is a later split of one type,
not a redesign.

### Existing components reused

The checker's scope handling and `Completion` analysis extend to the new forms rather than
being replaced. The diagnostic codes and `--json` schema are unchanged. The parser gains
constructs; its two depth bounds and its refuse-by-name list already have the right shape
for them.

### Explicit exclusions

General effect handlers · one-shot continuations · whole-program taint · layout selection ·
closures (until row 1 settles capture) · traits beyond what row 4 needs · a package manager ·
floats · concurrency of any kind.

### Acceptance criteria

Per row: the acceptance program runs **identically under the interpreter and natively**,
and `spec.md` gains a *Settled* entry stating the rule and why the alternative was refused.
A row is not done when its feature parses; it is done when its semantics are written down
and differentially tested.

Milestone-level: a **lexer for Nazm, written in Nazm**, that reads a `.nz` file and emits
its token stream — running natively. That is the smallest artefact that exercises rows 1–9
together and is genuinely part of the eventual compiler rather than a demo.

### Evidence required

The Nazm lexer's token stream, on every file in `examples/`, byte-identical to the Rust
lexer's on the same input. A disagreement is a specification bug, and finding one is the
point.

### Effort

**Months, not weeks**, and the estimate is weak because rows 1, 3 and 4 each contain a
real design decision. Row 1 alone is the mutable-value-semantics question the project has
deferred since the beginning. Treat the row boundaries as the review points.

---

## M3 — Self-hosted compiler and bootstrap

### Three states, kept apart

The founder's distinction, restated as the thing that must not be blurred in a report:

| Stage | What is true | What is **not** |
| --- | --- | --- |
| **M3.0** | A compiler for a Nazm subset, *written in Nazm*, runs **under the Rust interpreter** and compiles a test program | Not self-hosting. Not the bootstrap. A useful intermediate, and reportable only as one |
| **M3.1** | That compiler, compiled natively by the Rust implementation, runs as a binary | Still not self-hosting — the Rust implementation built it |
| **M3.2** | That binary compiles its own source | Self-hosted. Reproducibility not yet established |
| **M3.3** | The result compiles its own source again, and the two agree | The bootstrap, with its fixed point checked |

### The chain

```
compiler.nz ──[ Rust implementation ]──► C1     stage 1
compiler.nz ──[ C1 ]─────────────────► C2       stage 2
compiler.nz ──[ C2 ]─────────────────► C3       stage 3
```

- **C1** is produced by the retained Rust toolchain from the same Nazm source.
- **C2** is produced by C1 from that *same* source.
- **C3** is produced by C2 from that *same* source.

C1 differs from C2 legitimately: different compilers built them. **C2 and C3 were built by
compilers that should be behaviourally identical**, so that is where a fixed point is
expected, and it is the only place equality may be asserted.

### Conformance and reproducibility checks

**Conformance — required, and the strong check:**

- C2 and C3 both pass the whole conformance corpus: the 98 cases in
  `crates/nazm-core/tests/language.rs` plus everything M1 and M2 added.
- Differential against the retained Rust implementation on the same corpus. Where they
  disagree, the specification decides, and if the specification is silent that is the
  finding.

**Reproducibility — stated carefully:**

- **Byte equality between C2 and C3 is asserted only once determinism has been
  established**, which means: no timestamps, no absolute paths, no build-host identifiers,
  and no hash-map iteration order in emitted output. None of that exists today, and
  nothing here should be read as claiming it.
- Until then the comparison is: **identical emitted LLVM IR text** for the corpus. That is
  a stronger check than identical binaries anyway, because it is diffable, and it isolates
  the compiler's determinism from `clang`'s and the linker's.
- Any remaining difference between C2 and C3 is **enumerated and explained**, or the
  bootstrap is not complete. "They behave the same" is not a substitute for knowing why
  the bytes differ.

### What stays outside Nazm, legitimately

Self-hosting does not mean eliminating these, and a report that implies otherwise is
wrong:

| Dependency | Why it stays |
| --- | --- |
| LLVM, via `clang` | Codegen is the reused layer by design (`architecture.md` §1) |
| The system linker | Same |
| libc | `write`, `read`, `exit`, `malloc` until `nazm-rt` exists |
| The OS and its loader | — |
| **The Rust implementation** | **Retained until a replacement is justified** — it is the bootstrap seed, the differential oracle, and the thing that still works when C1 does not |

### Explicit exclusions

Deleting the Rust implementation · a second backend · reproducible-build infrastructure
beyond determinism of the compiler's own output · distributing a binary seed ·
cross-compilation.

### Acceptance criteria

C3 exists; C2 and C3 emit identical IR for the whole corpus; both pass conformance; every
external dependency is listed; every difference that remains is named.

### Evidence required

The three build logs, the corpus results for C2 and C3, the IR diff (empty, or explained),
and an explicit statement of which of M3.0–M3.3 was reached. **A compiler running under
the Rust interpreter is M3.0 and must be reported as M3.0.**

---

## Concurrency track (separate, and not now)

Runs alongside, intersecting the compiler roadmap in exactly two places. **No scheduler is
to be started now**, and a compiler does not need one — a single-threaded compiler is a
perfectly good compiler.

| Stage | Content | Depends on |
| --- | --- | --- |
| C-1 | **Specify** the open questions `spec.md` already lists: what cancellation cancels, what cleanup is guaranteed, captured variables and globals under channel transfer, preemption at back-edges vs signals | Nothing. It is writing |
| C-2 | **Ownership across tasks.** The borrow region `[spawn point, scope end]` with parent and sibling access rejected throughout — already a *Settled* decision (`spec.md` §6) | **M2 row 1.** Ownership across tasks is the same exclusivity question as ownership across calls; solving it twice is how the two answers diverge |
| C-3 | Structured lifetimes and cancellation, as a checked property with compile-fail tests | C-2 |
| C-4 | `nazm-rt`: scheduler, stacks, channels, reactor. **The one crate allowed `unsafe`**, which means the workspace's `unsafe_code = "forbid"` becomes `deny` with a narrow allowance, and `xtask check`'s unsafe gate enforces the narrowness | C-3, and a backend that can call it |
| C-5 | Measurement: spawn-to-first-instruction latency, context-switch cost, task memory as **RSS not virtual reservation** with page size recorded, against Go | C-4 |

**Where it intersects:** at M2 row 1 (shared memory model) and at M1's backend (C-4 needs
something to compile against). Nowhere else. Concurrency features in the surface language
before C-1 is written would be syntax without semantics.

---

## AI-native track (start here, and keep it narrow)

`direction.md` separates three things that the phrase blurs. Only the first is in scope
now.

**(a) Agent-facing tooling and predictable semantics — in scope, and largely already
built.**

| Capability | State |
| --- | --- |
| Stable structured diagnostics: error codes, spans, `--json`, machine-applicable fixits | **Implemented** (`nazm-diag`, `--json`) |
| One mistake, one error — a broken body does not delete the function | **Implemented** (`ast.rs` `Function::body: Option<Block>`) |
| Predictable semantics: specified evaluation order, trapping overflow, no truthiness, refuse-by-name | **Implemented** (`spec.md` *Settled*) |
| **Safe source edits**: a lossless CST and a canonical formatter with no degrees of freedom to churn over | **Absent.** Needs `cstree`; the current parser produces a plain AST |
| **Reproducible checks**: the same input gives the same diagnostics, orderable and diffable | Partly — needs the determinism work M3 requires anyway |
| A cheatsheet under ~3k tokens, and a machine-readable grammar | **Absent** (`spec.md` *Open*) |

**(b) Libraries for writing AI applications in Nazm — out of scope.** Not until there is a
standard library and an I/O story, which is M2 at the earliest.

**(c) Tensors, GPUs, model providers, distributed execution — out of scope, and not
planned.** `architecture.md` §1 already says MLIR should be revisited *"only if Nazm pivots
toward tensor/accelerator codegen"*, and nothing here proposes that pivot.

**No language keywords will be added to support the label.** If a construct earns its place
on semantics, it earns it; "AI-native" is not an argument for syntax.

---

## Ordering summary

```
M1  native vertical slice        ← next; no language change
M2  facilities for a compiler    ← the long one; row 1 is the real decision
M3  bootstrap                    ← C1 → C2 → C3, byte equality only where earned
```

with the concurrency track joining at M2 row 1, and the AI-native track's one open item
(lossless CST + canonical formatter) schedulable any time after M1.
```

### `xtask/tests/lifecycle.rs`

```rust
//! The mutation runner's lifecycle, exercised against real processes and a real tree.
//!
//! Everything here runs the shipped `xtask` binary against a **disposable Cargo project**
//! in a temporary directory. Nothing touches this repository: a test that interrupts a
//! mutation run has to leave a damaged tree behind for a moment, and doing that to the
//! checkout is how an afternoon was lost once already.
//!
//! # Why these cannot be unit tests
//!
//! Three of the properties are about processes, not functions.
//!
//! * **Mutual exclusion** needs two independent processes to contend. One process cannot
//!   demonstrate that a *killed* holder's lock is released, because the kernel releases it
//!   on exit and there is no exit to observe from inside.
//! * **Recovery after an interruption** needs a process that did not create the state it
//!   repairs. A hand-built record proves that recovery reads records correctly; it says
//!   nothing about whether a real interrupted run leaves a state recovery can read.
//! * **Verdicts** are produced by building and running a suite, which is `cargo`.
//!
//! # Why they are `#[ignore]`d
//!
//! Each one drives several nested `cargo` invocations. `cargo xtask mutate` runs
//! `cargo test --workspace` once per catalogued defect — 44 of them — so leaving these in
//! the default set would add their cost to every iteration. Run them explicitly:
//!
//! ```sh
//! cargo test -p xtask --test lifecycle -- --ignored --test-threads=1
//! ```
//!
//! `--test-threads=1` because several of them spawn `cargo`, and running those in
//! parallel competes for the same build resources for no benefit.

// A test that cannot assert is not a test. The workspace denies these because the
// harness computes numbers a run is judged on; a fixture that panics on a broken
// precondition is exactly what is wanted here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const XTASK: &str = env!("CARGO_BIN_EXE_xtask");

/// The disposable project's one source file.
///
/// Three separate mutation targets, each unique in the file so the runner's
/// "must occur exactly once" check passes:
///
/// * `{ 42 }` — asserted by the test, so changing it is **caught**; replacing it with a
///   string makes the crate **not build**.
/// * `{ 7 }` — never asserted, so changing it **survives**.
/// * `{ 0 }` — the delay the test sleeps for, so changing it makes the mutant *slow*,
///   which is what makes an interruption reliable rather than a race.
const SOURCE: &str = r#"pub fn answer() -> u64 { 42 }

pub fn unchecked() -> u64 { 7 }

pub fn delay_ms() -> u64 { 0 }

#[cfg(test)]
mod tests {
    #[test]
    fn the_answer_is_right() {
        // Announces this process, so a test can prove the descendant is still alive
        // after its coordinator has been killed. Written before the sleep, and holding
        // the pid rather than a word, because "it started" is not "it is running".
        //
        // The delay goes with it, and that is not decoration: the *baseline* run writes
        // this file too, with delay 0, and then exits at once. A test that waited only
        // for the file would read a pid that was already dead and prove nothing.
        if let Ok(p) = std::env::var("NAZM_FIXTURE_READY") {
            let _ = std::fs::write(p, format!("{} {}", std::process::id(), super::delay_ms()));
        }
        std::thread::sleep(std::time::Duration::from_millis(super::delay_ms()));
        assert_eq!(super::answer(), 42);
    }
}
"#;

const SRC_REL: &str = "src/lib.rs";

/// What the slow mutation sets `delay_ms()` to. Long enough that the window in which the
/// tree is damaged is interrupted deliberately rather than by luck.
const MUTANT_DELAY_MS: u64 = 30_000;

/// Wait until the run is mid-mutation, and return the live descendant's pid.
///
/// "Mid-mutation" is three facts, not one: the record exists, the source holds the
/// expected mutated bytes, and the *mutated* build's test process has announced itself.
/// The third matters because the baseline writes the same announcement file and exits at
/// once — waiting only for the file yields a pid that is already dead.
fn wait_until_mutated(
    src: &Path,
    record: &Path,
    ready: &Path,
    expected_mutated: &str,
    delay_ms: u64,
    deadline: Duration,
) -> u32 {
    let mut descendant = 0u32;
    until(
        "the run to be mid-mutation with a live descendant",
        deadline,
        || {
            let has_record = record.exists();
            let mutated = fs::read_to_string(src).is_ok_and(|s| s == expected_mutated);
            let pid = fs::read_to_string(ready).ok().and_then(|s| {
                let mut f = s.split_whitespace();
                let pid = f.next()?.parse::<u32>().ok()?;
                let d = f.next()?.parse::<u64>().ok()?;
                (d == delay_ms).then_some(pid)
            });
            match (has_record, mutated, pid) {
                (true, true, Some(p)) => {
                    descendant = p;
                    Ok(())
                }
                (r, m, p) => Err(format!(
                    "record={r} mutated={m} live-descendant={}",
                    p.map_or("not yet announced by the mutated build".to_owned(), |v| v
                        .to_string())
                )),
            }
        },
    );
    descendant
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    let mut perm = fs::metadata(path).expect("metadata").permissions();
    perm.set_mode(mode);
    fs::set_permissions(path, perm).expect("set_permissions");
}

/// A standalone Cargo project with no dependencies.
///
/// `[workspace]` is present so the project is its own workspace wherever the temporary
/// directory lands, and no dependencies means `cargo` never reaches the network or the
/// shared registry lock.
fn disposable_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\n\n\
         [package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\n",
    )
    .expect("manifest");
    fs::write(root.join(SRC_REL), SOURCE).expect("source");
    dir
}

fn catalogue(root: &Path, name: &str, find: &str, replace: &str) -> PathBuf {
    let path = root.join(format!("{name}.toml"));
    fs::write(
        &path,
        format!(
            "[[mutation]]\n\
             name = \"{name}\"\n\
             why = \"lifecycle fixture\"\n\
             file = \"{SRC_REL}\"\n\
             find = {}\n\
             replace = {}\n",
            toml_string(find),
            toml_string(replace)
        ),
    )
    .expect("catalogue");
    path
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

struct Run {
    status: Option<i32>,
    stdout: String,
}

impl Run {
    fn says(&self, needle: &str) -> bool {
        self.stdout.contains(needle)
    }
}

fn xtask(args: &[&str]) -> Run {
    let out = Command::new(XTASK)
        .args(args)
        .stderr(Stdio::piped())
        .output()
        .expect("run xtask");
    Run {
        status: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr),
    }
}

/// Poll until `cond` holds, or fail with what was still missing at the deadline.
///
/// Every wait in this file goes through here. A bare `sleep` would make the tests pass or
/// fail on machine speed, and a pass bought that way is worth nothing.
fn until(what: &str, deadline: Duration, mut cond: impl FnMut() -> Result<(), String>) {
    let end = Instant::now() + deadline;
    let mut last = String::from("never evaluated");
    while Instant::now() < end {
        match cond() {
            Ok(()) => return,
            Err(why) => last = why,
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out after {deadline:?} waiting for {what}: {last}");
}

/// The tree is exactly as it started and the runner left nothing behind.
fn assert_clean(root: &Path) {
    assert_eq!(
        fs::read_to_string(root.join(SRC_REL)).expect("read source"),
        SOURCE,
        "the source was not restored to its original bytes"
    );
    assert!(
        !root.join("target/.nazm-mutation-backup").exists(),
        "a write-ahead record outlived the run that wrote it"
    );
    // Not "the lock file is gone" — it is deliberately kept, as the coordination object
    // the next run locks. What must be true is that it is *released*, and the only
    // honest way to say that is that a later process can take it.
    let r = xtask(&[
        "mutate-recover",
        "--root",
        &root.display().to_string(),
        "--confirm-no-writers",
    ]);
    assert_eq!(r.status, Some(0), "the lock was not released: {}", r.stdout);
    assert!(
        r.says("nothing to recover"),
        "expected a clean tree, got: {}",
        r.stdout
    );
}

// ---------------------------------------------------------------------------
// A. Independent verdicts, each with its cleanup checked immediately.
// ---------------------------------------------------------------------------

/// One entry per run, so a verdict cannot be produced by a neighbouring mutation.
///
/// Cleanup is asserted after *each* run rather than once at the end: a runner that
/// restored only the last file it touched would pass a single check at the end and still
/// be broken.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn one_entry_runs_report_caught_survived_and_did_not_build() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();

    // Caught: the mutation changes a value the suite asserts on.
    let c = catalogue(root, "caught", "u64 { 42 }", "u64 { 43 }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &c.display().to_string(),
    ]);
    assert_eq!(
        run.status,
        Some(0),
        "a caught mutation is a pass: {}",
        run.stdout
    );
    assert!(run.says("caught       caught"), "{}", run.stdout);
    assert_clean(root);

    // Survived: the mutation changes a value nothing asserts on.
    let s = catalogue(root, "survived", "u64 { 7 }", "u64 { 8 }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &s.display().to_string(),
    ]);
    assert_eq!(run.status, Some(1), "a survivor must fail the run");
    assert!(run.says("SURVIVED     survived"), "{}", run.stdout);
    assert!(
        run.says("injected and compiled, but the suite still passed"),
        "{}",
        run.stdout
    );
    assert_clean(root);

    // Did not build: the mutated tree does not compile, so a failing suite would not
    // mean the defect was detected.
    let b = catalogue(root, "unusable", "u64 { 42 }", "u64 { \\\"forty-two\\\" }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &b.display().to_string(),
    ]);
    assert_eq!(
        run.status,
        Some(1),
        "an unusable mutation must fail the run"
    );
    assert!(run.says("UNUSABLE     unusable"), "{}", run.stdout);
    assert!(run.says("does not compile"), "{}", run.stdout);
    assert_clean(root);
}

// ---------------------------------------------------------------------------
// B. Two independent processes, and only one winner.
// ---------------------------------------------------------------------------

/// Mutual exclusion across processes, and release afterwards.
///
/// Synchronised on files the holder writes *after* it has the lock, never on elapsed
/// time: the competitor must start only once the lock is genuinely held, or the test
/// proves nothing about contention.
#[test]
#[ignore = "spawns competing processes; run with --ignored"]
fn two_independent_processes_never_both_hold_the_tree() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let ready = root.join("held");
    let release = root.join("go");

    let mut holder = Command::new(XTASK)
        .args([
            "mutate-hold",
            "--root",
            &root_s,
            "--ready",
            &ready.display().to_string(),
            "--release",
            &release.display().to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn holder");

    until(
        "the first process to hold the lock",
        Duration::from_secs(30),
        || {
            if ready.exists() {
                Ok(())
            } else {
                Err("its ready file has not appeared".to_owned())
            }
        },
    );

    // A second, entirely independent process asks for the same tree.
    let rival_ready = root.join("held-2");
    let rival = Command::new(XTASK)
        .args([
            "mutate-hold",
            "--root",
            &root_s,
            "--ready",
            &rival_ready.display().to_string(),
            "--release",
            &release.display().to_string(),
        ])
        .output()
        .expect("run rival");

    assert!(
        !rival.status.success(),
        "two processes both took the tree: the rival exited {:?}",
        rival.status.code()
    );
    assert!(
        !rival_ready.exists(),
        "the rival announced ownership it was refused"
    );

    // And the refusal must say it was contention, not something it could retry through.
    let probe = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(probe.status, Some(1));
    assert!(
        probe.says("another mutation run holds"),
        "contention must be named as such: {}",
        probe.stdout
    );

    fs::write(&release, "go").expect("signal release");
    let held = holder.wait().expect("reap holder");
    assert!(held.success(), "the holder exited {:?}", held.code());

    // Released — which is asserted by a later process succeeding, not by the file's
    // absence. The lock file is deliberately still there.
    assert!(
        root.join("target/.nazm-mutation-lock.v2").exists(),
        "the coordination object must survive release"
    );
    let after = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        after.status,
        Some(0),
        "the lock was not released: {}",
        after.stdout
    );
}

// ---------------------------------------------------------------------------
// C. A real run, really interrupted, really recovered.
// ---------------------------------------------------------------------------

/// Kill a running mutation and repair the tree through the production recovery path.
///
/// The claim is narrow and worth stating: **recovery after an actual interruption**.
/// That the record is written *before* the mutation is a separate property, and this test
/// does not establish it — it could not, because it only ever observes states in which
/// both already exist.
#[test]
#[ignore = "kills a real mutation run; run with --ignored"]
fn an_interrupted_real_run_is_recovered_through_the_production_path() {
    use std::os::unix::process::CommandExt as _;

    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);

    // The mutant sleeps for thirty seconds, so the window in which the tree is damaged
    // is long enough to interrupt deliberately rather than by luck. The baseline still
    // runs at full speed, because the delay only exists once the mutation is applied.
    let cat = catalogue(root, "slow", "u64 { 0 }", "u64 { 30_000 }");
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);
    assert_ne!(expected_mutated, SOURCE, "the fixture must actually change");

    let mut runner = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root_s,
            "--catalogue",
            &cat.display().to_string(),
        ])
        // Its own process group, so the kill below reaches the `cargo` and `rustc`
        // descendants too and cannot reach the test runner.
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");
    let pgid = runner.id();

    let record = root.join("target/.nazm-mutation-backup");

    // Ready means **both**: the record exists *and* the source holds the mutated bytes.
    // Either alone is a state the runner passes through, and killing there would test a
    // different thing than the one claimed.
    until(
        "the run to be mid-mutation",
        Duration::from_secs(180),
        || {
            let has_record = record.exists();
            let mutated = fs::read_to_string(&src).is_ok_and(|s| s == expected_mutated);
            match (has_record, mutated) {
                (true, true) => Ok(()),
                (false, true) => Err("the mutated bytes are present but the record is not".into()),
                (true, false) => Err("the record is present but the source is not mutated".into()),
                (false, false) => Err("neither the record nor the mutation has appeared".into()),
            }
        },
    );

    // Kill the whole group. Reaping the runner alone would say nothing about the `cargo`
    // it spawned, which is still writing to the tree.
    let killed = Command::new("kill")
        .args(["-KILL", &format!("-{pgid}")])
        .status()
        .expect("kill the process group");
    assert!(killed.success(), "could not signal the process group");

    runner.wait().expect("reap the runner");

    // Reaping the direct child does not establish that its descendants are gone; they
    // were signalled, but "signalled" is not "stopped". Confirm before recovering,
    // because a surviving `cargo` could still write to the very file about to be
    // compared and restored.
    until(
        "every process in the interrupted run's group to stop",
        Duration::from_secs(60),
        || {
            let out = Command::new("pgrep")
                .args(["-g", &pgid.to_string()])
                .output()
                .expect("pgrep");
            let survivors = String::from_utf8_lossy(&out.stdout);
            let survivors: Vec<&str> = survivors.split_whitespace().collect();
            if survivors.is_empty() {
                Ok(())
            } else {
                Err(format!("still running: {survivors:?}"))
            }
        },
    );

    // The stranded state, inspected before anything repairs it.
    assert!(
        record.exists(),
        "an interrupted run left no recovery record, so nothing could repair it"
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read source"),
        expected_mutated,
        "the interruption was expected to leave the mutation applied"
    );

    // Recover through the production path. Note what changed: a real run no longer does
    // this for itself, because it cannot establish that the descendants above have
    // stopped. This test *did* establish it, one assertion ago, which is the only reason
    // the attestation flag is honest here.
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "recovery failed: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "recovery must name what it repaired: {}",
        recovered.stdout
    );

    assert_clean(root);
}

/// The window between restoring a file and clearing its record, reached for real.
///
/// A run killed there leaves an intact tree and a stale record. Recovery must recognise
/// that and clear it, rather than reporting a conflict on a tree that needs no repair —
/// which is what the first version did. The unit test covers the same case on a
/// hand-built record; this one gets there through an actual run.
#[test]
#[ignore = "drives a nested cargo build; run with --ignored"]
fn a_record_stranded_after_the_source_was_restored_is_cleared_not_disputed() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();

    // A complete, successful run: the source ends up restored and the record removed.
    let c = catalogue(root, "caught", "u64 { 42 }", "u64 { 43 }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &c.display().to_string(),
    ]);
    assert_eq!(run.status, Some(0), "{}", run.stdout);

    // Now reconstruct exactly the state a kill between those two writes leaves: the file
    // is already original, and the record the run had written is still there. Taken from
    // the real run's own bytes rather than composed by hand.
    let record = root.join("target/.nazm-mutation-backup");
    fs::write(
        &record,
        format!(
            "nazm-mutation-backup 2\n{SRC_REL}\n{}\n{}\n{SOURCE}",
            fnv("pub fn answer() -> u64 { 43 }"),
            fnv(SOURCE)
        ),
    )
    .expect("re-strand the record");

    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "an already-restored tree is not a conflict: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("already-restored {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}

/// FNV-1a, matching the runner's own digest.
///
/// Duplicated rather than exported: a test that imports the function under test to
/// compute its expected value checks only that the function equals itself.
fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ---------------------------------------------------------------------------
// D. A descendant that outlives its coordinator.
// ---------------------------------------------------------------------------

/// The case the lock does **not** cover, and the reason recovery is no longer automatic.
///
/// Killing a mutation run releases its lock immediately — Rust opens files `O_CLOEXEC`,
/// so the `cargo` and `rustc` processes it spawned never held the lock and are not killed
/// with it. They keep running and keep writing. A digest of the source does not help: it
/// says what the file held at the instant it was read, not that nothing will write it
/// next.
///
/// So this test kills **only the coordinator**, proves a descendant is still alive, and
/// requires the next run through the production entry point to refuse. It deliberately
/// does *not* stop the descendant first — doing the missing safety step inside the test
/// and then asserting success would be the test proving a property the product does not
/// have.
#[test]
#[ignore = "kills a real mutation run and leaves a descendant running; run with --ignored"]
fn a_second_run_refuses_while_a_descendant_of_the_dead_coordinator_survives() {
    use std::os::unix::process::CommandExt as _;

    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);
    let ready = root.join("descendant.pid");

    let cat = catalogue(root, "slow", "u64 { 0 }", "u64 { 30_000 }");
    let cat_s = cat.display().to_string();
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);
    let record = root.join("target/.nazm-mutation-backup");

    // Its own group, so cleanup at the end can reach every descendant. The kill in the
    // middle of this test is aimed at the coordinator alone, by pid.
    let mut runner = Command::new(XTASK)
        .args(["mutate-run", "--root", &root_s, "--catalogue", &cat_s])
        .env("NAZM_FIXTURE_READY", &ready)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");
    let pgid = runner.id();

    // 2. Ready means all three: the record, the mutated bytes, and a descendant that has
    //    announced itself. Waiting on fewer would leave which-state-we-killed-in to luck.
    let descendant = wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        MUTANT_DELAY_MS,
        Duration::from_secs(240),
    );

    // 3. Only the coordinator. Not the group.
    runner.kill().expect("kill the coordinator");
    runner.wait().expect("reap the coordinator");

    // 4. And the descendant is still there, holding the workspace open.
    assert!(
        pid_alive(descendant),
        "the descendant exited on its own; this test proves nothing unless it outlives \
         its coordinator"
    );

    // 5. A second run, through the production entry point, with that descendant running.
    let second = xtask(&["mutate-run", "--root", &root_s, "--catalogue", &cat_s]);

    // 6. It must refuse. The lock is free — the kernel released it when the coordinator
    //    died — so nothing about the lock stops this; the refusal has to come from the
    //    stranded record, and it has to be a refusal rather than a repair.
    assert_ne!(
        second.status,
        Some(0),
        "a second run started: {}",
        second.stdout
    );
    assert!(
        second.says("recovery is not automatic"),
        "the refusal must say why: {}",
        second.stdout
    );
    assert!(
        second.says("pgrep"),
        "and must give a procedure, not a suggestion: {}",
        second.stdout
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read source"),
        expected_mutated,
        "the refused run restored the source anyway, under a live writer"
    );
    assert!(
        record.exists(),
        "the refused run cleared the recovery record"
    );

    // Recovery without the attestation must refuse on the same grounds.
    let unattested = xtask(&["mutate-recover", "--root", &root_s]);
    assert_ne!(unattested.status, Some(0), "recovery ran unattested");
    assert!(
        unattested.says("recovery is not automatic"),
        "{}",
        unattested.stdout
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read source"),
        expected_mutated,
        "unattested recovery restored anyway"
    );
    assert!(descendant > 0);

    // 7. Now clean up what this test started, and only that.
    let _ = Command::new("kill")
        .args(["-KILL", &format!("-{pgid}")])
        .status();
    until(
        "the test's own process group to stop",
        Duration::from_secs(60),
        || {
            let out = Command::new("pgrep")
                .args(["-g", &pgid.to_string()])
                .output()
                .expect("pgrep");
            let s = String::from_utf8_lossy(&out.stdout);
            let alive: Vec<&str> = s.split_whitespace().collect();
            if alive.is_empty() {
                Ok(())
            } else {
                Err(format!("still running: {alive:?}"))
            }
        },
    );

    // Only now is the attestation true, and only now does recovery run.
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "recovery failed: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}

fn pid_alive(pid: u32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------------------------------------------------------------------------
// E. A restore that fails, at the real boundary, in a real run.
// ---------------------------------------------------------------------------

/// The injection cannot be taken back out, and the run must stop rather than continue.
///
/// The failure is injected where it actually happens — the source file is made read-only
/// while the mutation is applied, so the runner's own `fs::write` fails — rather than
/// through a filesystem abstraction. Two catalogue entries, so "the run stopped" is an
/// observation about the second one never being attempted and not an inference.
#[test]
#[ignore = "drives a real mutation run; run with --ignored"]
fn a_failed_restore_keeps_the_record_and_abandons_the_rest_of_the_catalogue() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);
    let ready = root.join("descendant.pid");
    let record = root.join("target/.nazm-mutation-backup");

    // Short, because this one is not interrupted — it just has to still be inside the
    // mutated window when the file is made unwritable.
    const DELAY: u64 = 3_000;
    let cat = root.join("two.toml");
    fs::write(
        &cat,
        format!(
            "[[mutation]]\n\
             name = \"slow\"\nwhy = \"fixture\"\nfile = \"{SRC_REL}\"\n\
             find = \"u64 {{ 0 }}\"\nreplace = \"u64 {{ {DELAY} }}\"\n\n\
             [[mutation]]\n\
             name = \"second\"\nwhy = \"must never be attempted\"\nfile = \"{SRC_REL}\"\n\
             find = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 43 }}\"\n"
        ),
    )
    .expect("catalogue");
    let expected_mutated = SOURCE.replacen("u64 { 0 }", &format!("u64 {{ {DELAY} }}"), 1);

    let runner = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root_s,
            "--catalogue",
            &cat.display().to_string(),
        ])
        .env("NAZM_FIXTURE_READY", &ready)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");

    wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        DELAY,
        Duration::from_secs(240),
    );

    // Injected at the restoration boundary: the runner's next write to this file is the
    // one that puts the original back, and it will now fail.
    set_mode(&src, 0o444);

    let out = runner.wait_with_output().expect("reap runner");
    let said = String::from_utf8_lossy(&out.stdout).into_owned();

    assert!(!out.status.success(), "the run reported success: {said}");
    assert!(said.contains("NOT RESTORED"), "{said}");
    assert!(
        said.contains("could not be restored"),
        "the failure must propagate with its cause: {said}"
    );
    assert!(
        !said.contains("second"),
        "the run injected the next mutation on top of one it could not remove: {said}"
    );

    assert!(
        record.exists(),
        "the record was cleared after a failed restore"
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read"),
        expected_mutated,
        "precondition for recovery: the file is still the mutated one the record names"
    );

    // Lift the injected failure, then recover for real.
    set_mode(&src, 0o644);
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "recovery failed: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}
```

