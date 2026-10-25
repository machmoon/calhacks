# syntax=docker/dockerfile:1.7
#
# Headless `wright` for bench / eval / sweep shards on Azure Container
# Instances (docs/aci.md, scripts/aci-fanout.sh). Build from the repo root:
#
#     docker build -f docker/Dockerfile -t wright:bench .                    # bench image (default target)
#     docker build -f docker/Dockerfile --target solvers -t wright:solvers . # + Python solvers, CalculiX
#
# Layering follows SWE-bench's harness (swebench/harness/docker_build.py:
# a base image, an environment image on top, then the per-run layer), here
# as stages: `builder` (Rust toolchain, never shipped), `payload` (the repo
# files the binary reads at run time), `bench` (debian-slim + binary, no
# solvers: the bench's 11 scenarios use the scripted toy domain only) and
# `solvers` (bench + the Python solver venv + CalculiX, for the evals and
# the rocket propellant sweep). The cache mounts are Docker's own Rust guide
# pattern (docs.docker.com/guides/rust/develop: `--mount=type=cache` for
# /usr/local/cargo/registry and target/, then copy the binary out).
#
# Solvers deliberately NOT in either image (each stage that needs one skips
# with its own "not run" message, which is a recorded outcome, never a pass):
#   FreeCAD     ~1 GB+, and CAD is owned by Astra (docs/cad-handoff.md)
#   SU2         no Debian package of v8.5; a source build is ~30 min (CFD stage)
#   PrusaSlicer GUI AppImage (GTK + FUSE); slicing stage skipped
#   AeroSandbox ~400 MB, an audit-only cross-check
#   usd-core, build123d/OCP  CAD-only
#   ffmpeg, wgpu GPU drivers renders/films are optional stages
#
# The binary resolves data through env!("CARGO_MANIFEST_DIR") (data/,
# bench/scenarios, evals/*, crates/*/py|sim|tools), so the payload keeps the
# repo layout at the same absolute path it was built at: /wright.

ARG RUST_VERSION=1.98
ARG DEBIAN=bookworm

# ---------------------------------------------------------------- builder
FROM rust:${RUST_VERSION}-${DEBIAN} AS builder
WORKDIR /wright
COPY . .
# winit refuses to compile on Linux without a windowing backend, and
# wright-desktop (egui, linked into the CLI) turns eframe's default features
# off. x11 is dlopen'ed at run time, so a headless container never opens it.
# Applied here, not in the shared Cargo.toml/Cargo.lock: on macOS it would
# change the lock for every session building in the tree.
RUN printf '\n[target.%s.dependencies]\neframe = { version = "0.37", default-features = false, features = ["x11"] }\n' \
      "'cfg(target_os = \"linux\")'" >> crates/wright-desktop/Cargo.toml
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/wright/target \
    set -eux; \
    cargo build --release -p wright-cli --bin wright; \
    cargo build --release -p wright-rocket --example rocket_matrix --example rocket_eval; \
    cargo build --release -p wright-balloon --example eval; \
    cargo build --release -p wright-air --example vtol_eval; \
    mkdir -p /out; \
    cp target/release/wright /out/; \
    cp target/release/examples/rocket_matrix target/release/examples/rocket_eval /out/; \
    cp target/release/examples/eval /out/balloon_eval; \
    cp target/release/examples/vtol_eval /out/

# ---------------------------------------------------------------- payload
FROM debian:${DEBIAN}-slim AS payload
COPY Cargo.toml /wright/Cargo.toml
COPY data /wright/data
COPY bench/scenarios /wright/bench/scenarios
COPY evals /wright/evals
COPY crates /wright/crates
COPY scripts/env.sh /wright/scripts/env.sh

# ------------------------------------------------------------------ bench
FROM debian:${DEBIAN}-slim AS bench
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/*
COPY --from=payload /wright /wright
COPY --from=builder /out/ /usr/local/bin/
ENV WRIGHT_DATA_DIR=/wright/data \
    WRIGHT_NO_ELICIT=1 \
    RUST_BACKTRACE=1
# Azure Files (or a Blob-backed volume) is mounted here by aci-fanout.sh.
VOLUME ["/results"]
WORKDIR /wright
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["wright", "bench", "-o", "/results/bench/results.json"]

# ---------------------------------------------------------------- solvers
# Versions from docs/TODO.md "Downloads log" (what the local evals ran on),
# except CalculiX: Debian bookworm ships 2.20, the local runs used 2.23
# (conda-forge). The X/GL libraries are what the Linux gmsh wheel links.
FROM python:3.12-slim-${DEBIAN} AS solvers
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tini calculix-ccx \
      libglu1-mesa libxrender1 libxcursor1 libxft2 libxinerama1 \
 && rm -rf /var/lib/apt/lists/*
RUN python -m venv /opt/venv \
 && /opt/venv/bin/pip install --no-cache-dir \
      numpy matplotlib jsbsim==1.3.3 optvl==2.5.0 rocketpy==1.13.0 cantera==3.2.0 gmsh==4.15.2
COPY --from=payload /wright /wright
COPY --from=builder /out/ /usr/local/bin/
ENV WRIGHT_DATA_DIR=/wright/data \
    WRIGHT_PYTHON=/opt/venv/bin/python \
    WRIGHT_FEA_PYTHON=/opt/venv/bin/python \
    WRIGHT_CCX=/usr/bin/ccx \
    WRIGHT_NO_ELICIT=1 \
    MPLBACKEND=Agg \
    RUST_BACKTRACE=1
VOLUME ["/results"]
WORKDIR /wright
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["wright", "bench", "-o", "/results/bench/results.json"]
