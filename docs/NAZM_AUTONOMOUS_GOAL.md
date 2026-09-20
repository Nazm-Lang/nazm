# Nazm autonomous development goal

Version: 1.0  
Owner: Md Ferdous Alam  
Suggested repository location: `docs/NAZM_AUTONOMOUS_GOAL.md`

## Goal invocation

Read this entire document, inspect the actual repository, and achieve the goal below. Use this document as the persistent execution contract. Continue through milestones without requesting routine approval or stopping for adviser review. Keep verified checkpoints so work can resume after context compaction, interruption, or a new session. Never claim completion merely because a plan, prototype, or test count exists.

## 1. Founder directive

Build Nazm into an experimental, usable programming language with:

- A native compiler and predictable, specified semantics.
- A compiler implemented in Nazm that reproducibly rebuilds itself.
- An initial structured-concurrency implementation with explicit ownership, lifetime, cancellation, and cleanup rules.
- Reliable tooling for humans and coding agents.
- Measured execution, compilation, and memory behavior, with concrete improvements where evidence identifies bottlenecks.

Reuse the existing Rust implementation, language frontend, interpreter, diagnostics, native backend, tests, and evaluation machinery. This is an evolution of the repository, not a fresh project. The Rust bootstrap compiler and external backend/linker can remain part of the trusted toolchain.

The primary objective is a programming language. The comparative TypeScript experiment remains a way to test specific claims, not a prerequisite for continuing language development. Preserve registered protocols and historical results; never rewrite evidence to fit this direction.

Deliver an experimental developer release candidate locally. This goal does not authorize a public release or claim production readiness.

## 2. Authority, autonomy, and limits

This founder directive supersedes earlier conversational instructions to stop after M1f or any other individual milestone, to wait for an adviser prompt, or to seek approval for routine implementation choices. It does not override tool permissions, security controls, licensing obligations, or unrelated user changes.

You may autonomously:

- Inspect and edit project source, tests, scripts, specifications, examples, and project instructions.
- Make reversible implementation choices and document them in concise decision records.
- Add justified dependencies after checking actual toolchain compatibility and licenses.
- Execute bounded local verification, generated programs, benchmarks, and isolated mutation tests.
- Create local branches, clean worktrees, review packages, and checkpoint commits containing only this goal's work. Preserve pre-existing uncommitted changes; do not sweep unrelated work into a commit.
- Update stale project instructions narrowly to reflect this goal, retaining historical decisions and explaining supersession.

Do not autonomously publish, push, deploy, buy services, use paid model APIs, expose credentials, change account permissions, delete unrelated files, reset user work, or prune shared Docker resources. A missing remote is not a blocker to local development or local checks.

Ask only when an essential action lacks authorization, an essential external dependency is inaccessible, or an architectural conflict cannot be resolved within this directive without breaking an established user-facing commitment. First finish independent authorized work. Present a concrete decision and evidence, not a general request to continue.

An agent session cannot run beyond its platform's enforced time, budget, or context limits. Do not bypass those controls. Persist the exact state and next command before yielding when possible. Such a yield is an interruption, not goal completion.

## 3. Starting evidence: verify, do not assume

The latest supplied handoff reports M1e completed from base commit `4326100`, with cumulative M1d/M1e changes uncommitted. This is historical evidence, not a requirement to return to that commit.

Reported native coverage includes Int/Bool, checked arithmetic, comparisons, conditional values, statement-position if, immutable and mutable locals, assignments, while, non-recursive direct function calls, break, and continue. Native return was proposed as M1f and is not yet confirmed implemented. Strings and native recursion remained unsupported.

Reported checks: 383 workspace tests, 89 native-build tests included in that total, six lifecycle tests separately, five repository checks, and 19 selftest cases. Ten Docker tests returned successfully without exercising Docker. One target was exercised; CI and other targets were unverified. Do not reuse these counts as current evidence.

At startup:

1. Read applicable project instructions, this document, specification, architecture, roadmap, direction record, manifests, and current handoff.
2. Inspect Git, toolchain, platform, disk space, and active mutation/build processes. Preserve the current tree.
3. Determine which capabilities actually exist. Validate only enough to establish a trustworthy baseline.
4. If a mutation run owns the tree, do not edit it or launch competing checks. Never delete a lock because it looks stale.
5. Create or update a small capability matrix and execution ledger. Record confirmed gaps and the dependency order to close them.
6. Continue directly into the first unfinished milestone. Do not repeat completed work or produce another audit instead of implementation.

