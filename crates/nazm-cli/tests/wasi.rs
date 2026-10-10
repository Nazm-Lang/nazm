//! Gate 3: WebAssembly with WASI preview 1, `wasm32-wasip1` (`docs/spec.md`, *WASI*;
//! `docs/systems-domains.md` Part O).
//!
//! Without a container, on any host: the target builds its objects, the `wasm-ld` line that links
//! them and Node's WASI host, and refuses by name what WASI preview 1 has not got — tasks,
//! channels, the network. With Docker (the ignored tests, in `nazm-wasm:n64`, which has `wasm-ld`
//! and Node's `node:wasi`): a module's heap, arguments, standard streams, files beneath its
//! preopened directory, a seek, metadata, the clocks and a sleep, the same at `-O0` and `-O2`; a
//! path outside the preopen refused; a failure reported with status 2; and every import WASI's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");
const WASI: &str = "wasm32-wasip1";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn tour() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wasi/tour.nz")
}

/// `src`'s objects for WASI at `opt`, in `dir/out`.
fn objects(dir: &Path, src: &Path, opt: &str) -> Output {
    nazm(
        dir,
        &[
            "build",
            "--target",
            WASI,
            "--opt-level",
            opt,
            "--objects",
            "out",
            src.to_str().unwrap(),
        ],
    )
}

#[test]
fn the_wasi_target_writes_its_objects_link_line_and_host() {
    let d = tempfile::tempdir().unwrap();
    let o = objects(d.path(), &tour(), "0");
    assert!(o.status.success(), "{}", err(&o));
    let link = std::fs::read_to_string(d.path().join("out/link.txt")).unwrap();
    assert!(link.contains("target wasm32-wasip1"), "{link}");
    assert!(
        link.contains("wasm-ld --stack-first -z stack-size=1048576 --export=_start"),
        "{link}"
    );
    let host = std::fs::read_to_string(d.path().join("out/wasi.mjs")).unwrap();
    assert!(
        host.contains("node:wasi") && host.contains("preopens"),
        "{host}"
    );
    // Linked by its host's toolchain, never here.
    let o = nazm(
        d.path(),
        &["build", "--target", WASI, tour().to_str().unwrap()],
    );
    assert!(!o.status.success());
    assert!(err(&o).contains("--objects DIR"), "{}", err(&o));
}

#[test]
fn what_wasi_preview_1_has_not_got_is_refused_by_name() {
    for (src, part) in [
        (
            "fn work(n: Int) -> Int ! {} { n }\n\
             fn main(t: SpawnCap) -> Int ! { spawn } { scope { spawn work(1); } 0 }\n",
            "tasks",
        ),
        (
            "fn main() -> Int ! {} { let c = chan_new_of[Int](1); chan_send_of(c, 1); 0 }\n",
            "channels",
        ),
        (
            "use \"@std/net\";\nuse \"@std/ioerror\";\n\
             fn main(net: NetCap) -> Int ! { io } { match tcp_listen(net, \"127.0.0.1:0\") { Result.Ok(value: l) => 1, Result.Err(error: e) => 0, } }\n",
            "networking",
        ),
    ] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), src).unwrap();
        let o = objects(d.path(), &d.path().join("prog.nz"), "0");
        assert_eq!(o.status.code(), Some(1), "{part}: {}", err(&o));
        let e = err(&o);
        assert!(e.contains("is WASI preview 1, one thread and no sockets"), "{e}");
        assert!(e.contains(part), "{part}: {e}");
        assert!(!d.path().join("out").exists(), "{part}: objects written");
    }
}

