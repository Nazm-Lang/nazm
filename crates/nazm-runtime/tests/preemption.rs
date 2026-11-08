//! The preemption machinery's own logic, run — not read — on any host with `clang` (R1.5 6.3).
//!
//! Preemption itself is macOS's before 1.0: a signal interrupts a worker's thread and a handler
//! yields only where the thread was in generated code. The three pieces that decide *whether* to
//! interrupt and *whether* to yield are plain LLVM IR over the worker's and the task's layout, so
//! each is cut out of the runtime's real text, compiled for the host, and driven by a C harness that
//! lays out workers the way the runtime does and counts what the piece does: the monitor
//! (`nz.pool_watch`), the interrupt's yield (`nz.pool_preempt`) and the signal handler
//! (`nz.os_preempt_handler`). Each case states a situation and the exact outcome.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use nazm_runtime::switch::SwitchTarget;
use nazm_runtime::Needs;

/// The text of one `define … @name(` through its closing `}` line, from `text`.
fn function(text: &str, name: &str) -> String {
    let head = format!("@{name}(");
    let start = text
        .match_indices("define ")
        .map(|(i, _)| i)
        .find(|&i| text[i..].lines().next().is_some_and(|l| l.contains(&head)))
        .unwrap_or_else(|| panic!("no `define` of {name}"));
    let end = text[start..].find("\n}\n").expect("an unclosed function") + start + 3;
    text[start..end].to_owned()
}

fn runtime_text() -> String {
    nazm_runtime::emit_runtime(Needs {
        tasks: true,
        ..Needs::default()
    })
}

/// A scratch directory of this test's own.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nazm-preemption-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Compile `ir` and `harness` (C) for the host, run it, and return what it printed.
fn run(dir: &Path, ir: &str, harness: &str) -> Option<String> {
    if Command::new("clang").arg("--version").output().is_err() {
        assert!(
            std::env::var("NAZM_REQUIRE_HOST_TOOLS").as_deref() != Ok("1"),
            "NAZM_REQUIRE_HOST_TOOLS=1, and there is no clang"
        );
        eprintln!("SKIP: no clang on this machine");
        return None;
    }
    std::fs::write(dir.join("piece.ll"), ir).unwrap();
    std::fs::write(dir.join("harness.c"), harness).unwrap();
    let built = Command::new("clang")
        .current_dir(dir)
        .args(["-O0", "-w", "-o", "t", "piece.ll", "harness.c", "-lpthread"])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(dir.join("t")).output().unwrap();
    assert!(ran.status.success(), "{:?}", ran);
    Some(String::from_utf8(ran.stdout).unwrap())
}

#[test]
fn the_monitor_asks_and_interrupts_exactly_a_stuck_worker_with_a_task_waiting() {
    let ir = format!(
        "@nz.pool_workers = global ptr null\n@nz.pool_count = global i64 0\n\
         declare void @nz.os_preempt(ptr)\n\n{}",
        function(&runtime_text(), "nz.pool_watch")
    );
    // A worker is 512 bytes: the run flag at 408, the switch count at 392, the count last seen at
    // 400, the resumed queue at 256, the unstarted queue at 352, the request at 416, the thread
    // at 296 — the offsets `nz.pool_watch` reads.
    let harness = r#"
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#ifdef __APPLE__
#define SYM(n) "_" n
#else
#define SYM(n) n
#endif
extern int64_t pool_count __asm__(SYM("nz.pool_count"));
extern void *pool_workers __asm__(SYM("nz.pool_workers"));
void pool_watch(void) __asm__(SYM("nz.pool_watch"));
static int interrupts; static void *last_thread;
void os_preempt(void *t) __asm__(SYM("nz.os_preempt"));
void os_preempt(void *t) { interrupts++; last_thread = t; }
#define AT(w, off, T) (*(T *)((char *)(w) + (off)))
static char ws[4][512]; static void *arr[4];
static void set(int i, int64_t run, int64_t cnt, int64_t last, void *rq, void *fq) {
  memset(ws[i], 0, 512);
  AT(ws[i], 408, int64_t) = run; AT(ws[i], 392, int64_t) = cnt; AT(ws[i], 400, int64_t) = last;
  AT(ws[i], 256, void *) = rq; AT(ws[i], 352, void *) = fq;
}
static void look(const char *what, int n) {
  interrupts = 0; last_thread = 0;
  for (int i = 0; i < n; i++) arr[i] = ws[i];
  pool_workers = arr; pool_count = n; pool_watch();
  printf("%s: asked=%lld interrupts=%d at_thread=%d last=%lld\n", what,
         (long long)AT(ws[0], 416, int64_t), interrupts,
         last_thread == (void *)((char *)ws[0] + 296), (long long)AT(ws[0], 400, int64_t));
}
int main(void) {
  int x;
  set(0, 1, 5, 5, &x, 0);    look("stuck, resumed queue waiting", 1);
  set(0, 1, 5, 5, 0, &x);    look("stuck, unstarted queue waiting", 1);
  set(0, 1, 5, 5, 0, 0);     look("stuck, nobody waiting", 1);
  set(0, 1, 6, 5, &x, &x);   look("moving, tasks waiting", 1);
  set(0, 0, 5, 5, &x, &x);   look("idle, tasks waiting", 1);
  set(0, 1, 5, 5, &x, 0); set(1, 1, 5, 5, 0, 0); look("two workers, one stuck", 2);
  printf("second: asked=%lld interrupts_total_workers=%d\n",
         (long long)AT(ws[1], 416, int64_t), interrupts);
  return 0;
}
"#;
    let dir = scratch("monitor");
    let Some(out) = run(&dir, &ir, harness) else {
        return;
    };
    assert_eq!(
        out,
        "stuck, resumed queue waiting: asked=1 interrupts=1 at_thread=1 last=5\n\
         stuck, unstarted queue waiting: asked=1 interrupts=1 at_thread=1 last=5\n\
         stuck, nobody waiting: asked=0 interrupts=0 at_thread=0 last=5\n\
         moving, tasks waiting: asked=0 interrupts=0 at_thread=0 last=6\n\
         idle, tasks waiting: asked=0 interrupts=0 at_thread=0 last=5\n\
         two workers, one stuck: asked=1 interrupts=1 at_thread=1 last=5\n\
         second: asked=0 interrupts_total_workers=1\n"
    );
}

