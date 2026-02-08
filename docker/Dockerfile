# lv multi-target builder
# Targets: linux-x86_64, linux-aarch64, windows-x86_64
#
# Usage (via scripts/docker-build.sh):
#   docker build -t lv-builder docker/
#   docker run --rm -v $PWD:/src lv-builder <target>
#
# Base: debian-slim (glibc required for AppImage, multiarch for arm64 cross)
FROM debian:bookworm-slim AS builder

ENV DEBIAN_FRONTEND=noninteractive

# ── Native x86_64 build deps ─────────────────────────────────────────
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    file \
    git \
    pkg-config \
    libsdl2-dev \
    libmpv-dev \
    # Windows cross-compile
    llvm \
    clang \
    nsis \
    && rm -rf /var/lib/apt/lists/*

# ── arm64 cross-compile deps (multiarch) ─────────────────────────────
RUN dpkg --add-architecture arm64 \
    && apt-get update && apt-get install -y --no-install-recommends \
    gcc-aarch64-linux-gnu \
    g++-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    libsdl2-dev:arm64 \
    libmpv-dev:arm64 \
    && rm -rf /var/lib/apt/lists/*

# ── Rust ──────────────────────────────────────────────────────────────
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable --profile minimal \
    -t x86_64-unknown-linux-gnu \
    -t aarch64-unknown-linux-gnu \
    -t x86_64-pc-windows-msvc
ENV PATH="/root/.cargo/bin:${PATH}"

# cargo-xwin for Windows MSVC cross-compilation
RUN cargo install cargo-xwin --locked

# ── Cargo config for arm64 cross ──────────────────────────────────────
RUN mkdir -p /root/.cargo && printf '\
[target.aarch64-unknown-linux-gnu]\n\
linker = "aarch64-linux-gnu-gcc"\n\
' >> /root/.cargo/config.toml

# ── Entrypoint ────────────────────────────────────────────────────────
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

WORKDIR /src
ENTRYPOINT ["/entrypoint.sh"]
