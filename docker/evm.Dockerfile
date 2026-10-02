# The image for N70's EVM backend: `docker build -t nazm-evm:n70 -f docker/evm.Dockerfile docker`.
# py-evm is a reference EVM implementation in Python; a test harness runs `nazm build`'s bytecode
# in it, with chosen senders, and reads storage, return data and gas.
FROM python:3.12-slim
RUN pip install --no-cache-dir "py-evm==0.12.1b1" "eth-hash[pycryptodome]"
