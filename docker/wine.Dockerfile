# The Wine image for the Windows target (Gate 3, docs/systems-domains.md Part A-W):
#
#   docker build --platform linux/amd64 -t nazm-wine:g3 -f docker/wine.Dockerfile docker
#
# MinGW-w64's gcc links `nazm build --target x86_64-pc-windows-gnu --objects DIR`'s objects as
# link.txt says (its libgcc and import libraries are MinGW-w64's own; nothing of Microsoft's SDK or
# CRT is downloaded), and wine64 runs the executable. What runs here is labelled *run-verified
# under Wine*: it is evidence from Wine in a Linux container, never from a Windows host.
FROM debian:bookworm-slim
RUN dpkg --add-architecture i386 && apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    wine64 wine mingw-w64 ca-certificates \
 && rm -rf /var/lib/apt/lists/*
ENV WINEARCH=win64 WINEDEBUG=-all WINEPREFIX=/tmp/wineprefix
# The prefix once, at build time, so a run does not spend its first seconds making it.
RUN /usr/lib/wine/wine64 cmd /c exit 0 >/dev/null 2>&1 || true
WORKDIR /w
