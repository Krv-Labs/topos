# syntax=docker/dockerfile:1
#
# Topos MCP server — containerized build for Glama releases.
#
# The package is a maturin `bin` wheel: the self-contained Rust `topos-mcp`
# stdio server (topos/mcp), with all computation in topos-engine and the
# Sighthound SAST engine compiled in — no Python runtime. The builder stage
# carries the Rust toolchain + maturin and produces the wheel; the runtime
# stage installs it and adds Node.js + GitNexus so the COMPOSABLE pillar works
# in-container.
#
# Transport is stdio. Launch the server with the `topos-mcp` entrypoint.

# ---- Builder: compile the topos-mcp binary into a wheel ---------------------
FROM python:3.12-slim AS builder

ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential curl cmake libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable \
    && pip install "maturin>=1.5,<2.0"

WORKDIR /src
COPY . .

# maturin reads the version from Cargo.toml; build a release `bin` wheel
# (the topos-mcp server binary) into /wheels.
RUN maturin build --release --bindings bin \
        --manifest-path topos/mcp/Cargo.toml --out /wheels

# ---- Runtime: install the wheel + Node/GitNexus -----------------------------
FROM python:3.12-slim AS runtime

ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# Node.js 20 (NodeSource) for the GitNexus CLI, which powers COMPOSABLE scoring.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates git \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g gitnexus@1.6.8 \
    && apt-get purge -y --auto-remove \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /wheels /wheels
RUN pip install /wheels/*.whl && rm -rf /wheels

# Trusted file-access root. Mount the repository to evaluate at /workspace, or
# override with TOPOS_MCP_FILE_ROOT. Topos refuses to read files outside it.
ENV TOPOS_MCP_FILE_ROOT=/workspace
WORKDIR /workspace

# stdio MCP server.
ENTRYPOINT ["topos-mcp"]
