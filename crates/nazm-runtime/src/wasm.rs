//! The WebAssembly runtime and entry (N64, `docs/architecture.md` §7.66): `wasm32-unknown-unknown`
//! — no OS, no C library, no heap, one thread, and every capability of the outside an import.
//!
//! A failure is recorded in plain globals, as on the board (§7.64), and handed to the host by the
//! `nazm_host.report` import; standard output and error reach the host only through
//! `nazm_host.write`, which the module imports only where the program writes. The module exports
//! `memory` and `nazm_main`.

use std::fmt::Write as _;

use crate::preamble;

/// The one module every import comes from.
pub const HOST_MODULE: &str = "nazm_host";

/// The shadow stack the linker reserves, in bytes: 64 KiB, as on the board.
pub const STACK: u64 = 0x1_0000;

/// What `nz.stack_check` keeps in reserve (N49's quarter).
pub const STACK_HEADROOM: u64 = STACK >> 2;

/// The `wasm-ld` line a module is linked with, after the objects.
pub const LINK: &str =
    "wasm-ld --no-entry --stack-first -z stack-size=65536 --export=nazm_main -o module.wasm";

/// The runtime: the failure state and its helpers, `write` over the host's import, and — where a
/// function in a call cycle checks it — the stack check against the linker's `__stack_low`.
#[must_use]
pub fn emit_runtime(stack: bool) -> String {
    let mut out = String::new();
    preamble(&mut out, "<nazm WebAssembly runtime>");
    out.push_str(&format!(
        r#"declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @nz.host_write(i32, ptr, i32) #0

@nz.failed = hidden global i1 false
@nz.errmsg = hidden global ptr null
@nz.errlen = hidden global i64 0

define hidden void @nz.fail(ptr %m, i64 %n) {{
entry:
  %had = load i1, ptr @nz.failed
  br i1 %had, label %done, label %record
record:
  store i1 true, ptr @nz.failed
  store ptr %m, ptr @nz.errmsg
  store i64 %n, ptr @nz.errlen
  br label %done
done:
  ret void
}}

; No C library keeps an `errno` here (N85): a foreign call's capture of it does nothing, and
; `c_errno()`, which would read it, is refused (`N0613`).
define hidden void @nz.errno_save() {{
entry:
  ret void
}}

define hidden i32 @nz.failed_now() {{
entry:
  %f = load i1, ptr @nz.failed
  %r = zext i1 %f to i32
  ret i32 %r
}}

; The C library's `write`, as generated code calls it, over the host's import: the only way out.
define hidden i64 @write(i32 %fd, ptr %p, i64 %n) {{
entry:
  %len = trunc i64 %n to i32
  call void @nz.host_write(i32 %fd, ptr %p, i32 %len)
  ret i64 %n
}}

attributes #0 = {{ "wasm-import-module"="{HOST_MODULE}" "wasm-import-name"="write" }}
"#
    ));
    out.push_str(&multi3());
    if stack {
        let _ = write!(
            out,
            r"
@__stack_low = external global i8
declare ptr @llvm.frameaddress.p0(i32)

define hidden i32 @nz.stack_check() {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %low = ptrtoint ptr @__stack_low to i64
  %limit = add i64 %low, {STACK_HEADROOM}
  %past = icmp ult i64 %sp, %limit
  %r = zext i1 %past to i32
  ret i32 %r
}}

; A function whose own frame is large asks this instead (Q1-A1-01): past the limit, or with less
; than `need` bytes between this frame and the limit.
define hidden i32 @nz.stack_check_frame(i64 %need) {{
entry:
  %fp = call ptr @llvm.frameaddress.p0(i32 0)
  %sp = ptrtoint ptr %fp to i64
  %low = ptrtoint ptr @__stack_low to i64
  %limit = add i64 %low, {STACK_HEADROOM}
  %past = icmp ult i64 %sp, %limit
  %room = sub i64 %sp, %limit
  %short = icmp ult i64 %room, %need
  %either = or i1 %past, %short
  %r = zext i1 %either to i32
  ret i32 %r
}}
"
        );
    }
    out
}

/// `__multi3`, which LLVM's checked `i64` multiply calls on wasm32: here, and in the WASI runtime
/// (Gate 3), because there is no compiler-rt.
pub(crate) fn multi3() -> String {
    MULTI3.to_owned()
}

