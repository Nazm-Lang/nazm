"""Write `toolchain/pipeline/rtext.nz` from the runtime crate (`crates/nazm-runtime/src`): every
text constant of the hosted runtime (`const NAME: &str = ...;`, as `rt_FILE_NAME()`), the
declarations every unit carries (`os::declarations`, `GLOBALS`, the declared `INVENTORY`), the
state a program of one thread keeps in plain globals (`SINGLE_THREAD_STATE`), the functions whose
reference counts it makes plain (`PLAIN_IN_ONE_THREAD`), and each inventory
entry as Rust's `Debug` prints it (what the runtime's digest covers). The texts are the runtime's
data; the rules that assemble them are ported by hand in `rtgen.nz`. Run from the repository root
whenever the runtime changes; `run.sh` checks the file is current."""
import re
import sys

SRC = "crates/nazm-runtime/src/"
FILES = ["core", "files", "net", "switch", "platform", "heap", "board"]


def literal(s, i):
    """The Rust string literal at `s[i]`: its value and the index after it."""
    if s[i] == "r":
        j = i + 1
        while s[j] == "#":
            j += 1
        hashes = s[i + 1:j]
        assert s[j] == '"', s[i:i + 40]
        end = s.index('"' + hashes, j + 1)
        return s[j + 1:end], end + 1 + len(hashes)
    assert s[i] == '"', s[i:i + 40]
    out = []
    j = i + 1
    while True:
        c = s[j]
        if c == '"':
            return "".join(out), j + 1
        if c == "\\":
            e = s[j + 1]
            if e == "\n":
                j += 2
                while s[j] in " \t\n\r":
                    j += 1
                continue
            simple = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}
            if e in simple:
                out.append(simple[e])
                j += 2
                continue
            if e == "x":
                out.append(chr(int(s[j + 2:j + 4], 16)))
                j += 4
                continue
            if e == "u":
                close = s.index("}", j)
                out.append(chr(int(s[j + 3:close], 16)))
                j = close + 1
                continue
            raise ValueError("escape " + e)
        out.append(c)
        j += 1


def raw(text):
    """`text` as a Nazm raw string."""
    n = 1
    while ('"' + "#" * n) in text:
        n += 1
    return "r" + "#" * n + '"' + text + '"' + "#" * n


def debug(v):
    """A `&str` as Rust's `Debug` writes it."""
    out = []
    for ch in v:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\0":
            out.append("\\0")
        elif ord(ch) < 0x20 or ord(ch) == 0x7F:
            out.append("\\u{%x}" % ord(ch))
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


fns = []


def add(name, text):
    fns.append("pub fn " + name + "() -> Str ! {} {\n    " + raw(text) + "\n}\n")


for f in FILES:
    s = open(SRC + f + ".rs").read()
    # Leave out the crate's tests.
    s = s.split("\n#[cfg(test)]")[0]
    for m in re.finditer(r"^(?:pub(?:\(crate\))? )?const ([A-Z][A-Z0-9_]*): &str =\s*", s, re.M):
        value, _ = literal(s, m.end())
        add("rt_" + f + "_" + m.group(1), value)

# The `os` unit's POSIX head, a format string whose holes `rtgen.nz` fills by name.
plat = open(SRC + "platform.rs").read()
head = plat[plat.index("pub fn emit_os"):]
head = head[head.index("write!(") :]
value, _ = literal(head, head.index('"'))
add("rt_platform_OS_FORMAT", value)

lib = open(SRC + "lib.rs").read()
os_src = open(SRC + "os.rs").read()
body = os_src[os_src.index("pub fn declarations"):]
body = body[:body.index("] {")]
decls = []
for m in re.finditer(r'^\s*"', body, re.M):
    v, _ = literal(body, m.end() - 1)
    decls.append(v)
add("rt_os_declarations", "\n".join(decls) + "\n")

glob = []
for m in re.finditer(r'RuntimeGlobal \{\s*symbol: "([^"]+)",\s*ty: "([^"]+)",\s*thread_local: (true|false),', lib):
    tl = "thread_local " if m.group(3) == "true" else ""
    glob.append("@" + m.group(1) + " = external " + tl + "global " + m.group(2))
add("rt_globals", "\n".join(glob) + "\n")

inv = []
dbg = []
for line in lib.splitlines():
    t = line.strip()
    if not t.startswith('rt!("'):
        continue
    i = t.index("(") + 1
    sym, i = literal(t, i)
    i += 2
    ret, i = literal(t, i)
    i = t.index("(", i) + 1
    params = []
    while t[i] != ")":
        if t[i] in ", ":
            i += 1
            continue
        p, i = literal(t, i)
        params.append(p)
    i += 2
    attrs, i = literal(t, i)
    rest = t[i:]
    m = re.match(r', (true|false), ([A-Za-z]+), ', rest)
    declared, part = m.group(1), m.group(2)
    i += m.end()
    eff, i = literal(t, i)
    m = re.match(r', ([A-Za-z]+), ([A-Za-z]+), ', t[i:])
    fail, safe = m.group(1), m.group(2)
    i += m.end()
    resp, i = literal(t, i)
    i += 2
    own, i = literal(t, i)
    assert t[i:] == "),", t
    if declared == "true":
        a = " " + attrs if attrs else ""
        inv.append("declare " + ret + " @" + sym + "(" + ", ".join(params) + ")" + a)
    dbg.append(
        "RuntimeSymbol { symbol: " + debug(sym) + ", ret: " + debug(ret) + ", params: ["
        + ", ".join(debug(p) for p in params) + "], attrs: " + debug(attrs) + ", declared: "
        + declared + ", part: " + part + ", responsibility: " + debug(resp) + ", ownership: "
        + debug(own) + ", effect: " + debug(eff) + ", failure: " + fail + ", safety: " + safe + " }")
add("rt_inventory_declarations", "\n".join(inv) + "\n")
add("rt_inventory_debug", "\n".join(dbg) + "\n")

m = re.search(r"pub const SINGLE_THREAD_STATE: &\[&str\] = &\[([^\]]*)\];", lib)
add("rt_single_thread_state", "\n".join(re.findall(r'"([^"]+)"', m.group(1))) + "\n")
core = open(SRC + "core.rs").read()
m = re.search(r"pub const PLAIN_IN_ONE_THREAD: \[&str; \d+\] = \[([^\]]*)\];", core)
add("rt_plain_in_one_thread", "\n".join(re.findall(r'"([^"]+)"', m.group(1))) + "\n")
m = re.search(r"pub const RUNTIME_ABI_REVISION: u32 = (\d+);", lib)
fns.append("pub fn rt_abi_revision() -> Int ! {} { " + m.group(1) + " }\n")
m = re.search(r'pub const TARGET_NOTE: &str = "', lib)
note, _ = literal(lib, m.end() - 1)
add("rt_target_note", note)

out = ("// Generated by `toolchain/pipeline/tests/runtime_text.py` from the runtime crate\n"
       "// (`crates/nazm-runtime/src`); do not edit. The hosted runtime's texts and tables, which\n"
       "// `rtgen.nz` assembles as the crate does.\n\n" + "\n".join(fns))
target = "toolchain/pipeline/rtext.nz"
if len(sys.argv) > 1 and sys.argv[1] == "--check":
    sys.exit(0 if open(target).read() == out else 1)
open(target, "w").write(out)