/// Every import of a WebAssembly module, as `(module, name)`.
fn imports(bytes: &[u8]) -> Vec<(String, String)> {
    assert_eq!(&bytes[..4], b"\0asm", "not WebAssembly");
    let leb = |b: &[u8], at: &mut usize| -> usize {
        let (mut n, mut shift) = (0usize, 0);
        loop {
            let byte = b[*at];
            *at += 1;
            n |= usize::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return n;
            }
        }
    };
    let name = |b: &[u8], at: &mut usize| -> String {
        let n = leb(b, at);
        let s = String::from_utf8(b[*at..*at + n].to_vec()).unwrap();
        *at += n;
        s
    };
    let mut at = 8;
    let mut out = Vec::new();
    while at < bytes.len() {
        let id = bytes[at];
        at += 1;
        let size = leb(bytes, &mut at);
        let end = at + size;
        if id == 2 {
            let mut p = at;
            for _ in 0..leb(bytes, &mut p) {
                let module = name(bytes, &mut p);
                let field = name(bytes, &mut p);
                let kind = bytes[p];
                p += 1;
                // A function import's type index; nothing else is imported here.
                assert_eq!(kind, 0, "{module}.{field} is not a function");
                leb(bytes, &mut p);
                out.push((module, field));
            }
        }
        at = end;
    }
    out
}

/// Link `dir/out` in `nazm-wasm:n64` and run it under Node's WASI with `dir/root` preopened and
/// `args`: its standard output, standard error and status.
fn link_and_run(dir: &Path, args: &[&str]) -> (String, String, i32) {
    std::fs::create_dir_all(dir.join("root")).unwrap();
    let script = format!(
        "cd /w/out && wasm-ld --stack-first -z stack-size=1048576 --export=_start -o module.wasm \
         $(sed -n 's/^objects //p' link.txt) && node --no-warnings wasi.mjs module.wasm /w/root {}",
        args.join(" ")
    );
    let o = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", dir.display()))
        .args(["nazm-wasm:n64", "sh", "-c", &script])
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

const TOUR: &str = "args 3\nbuilt 215\nwritten under WASI\nseek 456\nsize 10\nclock ok\n\
outside refused\nmissing refused\n0\n";

/// Needs Docker and `nazm-wasm:n64`: `cargo test -p nazm-cli --test wasi -- --ignored`.
#[test]
#[ignore = "starts a container"]
fn a_wasi_module_has_a_heap_streams_arguments_files_and_clocks() {
    for opt in ["0", "2"] {
        let d = tempfile::tempdir().unwrap();
        let o = objects(d.path(), &tour(), opt);
        assert!(o.status.success(), "-O{opt}: {}", err(&o));
        let (out, e, status) = link_and_run(d.path(), &["x", "y", "z"]);
        assert_eq!(status, 0, "-O{opt}: {e}");
        assert_eq!(out, TOUR, "-O{opt}: {e}");
        // The files it wrote are beneath its preopened directory, and nowhere else.
        assert_eq!(
            std::fs::read_to_string(d.path().join("root/note.txt")).unwrap(),
            "written under WASI\n"
        );
        assert_eq!(
            std::fs::read(d.path().join("root/data.bin")).unwrap(),
            b"0123456789"
        );
        // Every import is WASI's.
        let module = std::fs::read(d.path().join("out/module.wasm")).unwrap();
        let imported = imports(&module);
        assert!(!imported.is_empty());
        assert!(
            imported.iter().all(|(m, _)| m == "wasi_snapshot_preview1"),
            "{imported:?}"
        );
        for needed in ["fd_write", "args_get", "path_open", "clock_time_get"] {
            assert!(imported.iter().any(|(_, f)| f == needed), "{needed}");
        }
    }
}

#[test]
#[ignore = "starts a container"]
fn a_wasi_failure_is_reported_with_status_2_and_a_sleep_waits() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "use \"@std/io\";\nuse \"@std/time\";\n\
         fn main(io: IoCap, t: TimeCap) -> Int ! { io } {\n\
             let a = time_ms(t);\n\
             time_sleep(t, time_duration_ms(120));\n\
             let waited = time_ms(t) - a;\n\
             io_println(io, if waited >= 120 { \"slept\" } else { int_to_str(waited) });\n\
             let v = vec_new[Int]();\n\
             vec_get(v, args_count())\n\
         }\n",
    )
    .unwrap();
    let o = objects(d.path(), &d.path().join("prog.nz"), "0");
    assert!(o.status.success(), "{}", err(&o));
    let (out, e, status) = link_and_run(d.path(), &[]);
    assert_eq!(out, "slept\n", "{e}");
    assert_eq!(status, 2, "{out}{e}");
    assert!(e.contains("nazm: error[N0405]"), "{e}");
}
