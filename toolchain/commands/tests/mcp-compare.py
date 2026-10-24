#!/usr/bin/env python3
"""One session against `nazm-mcp` and the port, compared, for tests/run.sh's `mcp` part.

    mcp-compare.py protocol REF PORT REPO [ARGS...]    the scripted session of tests/mcp/protocol.txt, the
                                                       documentation sections of the index it gets, then ARGS
    mcp-compare.py agent REF PORT REPO FILE         the agent tools of slice 9: `nazm.semantic_context` at the first offsets of FILE (and the
                                                    argument errors), `nazm.repository_map`, `nazm.task_context` (seeds read from the map)
    mcp-compare.py agent-gen REF PORT REPO FILE PREFIX   writes PREFIX.in (the messages) and PREFIX.ref (the reference's answers)
    mcp-compare.py agent-cmp PREFIX PORT REPO            compares PREFIX.got (the port's raw standard output) with PREFIX.ref
    mcp-compare.py file REF PORT REPO FILE [ARGS...]   the tools asked of FILE as the root: the snapshot, the
                                                       diagnostics, the check, `nazm.semantic_delta` against
                                                       the snapshot the reference gave and against two baselines
                                                       made wrong, the first three diagnostics in full, a rename
                                                       at six names and a fix at three diagnostics

Prints `agree`, `refused: WHY` or `differ: WHERE` and, for a difference, the two lines on the lines after. The
reference runs the session first; its answers (an id, a state, a snapshot) shape what the port is asked.
Environment: NAZM_REF the reference's `nazm` (for the positions), NAZM_PORT the port's `nazm` (the
program the port's server asks), NAZM_LIBRARY, KEEP a directory to leave a difference's two answers in.
"""
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def session(program, root, extra, messages, repo, env):
    data = ''.join(json.dumps(m, separators=(',', ':')) + '\n' for m in messages).encode()
    p = subprocess.run([sys.executable, '-I', HERE + '/mcp-session.py', program, '--root', root] + extra,
                       input=data, capture_output=True, cwd=repo, env=env, timeout=600)
    lines = p.stdout.decode('utf-8', 'replace').split('\n')
    return [l for l in lines if l], p.stderr.decode('utf-8', 'replace')


def session_patient(program, root, extra, messages, repo, env):
    """`session`, but standard input stays open until every request has been answered. The reference serves requests concurrently and,
    on the end of its input, drops the answers still being worked on (a repository map takes seconds): a session of fast calls does not
    see it, a session with `nazm.repository_map` and `nazm.task_context` does, differently at each run. The messages are written one at
    a time; the answers are read as they come."""
    import threading
    want = {m['id'] for m in messages if 'id' in m}
    p = subprocess.Popen([program, '--root', root] + extra, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=repo, env=env)
    lines = []
    seen = set()
    done = threading.Event()

    def pump():
        for raw in p.stdout:
            l = raw.decode('utf-8', 'replace').rstrip('\n')
            if not l:
                continue
            lines.append(l)
            try:
                k = json.loads(l).get('id')
            except Exception:
                k = None
            if k in want:
                seen.add(k)
            if seen >= want:
                done.set()
        done.set()

    t = threading.Thread(target=pump)
    t.start()
    errs = []
    te = threading.Thread(target=lambda: errs.append(p.stderr.read()))
    te.start()
    for m in messages:
        p.stdin.write((json.dumps(m, separators=(',', ':')) + '\n').encode())
        p.stdin.flush()
    done.wait(1800)
    p.stdin.close()
    p.wait(timeout=600)
    t.join()
    te.join()
    rows = []
    for n, l in enumerate(lines):
        try:
            key = json.loads(l).get('id')
        except Exception:
            key = None
        rows.append(((0, key, n) if isinstance(key, int) else (1, 0, n), l))
    rows.sort(key=lambda r: r[0])
    out = [r[1] for r in rows]
    out.append('exit %d' % p.returncode)
    if p.returncode != 0:
        out.append('stderr: ' + errs[0].decode('utf-8', 'replace').strip())
    return out, ''