## 4. Continuous execution loop

For each bounded increment:

1. Select the highest-priority unmet acceptance criterion whose prerequisites exist.
2. State scope, explicit exclusions, and the semantic contract before implementing.
3. Reuse existing mechanisms; make the smallest coherent change.
4. Add tests for behavior, failure paths, and meaningful regressions.
5. Run focused verification, then required repository gates against the final state.
6. Review the diff for semantic drift, dead scaffolding, unsafe assumptions, and inaccurate claims.
7. Update the ledger, capability matrix, and affected documentation; create a local checkpoint when appropriate.
8. Immediately begin the next increment. Do not stop with “ready for review” or ask whether to continue.

Keep progress updates concise. Spend most execution time on the language, compiler, and runtime rather than reports or repeated harness improvements.

Maintain `docs/goal-status.md` with:

- Goal version, branch, base/current commit, and uncommitted work description.
- Capability status: implemented, verified, partial, planned, or blocked.
- Latest meaningful commands and outcomes, including skip conditions.
- Active processes and temporary resources, if any.
- Current task, next exact action, and unresolved decisions.
- Completion checklist for Section 13.

On resumption, check this ledger against Git and current processes before trusting it. Never treat a checkpoint summary as more authoritative than source and fresh execution evidence.

## 5. Semantic and architectural invariants

Preserve these unless a documented compatibility-preserving extension is needed:

- Plain UTF-8 `.nz` source remains authoritative. Derived caches are rebuildable.
- Frontend validity and native-backend coverage are different: valid unsupported programs receive a backend limitation diagnostic.
- Checked signed arithmetic has the specified behavior at every optimization level. `Int::MIN % -1` is zero; division by zero and unrepresentable signed results fail as specified.
- Operand and argument evaluation remains left to right. Nonselected branches do not execute.
- Falls-through-with-value, falls-through-without-value, and transferred control remain distinct. An absent tail is not proof of divergence.
- Loop targets follow existing lexical rules. The latest handoff reports that control transfer in an inner loop's condition targets the enclosing loop; preserve verified frontend/interpreter semantics.
- The compiler's recursive AST/IR work and destruction remain within its controlled-stack arrangement or a demonstrably safe replacement. Do not accidentally return recursive owned structures to an inadequately sized caller stack.
- Runtime resource limits are implementation contracts, not silently invented language rules.
- Effects, authority/capabilities, and information flow are distinct concerns. Do not claim one proves the others.
- Field access remains symbolic until layout is legitimately required. Do not prematurely freeze public ABI or layout.
- Reuse the LLVM-text backend for near-term delivery. Do not add a second backend, every proposed IR layer, an optimizer framework, or a custom scheduler merely to match an aspirational diagram.
- No dummy values, silent fallback types, poison-producing arithmetic shortcuts, or unreachable terminators may conceal unsupported behavior or broken lowering.

## 6. Milestone A: complete existing scalar control flow

First implement or verify M1f: native `return` using existing language semantics.

Prefer direct typed returns while no cleanup mechanism needs a shared exit. Cover early return, all-paths-return bodies, nested loops, returns in value positions, nested return operands, caller/callee distinction, and skipped operands/arguments/stores. Only the platform wrapper prints main's result.

Use explicit expected outcomes plus interpreter comparison at unoptimized and optimized settings. Validate block terminators, actual phi predecessors, and source locations. Preserve unsupported recursion until its contract is implemented.

Exit criteria: the supported Int/Bool scalar subset has coherent function, branch, loop, and transfer semantics, with a current conformance table. Do not call the whole language complete.

## 7. Milestone B: define and implement the bootstrap language subset

Derive features from a concrete compiler design. Sketch the actual compiler's data structures and operations first, then implement them in small vertical slices across specification, checking, interpreter/reference execution, native lowering, runtime support, and tests.

Required capabilities, with ordering adjusted for dependencies:

### B1. Text and bytes

Support reading and inspecting source data, bounded indexing, lengths, slices, construction, and output assembly. Specify byte offsets versus Unicode scalar values, valid UTF-8 boundaries, embedded NUL behavior, bounds failures, equality, lifetime, copying, and ownership. Keep source locations consistent with diagnostics. Do not use host-language string behavior as an unstated specification.

### B2. Data structures and memory

