# syntax=docker/dockerfile:1
# deploy/lab/Dockerfile — the one image every P2P lab container runs (scripts/lab/lab.sh builds
# it). The same image plays a device (txtodod + txtodo), a NAT router and the iroh relay, so a run
# builds once. See tasks/p2p-lab/notes.md.
#
# Multi-stage builds: https://docs.docker.com/build/building/multi-stage/
# Cache mounts keep cargo's registry and target dir between builds, so a rebuild only recompiles
# the crates that changed: https://docs.docker.com/build/cache/optimize/#use-cache-mounts
# `sharing=locked` makes a second concurrent build wait instead of corrupting the shared target:
# https://docs.docker.com/reference/dockerfile/#run---mounttypecache

ARG RUST_IMAGE=rust:1.95-bookworm

# The relay the lab's devices use instead of n0's public one. `--dev` serves plain HTTP (no TLS),
# and iroh's client turns TLS off for an http:// relay URL. The binary needs the `server` feature:
# https://docs.rs/crate/iroh-relay/latest/features. The runner passes the version from Cargo.lock,
# so the lab relay speaks exactly what txtodod speaks.
FROM ${RUST_IMAGE} AS relay-build
ARG IROH_RELAY_VERSION
RUN --mount=type=cache,id=txtodo-lab-registry,target=/usr/local/cargo/registry,sharing=locked \
    cargo install iroh-relay --version "=${IROH_RELAY_VERSION}" --features server --locked --root /out

FROM ${RUST_IMAGE} AS build
WORKDIR /src
# .cargo/config.toml (zig linkers) is kept out of the context; this pins an x86_64 host to its own
# cc the same way .github/workflows/ci.yml does. Ignored on arm64.
# https://doc.rust-lang.org/cargo/reference/config.html#targettriplelinker
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc \
    CARGO_TARGET_DIR=/cargo-target
COPY . .
# Release, like what ships. `--locked`: the lab tests Cargo.lock, never a fresh resolve.
RUN --mount=type=cache,id=txtodo-lab-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=txtodo-lab-target,target=/cargo-target,sharing=locked \
    cargo build --release --locked -p txtodo-daemon -p txtodo-cli \
 && mkdir -p /out \
 && cp /cargo-target/release/txtodod /cargo-target/release/txtodo /out/

FROM debian:bookworm-slim AS device
# iproute2: routes and `tc netem` (delay/loss). iptables: the routers' NAT.
# libfaketime: per-device clock skew via LD_PRELOAD (glibc build, so the preload takes).
# https://github.com/wolfcw/libfaketime
RUN apt-get update \
 && apt-get install -y --no-install-recommends iproute2 iptables iputils-ping procps libfaketime \
 && rm -rf /var/lib/apt/lists/* \
 && ln -s /usr/lib/*/faketime/libfaketime.so.1 /usr/local/lib/libfaketime.so.1
COPY --from=relay-build /out/bin/iroh-relay /usr/local/bin/
COPY --from=build /out/ /usr/local/bin/
COPY deploy/lab/entrypoint.sh /usr/local/bin/
COPY deploy/lab/iroh-relay.toml /etc/iroh-relay.toml
ENV HOME=/lab
WORKDIR /lab
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