class Ids:
    def __init__(self):
        self.n = 1

    def req(self, method, params=None):
        self.n += 1
        m = {"jsonrpc": "2.0", "id": self.n, "method": method}
        if params is not None:
            m["params"] = params
        return m

    def call(self, name, arguments):
        return self.req("tools/call", {"name": name, "arguments": arguments})


def start():
    return [{"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"}]


def result_of(lines, id_):
    for l in lines:
        try:
            m = json.loads(l)
        except Exception:
            continue
        if isinstance(m, dict) and m.get('id') == id_:
            return m
    return None


def compare(ref_out, port_out, port_err, label):
    if 'exit 6' in port_out:
        why = ''
        for l in port_out:
            if l.startswith('stderr: '):
                why = l[len('stderr: '):].split('\n')[0]
        return 'refused: ' + why[:150].replace('nazm-mcp: not carried: ', '')
    if ref_out == port_out:
        return 'agree'
    for i in range(max(len(ref_out), len(port_out))):
        a = ref_out[i] if i < len(ref_out) else '<none>'
        b = port_out[i] if i < len(port_out) else '<none>'
        if a != b:
            k = 0
            while k < min(len(a), len(b)) and a[k] == b[k]:
                k += 1
            keep = os.environ.get('KEEP')
            if keep:
                os.makedirs(keep, exist_ok=True)
                tag = label.replace('/', '_')
                open('%s/mcp-%s.want' % (keep, tag), 'w').write('\n'.join(ref_out) + '\n')
                open('%s/mcp-%s.got' % (keep, tag), 'w').write('\n'.join(port_out) + '\n')
            return 'differ: line %d at %d\n  ref  %s\n  port %s' % (i, k, a[max(0, k - 60):k + 160], b[max(0, k - 60):k + 160])
    return 'agree'


def run_both(ref, port, root, extra, messages, repo, env, label):
    r, _ = session(ref, root, extra, messages, repo, env)
    p, perr = session(port, root, extra, messages, repo, env)
    return compare(r, p, perr, label)


def protocol(ref, port, repo, extra, env):
    ids = Ids()
    msgs = start()
    for line in open(HERE + '/mcp/protocol.txt'):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        m = eval(line)
        msgs.append(ids.req(m[0], m[1]))
    # The documentation: the index, then three of its sections by id with its state.
    probe = start() + [{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "nazm.docs_index", "arguments": {}}}]
    r, _ = session(ref, 'examples/max.nz', extra, probe, repo, env)
    idx = result_of(r, 2)
    if idx and 'result' in idx:
        sc = idx['result']['structuredContent']
        state = sc.get('state')
        docs = sc.get('documents', [])
        for d in docs[:3]:
            msgs.append(ids.call('nazm.docs_index', {'document': d['id']}))
            for s in d.get('sections', [])[:2]:
                msgs.append(ids.call('nazm.docs_section', {'id': s['id'], 'state': state}))
        if docs:
            msgs.append(ids.call('nazm.docs_section', {'id': docs[0]['sections'][0]['id'], 'state': 'stale'}))
    return run_both(ref, port, 'examples/max.nz', extra, msgs, repo, env, 'protocol')


def positions(repo, env, f):
    ref_nazm = env['NAZM_REF']
    check = subprocess.run([ref_nazm, 'check', '--json', '--no-cache', f], capture_output=True, cwd=repo, env=env)
    p = subprocess.run([sys.executable, '-I', HERE + '/patch-positions.py', repo + '/' + f], input=check.stderr,
                       capture_output=True, cwd=repo, env=env)
    out = []
    for l in p.stdout.decode('utf-8', 'replace').split('\n'):
        w = l.split(' ')
        if len(w) >= 2:
            out.append(w)
    return out