Provide enough records, tagged alternatives, sequences, and lookup structures to represent tokens, syntax, symbols, types, diagnostics, and generated output. Pattern matching or another explicit tag-dispatch mechanism must be sufficient for compiler logic.

Start with a narrow concrete type system if that avoids premature generics. A bootstrap compiler need not require traits, unrestricted generics, closures, or arbitrary shared pointer graphs.

Specify initialization, copy/move behavior, mutation, aliasing, destruction, growth failure, and bounds checking before relying on mutable collections. Arena indices are a useful compiler-internal option, not automatically a permanent language-wide restriction.

Introduce the smallest runtime support needed. Respect existing unsafe-code boundaries; any new unsafe boundary needs a stated invariant, ownership contract, and focused verification. Do not claim ownership safety while silently outsourcing all values to unconstrained shared host references.

### B3. I/O, errors, and authority

Implement enough command-line, file, and output support to read compiler inputs, write artifacts, report diagnostics, and invoke the existing external backend/linker where necessary.

Choose a narrow documented authority/effect contract that can grow toward the existing effect design. Do not finalize `main(io: IO)` merely because a previous handoff suggested it. Full effect polymorphism and general handlers are not prerequisites for the first compiler, but partial enforcement must be labelled accurately.

Specify error propagation and cleanup. Compile-time tooling must never silently read arbitrary ambient inputs while claiming reproducibility.

### B4. Modules and builds

Implement deterministic multi-file name resolution, imports, visibility sufficient for the compiler, cycle rules, diagnostics, and a reproducible build command. Avoid a network package registry. A local dependency model and locked external toolchain are sufficient initially.

### B5. Recursion strategy

Choose an explicit strategy for recursive algorithms. A counter alone is not a native stack-safety guarantee. Options include an adequately justified native stack contract, checked runtime frames, or an iterative compiler implementation with explicit work stacks while native recursion remains unsupported.

Use iterative compiler algorithms if that is the smallest sound bootstrap path. Do not block self-hosting on general native recursion when the compiler can be expressed without it. If recursion is implemented, distinguish call-depth policy from actual stack-exhaustion handling and test the supported environment.

Exit criteria: small Nazm programs can tokenize a source file, build and traverse structured syntax data, resolve names across files, construct output, and report located errors. Each must execute natively with documented memory and I/O behavior. These programs should become reusable components of the compiler, not throwaway demonstrations.

## 8. Milestone C: compiler implementation in Nazm

Implement the compiler in Nazm incrementally: source management, lexing, parsing, resolution, type checking, lowering, backend emission, diagnostics, and driver. Reuse the established specification, test corpus, LLVM contract, and architecture lessons.

Keep the Rust compiler as bootstrap and reference. Do not delete it after the first successful build. Port responsibilities deliberately rather than translating every Rust dependency or historical harness module.

The compiler written in Nazm must implement the entire declared bootstrap subset that its own source uses, plus the conformance commitments assigned to that release. It must compile ordinary independent programs, not recognize only its own source or delegate semantic compilation back to Rust.

A wrapper written in Nazm that shells out to the Rust compiler is not self-hosting. Emitting LLVM and invoking an external assembler/compiler/linker is acceptable when documented. External runtime support is acceptable when its responsibilities are explicit and it does not hide the language frontend or compiler implementation.

Validate valid programs, invalid programs, diagnostics, arithmetic boundaries, control transfers, modules, data lifetimes, and compiler failures against explicit expectations. Differential agreement detects disagreement, not shared bugs.

Exit criteria: the compiler source is genuinely Nazm, the Rust toolchain compiles it to a native executable, and that executable compiles the bootstrap corpus and representative independent programs without calling the Rust frontend.

## 9. Milestone D: native self-hosting and reproducible bootstrap

Implement a scripted chain using the same Nazm compiler source S:

1. The retained Rust bootstrap implementation compiles S into C1.
2. C1 compiles S into C2.
3. C2 compiles S into C3.
4. C2 and C3 compile and run the release conformance corpus and independent example programs.

Use separate clean output directories and record source digest, toolchain identities, commands, target, flags, external libraries, and relevant environment inputs. Eliminate accidental fallback to stale binaries or Rust compilation during later stages. Failure must propagate; no script may print success after a failed stage.

Require stable C2/C3 outputs for the supported pinned reproducible configuration. Prefer byte-identical compiler executables. If the system linker introduces unavoidable metadata, document narrowly which fields differ, compare deterministic compiler-produced artifacts and normalized binaries, and report this limitation prominently. Never normalize away code, data, or arbitrary differences to force equality.