const MULTI3: &str = r"
; The low 128 bits of a 128-bit product, which LLVM's checked `i64` multiply calls on wasm32
; (compiler-rt's `__multi3`, here because there is no compiler-rt): from 64-bit pieces only, so
; nothing in it lowers to itself.
define hidden void @__multi3(ptr %r, i64 %al, i64 %ah, i64 %bl, i64 %bh) {
entry:
  %all = and i64 %al, 4294967295
  %alh = lshr i64 %al, 32
  %bll = and i64 %bl, 4294967295
  %blh = lshr i64 %bl, 32
  %p0 = mul i64 %all, %bll
  %p1 = mul i64 %all, %blh
  %p2 = mul i64 %alh, %bll
  %p3 = mul i64 %alh, %blh
  %p0h = lshr i64 %p0, 32
  %p1l = and i64 %p1, 4294967295
  %p2l = and i64 %p2, 4294967295
  %m0 = add i64 %p0h, %p1l
  %mid = add i64 %m0, %p2l
  %p0l = and i64 %p0, 4294967295
  %midl = shl i64 %mid, 32
  %low = or i64 %p0l, %midl
  %p1h = lshr i64 %p1, 32
  %p2h = lshr i64 %p2, 32
  %midh = lshr i64 %mid, 32
  %h0 = add i64 %p3, %p1h
  %h1 = add i64 %h0, %p2h
  %h2 = add i64 %h1, %midh
  %x1 = mul i64 %al, %bh
  %x2 = mul i64 %ah, %bl
  %h3 = add i64 %h2, %x1
  %high = add i64 %h3, %x2
  store i64 %low, ptr %r
  %hp = getelementptr i8, ptr %r, i32 8
  store i64 %high, ptr %hp
  ret void
}
";

/// The entry: `nazm_main`, exported — `main`'s result, or `0` after handing the failure's text to
/// the host's `report`.
#[must_use]
pub fn emit_entry(entry: &str, roots: usize) -> String {
    let mut out = String::new();
    preamble(&mut out, "<nazm WebAssembly entry>");
    let root_types = vec!["i64"; roots].join(", ");
    let root_args = vec!["i64 0"; roots].join(", ");
    let _ = writeln!(out, "declare i64 {entry}({root_types})");
    let _ = writeln!(out, "@nz.failed = external global i1");
    let _ = writeln!(out, "@nz.errmsg = external global ptr");
    let _ = writeln!(out, "@nz.errlen = external global i64");
    let _ = writeln!(out, "declare void @nz.host_report(ptr, i32) #0");
    let _ = writeln!(out, "define i64 @nazm_main() #1 {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %result = call i64 {entry}({root_args})");
    let _ = writeln!(out, "  %failed = load i1, ptr @nz.failed");
    let _ = writeln!(out, "  br i1 %failed, label %reported, label %finished");
    let _ = writeln!(out, "reported:");
    let _ = writeln!(out, "  %m = load ptr, ptr @nz.errmsg");
    let _ = writeln!(out, "  %n = load i64, ptr @nz.errlen");
    let _ = writeln!(out, "  %n32 = trunc i64 %n to i32");
    let _ = writeln!(out, "  call void @nz.host_report(ptr %m, i32 %n32)");
    let _ = writeln!(out, "  ret i64 0");
    let _ = writeln!(out, "finished:");
    let _ = writeln!(out, "  ret i64 %result");
    let _ = writeln!(out, "}}");
    let _ = writeln!(
        out,
        "attributes #0 = {{ \"wasm-import-module\"=\"{HOST_MODULE}\" \"wasm-import-name\"=\"report\" }}"
    );
    let _ = writeln!(
        out,
        "attributes #1 = {{ \"wasm-export-name\"=\"nazm_main\" }}"
    );
    out
}

/// The reference host for Node (`DIR/host.mjs`): it provides exactly the imports it knows, refuses
/// a module that asks for any other, prints `main`'s result, and exits 0, or 2 after a failure.
pub const HOST: &str = r#"// Written by nazm build for wasm32-unknown-unknown (N64, docs/architecture.md §7.66).
// The reference host: `node host.mjs module.wasm`. It grants only what the module imports and
// it knows — `nazm_host.write` (standard output and error) and `nazm_host.report` (a failure).
import { readFileSync } from 'node:fs';
const module = new WebAssembly.Module(readFileSync(process.argv[2] ?? 'module.wasm'));
const known = new Set(['write', 'report']);
for (const i of WebAssembly.Module.imports(module)) {
  if (i.module !== 'nazm_host' || !known.has(i.name) || i.kind !== 'function') {
    process.stderr.write(`nazm host: the module asks for ${i.module}.${i.name}, which this host does not grant\n`);
    process.exit(3);
  }
}
let memory;
let failed = false;
const bytes = (p, n) => Buffer.from(memory.buffer, p, n);
const host = {
  write: (fd, p, n) => { (fd === 2 ? process.stderr : process.stdout).write(bytes(p, n)); },
  report: (p, n) => { failed = true; process.stderr.write(bytes(p, n)); },
};
const instance = new WebAssembly.Instance(module, { nazm_host: host });
memory = instance.exports.memory;
const result = instance.exports.nazm_main();
if (failed) process.exit(2);
process.stdout.write(`${result}\n`);
"#;