def file_session(ref, port, repo, f, extra, env):
    ids = Ids()
    probe = start() + [ids.call('nazm.semantic_snapshot', {}), ids.call('nazm.diagnostics', {})]
    r, _ = session(ref, f, extra, probe, repo, env)
    snap = result_of(r, 2)
    diag = result_of(r, 3)
    ids = Ids()
    msgs = start()
    msgs.append(ids.call('nazm.semantic_snapshot', {}))
    msgs.append(ids.call('nazm.diagnostics', {}))
    msgs.append(ids.call('nazm.command_summary', {'operation': 'check'}))
    if snap and 'result' in snap:
        sc = snap['result']['structuredContent']
        if sc.get('status') == 'available':
            msgs.append(ids.call('nazm.semantic_delta', {'baseline': sc}))
            defs = sc.get('definitions', [])
            if defs:
                t = json.loads(json.dumps(sc))
                digests = t['definitions'][0]['digests']
                named = [k for k in sorted(digests) if isinstance(digests[k], str)]
                if named:
                    digests[named[0]] = 'f' + digests[named[0]][1:]
                    msgs.append(ids.call('nazm.semantic_delta', {'baseline': t}))
                t = json.loads(json.dumps(sc))
                t['definitions'][0]['identity'] = t['definitions'][0]['identity'].replace('::fn ', '::fn zz_')
                msgs.append(ids.call('nazm.semantic_delta', {'baseline': t}))
    msgs.append(ids.call('nazm.semantic_delta', {'baseline': {}}))
    if diag and 'result' in diag:
        sc = diag['result']['structuredContent']
        if sc.get('status') == 'available':
            state = sc.get('state', {}).get('source')
            for d in sc.get('diagnostics', [])[:3]:
                msgs.append(ids.call('nazm.diagnostic_detail', {'id': d['id'], 'state': state}))
    verdicts = [run_both(ref, port, f, extra, msgs, repo, env, f)]
    # The patches in a session of their own: a patch the port refuses must not hide what it answers of the rest.
    msgs = start()
    ids = Ids()
    renames = fixes = 0
    for w in positions(repo, env, f):
        if w[0] == 'rename' and renames < 6 and len(w) >= 3:
            renames += 1
            msgs.append(ids.call('nazm.semantic_patch', {'operation': 'rename', 'file': os.path.basename(f), 'byte_offset': int(w[1]), 'new_name': w[2]}))
        if w[0] == 'fix' and fixes < 3:
            fixes += 1
            msgs.append(ids.call('nazm.semantic_patch', {'operation': 'diagnostic_fix', 'file': os.path.basename(f), 'byte_offset': int(w[1]), 'fix': 0}))
    if len(msgs) > 2:
        verdicts.append(run_both(ref, port, f, extra, msgs, repo, env, f + '.patch'))
    for v in verdicts:
        if v.startswith('differ'):
            return v
    for v in verdicts:
        if v.startswith('refused'):
            return v
    return 'agree'