Rebuild from a fresh local checkout/copy using documented prerequisites. Include offline operation after dependencies are obtained. Self-rebuilding is not proof of compiler trustworthiness or absence of common-mode bugs.

Exit criteria: all three stages execute successfully; later stages do not rely on the Rust frontend; conformance passes; reproducibility evidence and remaining external dependencies are recorded. An interpreted compiler or a single successful self-compilation is not completion of this milestone.

## 10. Milestone E: initial structured concurrency

Do not let self-hosting silently erase the concurrent-language objective. After the sequential ownership/resource model is stable, implement one coherent initial concurrency model. It may be developed before or after initial self-hosting according to dependencies; final bootstrap verification must include all compiler changes delivered under this goal.

Required initial behavior:

- Scoped task creation and joining with defined result/error propagation.
- Explicit cross-task capture/transfer rules that reject illegal shared mutation.
- A bounded channel or equivalent explicit ownership-transfer primitive with specified close behavior.
- Cooperative cancellation with defined observation points and cleanup behavior.
- Defined lifetime of tasks and captured resources; no implicit detached work.

A scoped OS-thread implementation is an acceptable first runtime if specified honestly. Do not claim M:N scheduling, green threads, work stealing, or massive task density until implemented and measured. Keep the interface separable from that runtime strategy.

Test cancellation, failed tasks, joins, channel close, blocked send/receive, nested scopes, and resource cleanup. Compare permitted outcomes and invariants, not exact scheduling traces. Use deadlines and stress cases where they address a specific race. No claim of deadlock freedom follows from ownership checking.

Exit criteria: native concurrent examples run, forbidden capture/sharing examples are rejected under the specified model, lifecycle tests pass, and at least one bounded parallel workload has measured outcomes. Record performance even if slower than sequential execution. If necessary, extend the compiler written in Nazm and rerun the bootstrap chain.

## 11. Milestone F: AI-native developer experience

Define AI-native for this release as reliable machine-consumable development interfaces, not embedded model calls or new keywords named after AI.

Deliver:

- Stable structured diagnostics with codes, source ranges, severities, and a versioned JSON schema.
- Deterministic check/build/test commands, useful exit statuses, and explicit unsupported-feature reports.
- A grammar artifact and concise language guide usable by coding agents.
- A canonical formatting path with semantic-preservation tests and idempotence, or an explicitly narrowed formatting scope with a plan to finish it before claiming full formatting support.
- Machine-applicable fixes for a small set of well-understood cases, with exact preconditions. Refuse stale or overlapping edits and recheck the result.
- Reproducible project examples and a command that inventories actual compiler capabilities/version.
- One scripted agent-style repair workflow using deterministic fixtures and structured diagnostics. No paid API is necessary.

Use stable CLI/JSON as the first interface. LSP/MCP adapters may follow if justified; they are not a reason to duplicate compiler logic.

Do not claim reduced agent cost or superiority to TypeScript from tooling existence. Preserve the evaluator and its fairness rules. Finish missing corpus/driver work only when conducting that comparative experiment; otherwise report that comparative benefit is unmeasured.

Exit criteria: a client can diagnose a program, apply an allowed fix under preconditions, recheck/build it, and consume the result without scraping prose. Grammar and guide match implemented syntax. The repair example is independently reproducible.

## 12. Milestone G: performance and release integration

Establish a reproducible measurement suite before optimizing. Separate compiler speed, generated-program speed, peak memory, startup, and concurrency overhead.

Use workloads relevant to this project: compiler self-build, text/token processing, a structured-data workload, allocation/collection use, and a bounded concurrent workload. Record machine, OS, target, versions, commit, flags, input sizes, repetitions, variability, and cold/warm conditions.

Runtime measurements must use genuinely external runtime inputs where needed; parameters and comparisons alone do not prevent whole-program folding. Verify equivalent work and results. Do not disable optimizations merely to manufacture a favorable comparison.

Measure against the previous Nazm checkpoint and a justified external reference where locally available. Do not promise arbitrary multipliers. Once baselines exist, set reasoned regression thresholds before tuning, record them, and investigate violations rather than silently relaxing them.

Profile the dominant relevant bottleneck. Complete at least one evidence-driven optimization pass, retaining improvements only when correctness holds and repeated measurements support the claimed benefit. An honest no-improvement finding is acceptable; fabricated superiority is not.

