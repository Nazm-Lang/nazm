# Reference applications (Gate 2, `docs/general-purpose.md` §24)

Five programs doing ordinary work, each held by `crates/nazm-cli/tests/apps.rs`: run under the
interpreter and built by LLVM at `-O0` and `-O2` and by Cranelift, with real arguments, files and
standard input, every one held to the output recorded beside it and to its exit status.

| app | what it is | what it exercises | held to |
| --- | --- | --- | --- |
| `nwc/` | a `wc`: lines, words and bytes, `-l -w -c`, files or standard input, a `total` line, a missing file reported and the rest counted | arguments, `@std/file`, standard input, byte scanning, exit status | `files.expected`, `flags.expected`, `stdin.expected`; status 1 and the error for a missing file |
| `jsonpipe/` | a JSON-lines filter and aggregator: records kept by status, totalled per user, sorted, a summary; every bad line named by file and line | `@std/json`, `@std/hashmap`, `@std/fs`, sorting | `all.expected`, `errors.expected`; three rejected lines on standard error |
| `nbody/` | the Jovian planets, a symplectic Euler step, energy before and after | `Float64`, `float_sqrt`, `Vec[Float64]` | `nbody.expected`: −0.169075164 and −0.169087605 after 1,000 steps, the benchmark's published values, in all four implementations |
| `kvstore/` | a persistent log-structured store: appended and synced records, replay on open, compaction by atomic replacement | `@std/file` append, sync and `file_replace_atomic`, a second process reading what the first wrote | `script.expected`, `again.expected`; the compacted log's exact bytes and no file left beside it |
| `kvd/` | a concurrent TCP key-value server: one task owns the table, one task per connection asks it over channels; a demonstration with four clients at once and a fifth reading what they left | `@std/net`, `scope`/`spawn`, `Chan[Request]` carrying a reply channel, the task pool | `kvd.expected`, whatever order the clients finish in |

Gate 3 adds two, held by their own tests (`docs/systems-domains.md` Parts M and N):

| app | what it is | held by |
| --- | --- | --- |
| `pagestore/` | a page-structured store: fixed checksummed pages behind a redo log of whole page images, one owner task serving readers and a writer through channels, a lock for one process | `storage.rs`: a run crashed at every write boundary, each recovery checked against the acknowledged writes |
| `ledger/` | two processes on `@std/dist`: a ledger server that logs each credit before it answers, and a client that retries with backoff and an idempotency key | `distributed.rs`: the server stopped after logging a credit and before answering, restarted, every credit applied once |

## Resources (2026-10-09)

One run each of the native executables on an Apple M1 Pro (macOS, aarch64), wall time and peak
resident memory from `/usr/bin/time -l`. A measurement of this machine at this moment, not a
comparison with any other language or a claim filed in `docs/performance.md`.

| workload | LLVM `-O2` | Cranelift |
| --- | --- | --- |
| `nwc` over 200,000 lines (10.8 MB) | 0.36 s, 25.2 MB | 0.44 s, 25.2 MB |
| `jsonpipe` over 100,000 records (4.6 MB) | 0.83 s, 12.8 MB | 1.29 s, 12.8 MB |
| `nbody 1000000` | 1.00 s, 1.8 MB | 1.52 s, 1.8 MB |
| `kvstore` running `script.txt` (five synced appends and one atomic replacement) | 0.34 s, 1.9 MB | 0.35 s, 2.0 MB |
| `kvd` demonstration (five connections, 65 requests) | 0.35 s, 2.2 MB | 0.37 s, 2.3 MB |

`nwc` reads a whole file into one buffer, so its memory follows the file's size. `kvstore`'s time is
its `fsync`s. Both are choices of the applications, recorded rather than tuned.