def agent_messages(ref, repo, f, extra, env):
    ids = Ids()
    msgs = start()
    out = subprocess.run([sys.executable, '-I', HERE + '/context-positions.py', repo + '/' + f, '2' if os.environ.get('AGENT_MCP_LITE') else '4'], capture_output=True).stdout.decode().split()
    for b in out:
        msgs.append(ids.call('nazm.semantic_context', {'file': os.path.basename(f), 'byte_offset': int(b)}))
    msgs.append(ids.call('nazm.semantic_context', {'file': os.path.abspath(repo + '/' + f), 'byte_offset': 3}))
    for bad in [{}, {'file': 'x.nz'}, {'file': 1, 'byte_offset': 1}, {'file': 'x.nz', 'byte_offset': -1}, {'file': 'x.nz', 'byte_offset': 4294967296},
                {'file': 'x.nz', 'byte_offset': 1, 'z': 2}, {'file': 'nope.nz', 'byte_offset': 1},
                # Rust's float and Unicode `Debug` wording in `serde`'s errors.
                {'file': 1.5, 'byte_offset': 1}, {'file': 1e3, 'byte_offset': 1}, {'file': 0.00001, 'byte_offset': 1}, {'file': 1.5e300, 'byte_offset': 1},
                {'file': 1e16, 'byte_offset': 1}, {'file': -2.5, 'byte_offset': 1}, {'file': 18446744073709551616, 'byte_offset': 1},
                {'file': 'x.nz', 'byte_offset': 'caf\u00e9'}, {'file': 'x.nz', 'byte_offset': 'e\u0301\U0001F600\u00a0\u200b'},
                {'file': 'x.nz', 'byte_offset': 'tab\there \"q\" \\ \x01 \x7f'}, {'file': 'x.nz', 'byte_offset': 1.5}, {'file': 'x.nz', 'byte_offset': -1.0}]:
        msgs.append(ids.call('nazm.semantic_context', bad))
    for bad in [{}, {'operation': 'rename'}, {'operation': 'nope'}, {'operation': 1.5}, {'operation': 'caf\u00e9'},
                {'operation': 'rename', 'file': 'x.nz', 'byte_offset': 1, 'new_name': 'y', 'z': 1}, {'operation': 'rename', 'file': 'x.nz', 'byte_offset': 1.5, 'new_name': 'y'},
                {'operation': 'diagnostic_fix', 'file': 'x.nz', 'byte_offset': 1, 'fix': 'e\u0301'}, {'operation': 'diagnostic_fix', 'file': 'x.nz', 'byte_offset': 1},
                {'operation': 'rename', 'file': os.path.basename(f), 'byte_offset': 0, 'new_name': 'zz'}, {'operation': 'diagnostic_fix', 'file': os.path.basename(f), 'byte_offset': 0, 'fix': 0}]:
        msgs.append(ids.call('nazm.semantic_patch', bad))
    msgs.append(ids.call('nazm.repository_map', {}))
    msgs.append(ids.call('nazm.repository_map', {'x': 1}))
    probe = start() + [{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "nazm.repository_map", "arguments": {}}}]
    r, _ = session(ref, f, extra, probe, repo, env)
    m = result_of(r, 2)
    if m and 'result' in m:
        sc = m['result']['structuredContent']
        state = sc['state']
        ents = sc['entities']
        pick = [e['id'] for e in ents if e['kind'] in ('package', 'sources', 'root', 'module', 'document', 'profile')][::(40 if os.environ.get('AGENT_MCP_LITE') else 6)]
        pick += [e['id'] for e in ents if e['kind'] == 'definition'][::(700 if os.environ.get('AGENT_MCP_LITE') else 150)]
        for s in pick:
            msgs.append(ids.call('nazm.task_context', {'seeds': [s], 'state': state}))
        for intent in (('edit',) if os.environ.get('AGENT_MCP_LITE') else ('understand', 'edit', 'diagnose', 'test')):
            msgs.append(ids.call('nazm.task_context', {'seeds': pick[:2], 'state': state, 'intent': intent, 'max_entities': 4}))
        msgs.append(ids.call('nazm.task_context', {'seeds': pick[:1], 'state': 'stale'}))
        msgs.append(ids.call('nazm.task_context', {'seeds': [], 'state': state}))
    for bad in [{}, {'seeds': ['a']}, {'seeds': 'a', 'state': 's'}, {'seeds': [1], 'state': 's'}, {'seeds': ['a'], 'state': 5}, {'seeds': ['a'], 'state': 's', 'intent': 'bogus'},
                {'seeds': ['a'], 'state': 's', 'intent': 3}, {'seeds': ['a'], 'state': 's', 'max_entities': -1}, {'seeds': ['a'], 'state': 's', 'max_entities': 'x'},
                {'seeds': ['a'], 'state': 's', 'max_entities': 1.5}, {'seeds': ['a'], 'state': 's', 'zz': 1}, {'seeds': None, 'state': 's'}]:
        msgs.append(ids.call('nazm.task_context', bad))
    return msgs


