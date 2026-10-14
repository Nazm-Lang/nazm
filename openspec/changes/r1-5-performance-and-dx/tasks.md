## 1. Build time
- [ ] 1.1 Precompiled runtime object per target, cached standard-library objects
- [ ] 1.2 The comparison reports cached and uncached builds

## 2. Running
- [ ] 2.1 `nazm run` compiles with Cranelift and executes; `--interpret` keeps the interpreter

## 3. Word frequency: speed and memory
- [ ] 3.1 Size-class allocator in the runtime
- [ ] 3.2 Native string-hash built-in with the same values
- [ ] 3.3 Allocation-free `split`/`words` iterator
- [ ] 3.4 Whole-program optimisation of runtime and program
- [ ] 3.5 Streaming `file_lines`

## 4. Size and lines
- [ ] 4.1 Dead-stripping; unused standard functions left out
- [ ] 4.2 Prelude imports of the common standard modules
- [ ] 4.3 `top(n)` and `sort_by(key)` on maps
- [ ] 4.4 Expression-bodied functions

## 5. The comparison
- [ ] 5.1 Safety-equivalent rows and a safety column; re-run and publish
