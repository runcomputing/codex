#!/usr/bin/env bash
#
# extras/release-linux.sh — produce the linux x86_64 (musl) codex binary from a
# Mac, with no linux box. It runs the very same extras/release.sh inside a
# linux/amd64 ubuntu container (Rosetta-accelerated on Apple Silicon), so the
# build logic has a single source of truth and the musl target is native to the
# container. Publishes codex-x86_64-unknown-linux-musl to the <tag>-patched
# release on the fork.
#
#   extras/release-linux.sh rust-v0.142.2
#
# The builder image (rust + zig + gh + the musl apt deps) and named Docker
# volumes for Cargo/release caches are reused across runs. Requires Docker
# running and an authenticated gh.

set -euo pipefail

IMAGE="codex-musl-builder:ubuntu24"
CARGO_HOME_VOLUME="codex-musl-cargo-home"
CARGO_TARGET_VOLUME="codex-musl-cargo-target"
RELEASE_CACHE_VOLUME="codex-musl-release-cache"
TAG="${1:-}"
[ -n "$TAG" ] || { echo "usage: extras/release-linux.sh <upstream-release-tag>   (e.g. rust-v0.142.2)" >&2; exit 1; }
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"

command -v docker >/dev/null 2>&1 || { echo "docker is required" >&2; exit 1; }
docker info >/dev/null 2>&1 || { echo "the docker daemon is not running" >&2; exit 1; }
command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1 || { echo "gh must be authenticated (gh auth login)" >&2; exit 1; }
TOKEN="$(gh auth token)"

echo "==> build/refresh the amd64 builder image (cached after first run)"
docker build --platform linux/amd64 -t "$IMAGE" - <<'DOCKER'
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
# build-essential + the exact apt deps install-musl-build-tools.sh expects, so
# its own apt-get install is a no-op at run time. python3 on 24.04 is 3.12, so
# rusty_v8_bazel.py's tomllib import works (the thing that broke CI on 22.04).
RUN apt-get update && apt-get install -y --no-install-recommends \
      curl git ca-certificates xz-utils sudo pkg-config make build-essential \
      python3 musl-tools libcap-dev g++ clang libc++-dev libc++abi-dev lld \
 && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg -o /usr/share/keyrings/githubcli-archive-keyring.gpg \
 && echo "deb [signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" > /etc/apt/sources.list.d/github-cli.list \
 && apt-get update && apt-get install -y gh && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://ziglang.org/download/0.14.0/zig-linux-x86_64-0.14.0.tar.xz | tar -xJ -C /opt \
 && ln -s /opt/zig-linux-x86_64-0.14.0/zig /usr/local/bin/zig
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
      --default-toolchain 1.95.0 --target x86_64-unknown-linux-musl
ENV PATH=/root/.cargo/bin:$PATH
DOCKER

echo "==> run extras/release.sh $TAG inside the container"
# The repo is mounted at /work; release.sh checks the tag out into a /tmp worktree
# (container-local, fast) and builds there. Cargo registry/git data, release
# helper downloads, and build output are kept in named Docker volumes so reruns
# do not start cold. The <tag>-patched tag should already exist, so no git push
# happens in here — only gh release upload (uses GH_TOKEN). Full parallelism:
# cargo uses all the VM's cores. The VM has enough RAM for the peak; bump the
# Docker memory if a big crate ever OOMs.
docker run --rm --platform linux/amd64 \
  -v "$ROOT":/work -w /work \
  -v "$CARGO_HOME_VOLUME":/cargo-home \
  -v "$CARGO_TARGET_VOLUME":/cargo-target \
  -v "$RELEASE_CACHE_VOLUME":/release-cache \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/cargo-target \
  -e CODEX_RELEASE_CACHE_DIR=/release-cache \
  -e GH_TOKEN="$TOKEN" -e CARGO_TERM_COLOR=always \
  "$IMAGE" bash -lc '
    set -e
    git config --global --add safe.directory "*"
    git worktree prune
    exec extras/release.sh '"$TAG"'
  '