def nodocs_messages():
    ids = Ids()
    return start() + [ids.call('nazm.docs_index', {}), ids.call('nazm.docs_index', {'document': 'spec'}), ids.call('nazm.docs_section', {'id': 'nope', 'state': 's'}),
                      ids.call('nazm.repository_map', {}), ids.call('nazm.task_context', {'seeds': ['x'], 'state': 's'})]


def nodocs_gen(ref, repo, f, prefix, env):
    """`nodocs`'s messages and the reference's answers (started with no `--docs`), for a port run elsewhere; PREFIX.nodocs marks it."""
    msgs = nodocs_messages()
    open(prefix + '.in', 'w').write(''.join(json.dumps(m, separators=(',', ':')) + '\n' for m in msgs))
    r, _ = session_patient(ref, f, [], msgs, repo, env)
    open(prefix + '.ref', 'w').write('\n'.join(r) + '\n')
    open(prefix + '.nodocs', 'w').write('')


def nodocs(ref, port, repo, f, env):
    """The documentation and repository tools of a server started with no `--docs`: the repository the binary was built from."""
    msgs = nodocs_messages()
    r, _ = session_patient(ref, f, [], msgs, repo, env)
    p, perr = session_patient(port, f, [], msgs, repo, env)
    return compare(r, p, perr, 'nodocs')


def agent(ref, port, repo, f, extra, env):
    return run_both(ref, port, f, extra, agent_messages(ref, repo, f, extra, env), repo, env, f + '.agent')


def agent_gen(ref, repo, f, prefix, extra, env):
    """The messages of `agent`, and the reference's answers to them, written for a port run elsewhere (a container has no Python)."""
    msgs = agent_messages(ref, repo, f, extra, env)
    open(prefix + '.in', 'w').write(''.join(json.dumps(m, separators=(',', ':')) + '\n' for m in msgs))
    r, _ = session_patient(ref, f, extra, msgs, repo, env)
    open(prefix + '.ref', 'w').write('\n'.join(r) + '\n')


def agent_cmp(prefix):
    rows = []
    for n, line in enumerate(open(prefix + '.got').read().split('\n')):
        if not line:
            continue
        try:
            key = json.loads(line).get('id')
        except Exception:
            key = None
        rows.append(((0, key, n) if isinstance(key, int) else (1, 0, n), line))
    rows.sort(key=lambda r: r[0])
    got = [r[1] for r in rows] + ['exit 0']
    want = [l for l in open(prefix + '.ref').read().split('\n') if l]
    return compare(want, got, '', os.path.basename(prefix))


def main():
    mode = sys.argv[1]
    ref, port, repo = sys.argv[2], sys.argv[3], sys.argv[4]
    env = dict(os.environ)
    env['NAZM_EXE'] = env.get('NAZM_PORT', '')
    if mode == 'agent':
        print(agent(ref, port, repo, sys.argv[5], ['--docs', repo], env))
    elif mode == 'agent-gen':
        agent_gen(ref, repo, sys.argv[5], sys.argv[6], ['--docs', repo], env)
    elif mode == 'agent-cmp':
        print(agent_cmp(ref))
    elif mode == 'nodocs-gen':
        nodocs_gen(ref, repo, sys.argv[5], sys.argv[6], env)
    elif mode == 'nodocs':
        print(nodocs(ref, port, repo, sys.argv[5], env))
    elif mode == 'protocol':
        print(protocol(ref, port, repo, ['--docs', repo] + sys.argv[5:], env))
    else:
        print(file_session(ref, port, repo, sys.argv[5], ['--docs', repo] + sys.argv[6:], env))


main()
