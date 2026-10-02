# The image for N64's WebAssembly target: `docker build -t nazm-wasm:n64 -f docker/wasm.Dockerfile docker`.
# wasm-ld links a module from `nazm build --target wasm32-unknown-unknown --objects DIR`; Node's
# WebAssembly engine runs it under the reference host `DIR/host.mjs` writes.
FROM nazm-qemu:n62
RUN apt-get update && apt-get install -y --no-install-recommends nodejs && rm -rf /var/lib/apt/lists/*
