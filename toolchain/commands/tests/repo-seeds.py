#!/usr/bin/env python3
"""The command lines `agent.sh repo` asks of `nazm repo --task --root .`: seeds read from the reference's own map, one line each.

    repo-seeds.py REF REPO [EVERY]      REF the reference `nazm`, REPO the repository's root

The seeds are every package, source root, compilation root, module, document, schema and profile, a sample of the targets,
definitions, mutations, tests and sections, and every diagnostic of the repository's bad examples; each under the default intent,
and the first few of each kind under every intent. Then the budgets, the invalid requests and the stale state. EVERY (default 1)
thins the samples: every EVERYth entity of a large kind.
"""
import json
import os
import subprocess
import sys

ref, repo = sys.argv[1], sys.argv[2]
every = int(sys.argv[3]) if len(sys.argv) > 3 else 1


def run(*args):
    return subprocess.run([ref] + list(args), capture_output=True, cwd=repo).stdout.decode('utf-8')


m = json.loads(run('repo', '--map', '--json', '--root', '.'))
state = m['state']
by = {}
for e in m['entities']:
    by.setdefault(e['kind'], []).append(e['id'])


def pick(ids, step):
    step = max(1, step * every)
    return ids[::step]


def quote(s):
    return "'" + s.replace("'", "'\\''") + "'"


lines = []


def seed(s, intent=None, extra=''):
    t = '--seed %s' % quote(s)
    if intent:
        t += ' --intent %s' % intent
    lines.append((t + (' ' + extra if extra else '')).strip())


intents = ['understand', 'edit', 'diagnose', 'test']
for kind, step, deep in [('package', 1, 3), ('sources', 1, 3), ('root', 1, 3), ('module', 1, 3), ('document', 1, 3), ('schema', 1, 1),
                         ('profile', 1, 3), ('target', 4, 2), ('definition', 12, 6)]:
    ids = pick(by.get(kind, []), step)
    for i, s in enumerate(ids):
        seed(s)
        if i < deep:
            for intent in intents:
                seed(s, intent)

# Sections: a few of the documents' sections, by their ids in the documentation index.
idx = json.loads(run('docs', '--index', '--json', '--root', '.'))
sections = [s['id'] for d in idx['documents'] for s in d['sections'] if s['parent']]
for i, s in enumerate(pick(sections, 25)):
    seed('doc:' + s)
    if i < 4:
        for intent in intents:
            seed('doc:' + s, intent)

# The catalogue: mutations and tests.
try:
    import tomllib
    cat = tomllib.load(open(os.path.join(repo, 'xtask/mutations/mutations.toml'), 'rb'))
    names = [x['name'] for x in cat['mutation']]
    for i, n in enumerate(pick(names, 90)):
        seed('mutation:' + n)
        if i < 3:
            for intent in intents:
                seed('mutation:' + n, intent)
    pk = {}
    tests = []
    for x in cat['mutation']:
        for k in x.get('killers', []):
            tests.append((k['package'], 'lib' if k.get('lib') else ('test' if 'test' in k else 'bin'), k.get('test') or k.get('bin'), k['name']))
    for i, (pkg, kind, target, name) in enumerate(pick(tests, 700)):
        tname = target if target else pkg.replace('-', '_')
        seed('test:%s:%s:%s::%s' % (pkg, kind, tname, name))
        if i < 2:
            for intent in intents:
                seed('test:%s:%s:%s::%s' % (pkg, kind, tname, name), intent)
except ImportError:
    pass

# Diagnostics: every one of the bad examples.
bad = sorted(f for f in os.listdir(os.path.join(repo, 'examples/bad')) if f.endswith('.nz'))
for f in bad:
    d = json.loads(run('diagnostics', '--json', '--root', 'examples/bad/' + f, '--source-root', 'examples/bad') or '{}')
    for x in d.get('diagnostics', []):
        seed('diagnostic:examples-bad:%s:%s' % (f, x['id']))
        if f == 'return_type_mismatch.nz':
            for intent in intents:
                seed('diagnostic:examples-bad:%s:%s' % (f, x['id']), intent)

# Several seeds, budgets, the state, and what is invalid.
defs = by.get('definition', [])
mods = by.get('module', [])
pkgs = by.get('package', [])
lines.append('--seed %s --seed %s' % (quote(pkgs[0]), quote(pkgs[1])))
lines.append('--seed %s --seed %s --seed %s --intent understand' % (quote(defs[0]), quote(mods[0]), quote(pkgs[2])))
lines.append('--seed %s --max-entities 3' % quote(defs[5]))
lines.append('--seed %s --max-entities 1' % quote(defs[5]))
lines.append('--seed %s --max-entities 0' % quote(defs[5]))
lines.append('--seed %s --max-source-bytes 10' % quote(defs[5]))
lines.append('--seed %s --max-source-bytes 500 --max-doc-bytes 100' % quote(defs[5]))
lines.append('--seed %s --max-doc-bytes 100' % quote('package:nazm-docs'))
lines.append('--seed %s --max-doc-bytes 0' % quote('package:nazm-docs'))
lines.append('--seed %s --max-entities 5 --max-source-bytes 4000 --max-doc-bytes 4000' % quote(defs[100]))
lines.append('--seed %s --state %s' % (quote(pkgs[0]), state))
lines.append('--seed %s --state 0000' % quote(pkgs[0]))
lines.append('--seed nope')
lines.append('--seed package:')
lines.append('--seed package:nope')
lines.append('--seed unknown:thing')
lines.append('--seed %s' % quote('def:compiler:../x.nz::fn f'))
lines.append('--seed %s' % quote('module:compiler:/etc/passwd'))
lines.append('--seed %s' % quote('root:compiler:sub/emit.nz'))
lines.append('--seed %s' % quote('def:compiler:nope.nz::fn f'))
lines.append('--seed %s' % quote('def:compiler:lex.nz::fn nope'))
lines.append('--seed %s' % quote('doc:nope'))
lines.append('--seed %s --seed %s' % (quote(pkgs[0]), quote(pkgs[0])))
lines.append(' '.join('--seed %s' % quote('package:' + p.split(':')[1]) for p in pkgs[:9]))
lines.append('')
for l in lines:
    if l:
        print(l)