Do not implement speculative layout selection, a second backend, tensor/GPU compilation, distributed actors, a package registry, or comprehensive information-flow analysis simply to make the feature list longer. Record these as future research unless a demonstrated prerequisite requires a narrowly scoped part.

Prepare a local experimental release candidate with source, bootstrap instructions, supported-platform statement, examples, toolchain requirements, license notices, test/benchmark commands, capability matrix, and known limitations. Other targets may remain explicitly unsupported or unverified. Local execution of CI-equivalent commands is useful evidence; a workflow file alone is not.

## 13. Definition of done

Declare this goal complete only when all items below are satisfied:

- [ ] Current scalar control-flow coverage is verified, including return and transfers inside values.
- [ ] The declared bootstrap subset has specified text, structured data, collections, memory behavior, file I/O, errors, and modules sufficient for the compiler.
- [ ] The compiler frontend and lowering/backend emission are implemented in Nazm, with documented external dependencies.
- [ ] The native C1 → C2 → C3 bootstrap succeeds from the same source and passes conformance and reproducibility checks.
- [ ] Ordinary independent programs compile with the compiler written in Nazm.
- [ ] Initial structured concurrency, transfer/capture rules, cancellation, and cleanup are implemented and verified.
- [ ] Structured developer interfaces, grammar/guide, formatting, and the deterministic repair workflow are delivered within an explicit supported scope.
- [ ] Performance and memory baselines exist, a profiling/optimization pass is reported, and no unsupported speed claims remain.
- [ ] A clean rebuild of the integrated final state succeeds on at least one explicitly supported target.
- [ ] A final local review/release package identifies exact source state, verification, skipped coverage, limitations, and reproduction steps.

Self-hosting is a major intermediate milestone, not permission to stop before the remaining release objectives. Conversely, this finite experimental release is not a promise to finish every future research feature or production ecosystem component.

If an item is genuinely blocked, record it as blocked and continue independent work. Do not relabel it optional to obtain a completed checklist. When no authorized useful work remains, return a precise partial-completion report with the evidence and smallest blocking decision.

## 14. Verification discipline and known traps

- Source inspection, a successful compile, runtime behavior, mutation discrimination, performance, and platform support are different evidence classes.
- Test counts are not capability proofs. Report conditional skips honestly; do not add suite counts already included in workspace totals.
- Check the final edited state, not a state before the last fix.
- A normal exit code does not rule out a panic; assert exact expected outcomes.
- LLVM validation catches malformed IR, not semantic miscompilation. Keep explicit outcome checks.
- Use targeted mutation testing for high-risk semantics, not as a ritual after every edit. A semantic mutant must compile and fail for the intended reason. At final integration, run the applicable catalogue in isolation if feasible; distinguish obsolete/inapplicable mutations from survivors and never remove survivors merely to make the score green.
- Retain backups on restoration failure and stop further mutation. Keep production automatic stranded recovery disabled unless a later independently justified mechanism establishes quiescence. Operator attestation is not automatic verification.
- Run destructive test scenarios only against isolated disposable projects or copies. Never mix old/new lock protocols against one checkout.
- No AST nesting limit bounds call-graph depth, module dependency depth, or runtime call depth automatically. Use iterative traversal or separately justified limits where appropriate.
- Native frame counts do not by themselves bound stack bytes. Cleanup/cancellation guarantees require actual implementation, not just types or prose.
- Inputs loaded from files, toolchain versions, target details, and environment-sensitive behavior must be accounted for in reproducibility claims.

## 15. Final delivery and continuation behavior

At each checkpoint keep a short status record; do not generate large cumulative archives repeatedly. At major milestones retain useful review evidence without nesting earlier bundles.

Final delivery must include:

1. A concise outcome report and completed checklist.
2. Exact source commit plus any remaining diff.
3. Bootstrap script and recorded C1/C2/C3 results.
4. Supported-platform and dependency manifest.
5. Conformance, concurrency, tooling, and benchmark results with skips and limitations.
6. Reproduction instructions and independent example programs.
7. A local archive containing the relevant source and evidence, excluding secrets, build caches, and unrelated files.

Do not stop after writing a plan, finishing one milestone, creating a review bundle, or asking for adviser review. Continue until the complete definition of done is met, an actual external blocker prevents all remaining authorized work, or the platform requires a checkpoint and yield.
