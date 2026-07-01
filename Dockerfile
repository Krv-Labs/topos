# syntax=docker/dockerfile:1
#
# Topos MCP server — containerized build for Glama releases.
#
# The package is a maturin hybrid: a Rust pyo3 extension (topos-functors) plus
# a Python package. The builder stage carries the Rust toolchain + maturin and
# produces a self-contained wheel; the runtime stage installs that wheel and
# adds Node.js + GitNexus so the COMPOSABLE pillar works in-container.
#
# Transport is stdio. Launch the server with the `topos-mcp` entrypoint.

# ---- Builder: compile the pyo3 extension into a wheel -----------------------
FROM python:3.12-slim AS builder

ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential curl \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable \
    && pip install "maturin>=1.5,<2.0"

WORKDIR /src
COPY . .

# maturin reads the version from Cargo.toml; build a release wheel into /wheels.
RUN maturin build --release --features pyo3/extension-module --out /wheels

# ---- Runtime: install the wheel + Node/GitNexus -----------------------------
FROM python:3.12-slim AS runtime

ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# Node.js 20 (NodeSource) for the GitNexus CLI, which powers COMPOSABLE scoring.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates git \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g gitnexus \
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