#[test]
fn an_interrupted_task_yields_only_when_its_worker_asked_and_no_failure_is_unwinding() {
    let ir = format!(
        "@nz.cur_task = thread_local global ptr null\n@nz.failed = global i1 false\n\
         declare void @nz.task_yield()\n\n{}",
        function(&runtime_text(), "nz.pool_preempt")
    );
    // A task holds its worker at 40; a worker's request is at 416.
    let harness = r#"
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#ifdef __APPLE__
#define SYM(n) "_" n
#else
#define SYM(n) n
#endif
extern __thread void *cur_task __asm__(SYM("nz.cur_task"));
extern _Bool failed __asm__(SYM("nz.failed"));
void pool_preempt(void) __asm__(SYM("nz.pool_preempt"));
static int yields;
void task_yield(void) __asm__(SYM("nz.task_yield"));
void task_yield(void) { yields++; }
static char worker[512], task[64];
static void go(const char *what, int have_task, _Bool fail, int64_t asked) {
  memset(worker, 0, 512); memset(task, 0, 64);
  *(void **)(task + 40) = worker; *(int64_t *)(worker + 416) = asked;
  cur_task = have_task ? task : 0; failed = fail; yields = 0;
  pool_preempt();
  printf("%s: yields=%d request_now=%lld\n", what, yields, (long long)*(int64_t *)(worker + 416));
}
int main(void) {
  go("asked", 1, 0, 1);
  go("not asked", 1, 0, 0);
  go("no task", 0, 0, 1);
  go("a failure unwinding", 1, 1, 1);
  return 0;
}
"#;
    let dir = scratch("preempt");
    let Some(out) = run(&dir, &ir, harness) else {
        return;
    };
    assert_eq!(
        out,
        "asked: yields=1 request_now=0\n\
         not asked: yields=0 request_now=0\n\
         no task: yields=0 request_now=1\n\
         a failure unwinding: yields=0 request_now=1\n"
    );
}

#[test]
fn the_signal_handler_yields_only_for_a_program_counter_inside_the_generated_code_section() {
    for (target, pc_offset) in [
        (SwitchTarget::Aarch64Darwin, 272),
        (SwitchTarget::X86_64Darwin, 144),
    ] {
        let text = nazm_runtime::platform::emit_stack(target);
        let handler = function(&text, "nz.os_preempt_handler");
        assert!(
            handler.contains(&format!("ptr %mc, i64 {pc_offset}")),
            "{target:?}: the program counter is not read at {pc_offset}"
        );
        // The section's bounds are the linker's symbols on macOS; here they are the two ends of one
        // array, compared the same way.
        let handler = handler
            .replace("@\"\\01section$start$__TEXT$__nz_text\"", "@nzsect")
            .replace(
                "@\"\\01section$end$__TEXT$__nz_text\"",
                "getelementptr inbounds ({ [64 x i8], i8 }, ptr @nzsect, i32 0, i32 1)",
            );
        let ir = format!(
            "@nz.os_preempt_yield = global ptr null\n\
             @nzsect = global {{ [64 x i8], i8 }} zeroinitializer\n\n{handler}"
        );
        let harness = format!(
            r#"
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#ifdef __APPLE__
#define SYM(n) "_" n
#else
#define SYM(n) n
#endif
extern char nzsect[65];
void os_preempt_handler(int, void *, void *) __asm__(SYM("nz.os_preempt_handler"));
extern void *yield_ptr __asm__(SYM("nz.os_preempt_yield"));
static int yields; static void y(void) {{ yields++; }}
static char uc[512], mc[512];
static void at(const char *what, uint64_t pc) {{
  memset(uc, 0, 512); memset(mc, 0, 512);
  *(void **)(uc + 48) = mc; *(uint64_t *)(mc + {pc_offset}) = pc;
  yields = 0; yield_ptr = (void *)y; os_preempt_handler(16, 0, uc);
  printf("%s: yields=%d\n", what, yields);
}}
int main(void) {{
  uint64_t lo = (uint64_t)nzsect, hi = lo + 64;
  at("first byte", lo); at("middle", lo + 32); at("last byte", hi - 1);
  at("one past the end", hi); at("below", lo - 1); at("null", 0);
  return 0;
}}
"#
        );
        let dir = scratch(&format!("handler-{pc_offset}"));
        let Some(out) = run(&dir, &ir, &harness) else {
            return;
        };
        assert_eq!(
            out,
            "first byte: yields=1\nmiddle: yields=1\nlast byte: yields=1\n\
             one past the end: yields=0\nbelow: yields=0\nnull: yields=0\n",
            "{target:?}"
        );
    }
}
