# {{name}}

A WebAssembly module. `nazm run .` runs it here; `nazm test tests` replays the tests; `nazm build
--target wasm32-unknown-unknown . --objects build` writes the objects, `link.txt` (its `wasm-ld`
line) and `host.mjs`, the reference host granting exactly the module's imports.
