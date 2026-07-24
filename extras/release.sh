#!/usr/bin/env bash
#
# extras/release.sh — build a patched codex binary for THIS host and publish it.
#
# Applies extras/codex-channel.patch onto an upstream stable release tag, builds
# the codex binary natively for the host platform, strips it, and uploads it to
# the <tag>-patched GitHub Release on this fork. We patch *release tags*, never
# main, and build natively per-host (a Mac makes the macOS binary in a few
# minutes; a Linux x86_64 box makes the static musl binary) — no cross-compiling.
#
# Also builds + ships codex-code-mode-host from the SAME patched worktree: codex
# resolves that binary as a sibling of its own executable and spawns it whenever
# a session uses code mode (code_mode_only models, MCP tools routed through code
# mode). Shipping codex without it produces "failed to spawn code-mode host" at
# runtime. Building both from one worktree keeps the codex<->host stdio protocol
# in lockstep even if a future patch ever touches the code-mode crates.
#
#   extras/release.sh rust-v0.142.2          # -> publishes to release rust-v0.142.2-patched
#
# Run it once on each platform you want to ship. Re-runnable: it --clobbers the
# host's asset and is a no-op on refs that already exist.

set -euo pipefail

REPO="runcomputing/codex"          # where releases + binaries live
UPSTREAM="openai/codex"            # where prebuilt rusty_v8 artifacts live
PATCH_RELPATH="extras/codex-channel.patch"

die()  { printf '\033[31mx %s\033[0m\n' "$*" >&2; exit 1; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }

UPSTREAM_TAG="${1:-}"
[ -n "$UPSTREAM_TAG" ] || die "usage: extras/release.sh <upstream-release-tag>   (e.g. rust-v0.142.2)"
PATCHED_TAG="${UPSTREAM_TAG}-patched"

# Refuse prereleases by default: a `<alpha>-patched` release would otherwise be
# created as a normal release, so devx's install.py (which skips only releases MARKED
# prerelease) and anyone hitting `releases/latest` would serve it. Set
# ALLOW_PRERELEASE=1 to build one anyway — it's then marked --prerelease so both
# consumers skip it.
PRERELEASE=0
case "$UPSTREAM_TAG" in
  *-alpha*|*-beta*|*-rc*|*-pre*|*-dev*|*-nightly*) PRERELEASE=1 ;;
esac
if [ "$PRERELEASE" = 1 ] && [ "${ALLOW_PRERELEASE:-0}" != "1" ]; then
  die "$UPSTREAM_TAG is a prerelease — refusing so installers can't serve it (devx install.py + releases/latest both skip only MARKED prereleases). We ship stable tags. Re-run with ALLOW_PRERELEASE=1 to build it anyway (marked --prerelease)."
fi

ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
PATCH="$ROOT/$PATCH_RELPATH"
[ -f "$PATCH" ] || die "missing $PATCH_RELPATH — run from a checkout of the fork's default branch"

# ---- host -> rust target + asset name ---------------------------------------
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64)   TARGET="aarch64-apple-darwin"        ; HOST_OS="darwin" ;;
  Linux/x86_64)   TARGET="x86_64-unknown-linux-musl"   ; HOST_OS="linux"  ;;
  Linux/aarch64)  TARGET="aarch64-unknown-linux-musl"  ; HOST_OS="linux"  ;;
  *) die "unsupported host $(uname -s)/$(uname -m) — we ship mac arm64 + linux x86_64 only" ;;
esac
ASSET="codex-$TARGET"
HOST_ASSET="codex-code-mode-host-$TARGET"
step "host $(uname -s)/$(uname -m) → $TARGET → $ASSET + $HOST_ASSET → release $PATCHED_TAG"

# ---- preflight --------------------------------------------------------------
for t in cargo rustup gh python3; do command -v "$t" >/dev/null 2>&1 || die "$t is required"; done
gh api user >/dev/null 2>&1 || die "gh is not authenticated — run: gh auth login"
[ "$HOST_OS" = "linux" ] && { command -v zig >/dev/null 2>&1 || die "zig 0.14.0 is required for the linux-musl toolchain (https://ziglang.org/download)"; }
rustup target add "$TARGET" >/dev/null 2>&1 || true

# ---- check out the upstream tag and apply our patch -------------------------
git -C "$ROOT" rev-parse -q --verify "refs/tags/$UPSTREAM_TAG" >/dev/null 2>&1 \
  || git -C "$ROOT" fetch --tags origin "$UPSTREAM_TAG" >/dev/null 2>&1 \
  || die "upstream tag $UPSTREAM_TAG not found locally — sync the mirror or 'git fetch --tags'"
WT="$(mktemp -d)/codex-$UPSTREAM_TAG"
git -C "$ROOT" worktree add -q --detach "$WT" "$UPSTREAM_TAG"
cleanup() { git -C "$ROOT" worktree remove --force "$WT" >/dev/null 2>&1 || true; }
trap cleanup EXIT
step "apply $PATCH_RELPATH onto $UPSTREAM_TAG"
( cd "$WT" && git apply "$PATCH" ) || die "patch did not apply onto $UPSTREAM_TAG — regenerate $PATCH_RELPATH (see extras/RELEASING.md)"
ok "patched worktree ready"

# ---- reuse the upstream CI scripts (they append KEY=VALUE to \$GITHUB_ENV) ----
export GITHUB_ENV; GITHUB_ENV="$(mktemp)"
export GITHUB_WORKSPACE="$WT"
export RUNNER_TEMP
if [ -n "${CODEX_RELEASE_CACHE_DIR:-}" ]; then
  RUNNER_TEMP="$CODEX_RELEASE_CACHE_DIR/runner-temp/$TARGET"
  mkdir -p "$RUNNER_TEMP"
else
  RUNNER_TEMP="$(mktemp -d)"
fi

step "fetch prebuilt rusty_v8 for $TARGET (downloaded, never compiled)"
V8_VERSION="$(python3 "$WT/.github/scripts/rusty_v8_bazel.py" resolved-v8-crate-version)"
V8_BASE="https://github.com/$UPSTREAM/releases/download/rusty-v8-v${V8_VERSION}"
V8_DIR="$RUNNER_TEMP/rusty_v8/$V8_VERSION"; mkdir -p "$V8_DIR"
if [ ! -f "$V8_DIR/librusty_v8_release_${TARGET}.a.gz" ]; then
  curl -fsSL "$V8_BASE/librusty_v8_release_${TARGET}.a.gz" -o "$V8_DIR/librusty_v8_release_${TARGET}.a.gz.tmp"
  mv "$V8_DIR/librusty_v8_release_${TARGET}.a.gz.tmp" "$V8_DIR/librusty_v8_release_${TARGET}.a.gz"
fi
if [ ! -f "$V8_DIR/src_binding_release_${TARGET}.rs" ]; then
  curl -fsSL "$V8_BASE/src_binding_release_${TARGET}.rs" -o "$V8_DIR/src_binding_release_${TARGET}.rs.tmp"
  mv "$V8_DIR/src_binding_release_${TARGET}.rs.tmp" "$V8_DIR/src_binding_release_${TARGET}.rs"
fi
if [ ! -f "$V8_DIR/rusty_v8_release_${TARGET}.sha256" ]; then
  curl -fsSL "$V8_BASE/rusty_v8_release_${TARGET}.sha256" -o "$V8_DIR/rusty_v8_release_${TARGET}.sha256.tmp"
  mv "$V8_DIR/rusty_v8_release_${TARGET}.sha256.tmp" "$V8_DIR/rusty_v8_release_${TARGET}.sha256"
fi
( cd "$V8_DIR" && { command -v sha256sum >/dev/null 2>&1 && sha256sum -c "rusty_v8_release_${TARGET}.sha256" || shasum -a 256 -c "rusty_v8_release_${TARGET}.sha256"; } )
{
  echo "RUSTY_V8_ARCHIVE=$V8_DIR/librusty_v8_release_${TARGET}.a.gz"
  echo "RUSTY_V8_SRC_BINDING_PATH=$V8_DIR/src_binding_release_${TARGET}.rs"
} >> "$GITHUB_ENV"
ok "rusty_v8 $V8_VERSION ready"

if [ "$HOST_OS" = "linux" ]; then
  step "set up musl cross toolchain (libcap from source, zig cc/cxx, BoringSSL sysroot)"
  TARGET="$TARGET" bash "$WT/.github/scripts/install-musl-build-tools.sh"
  echo "AWS_LC_SYS_NO_JITTER_ENTROPY=1" >> "$GITHUB_ENV"
  v="AWS_LC_SYS_NO_JITTER_ENTROPY_${TARGET//-/_}"; echo "${v}=1" >> "$GITHUB_ENV"
fi
# Load the GitHub-Actions-style env file. Do NOT `source` it: values may contain
# spaces (e.g. CMAKE_ARGS=-D.. -D..) and GitHub parses this format itself in CI,
# so a shell `source` would try to run the second flag as a command. Parse it as
# KEY=VALUE lines, honouring GitHub's KEY<<DELIM multiline blocks too.
while IFS= read -r __l || [ -n "$__l" ]; do
  case "$__l" in
    ''|'#'*) continue ;;
    *'<<'*)
      __k="${__l%%<<*}"; __d="${__l#*<<}"; __v=""
      while IFS= read -r __m && [ "$__m" != "$__d" ]; do __v="${__v}${__v:+$'\n'}${__m}"; done
      export "$__k=$__v" ;;
    *=*) export "${__l%%=*}=${__l#*=}" ;;
  esac
done < "$GITHUB_ENV"
[ "$HOST_OS" = "darwin" ] && export CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO="packed"

# ---- build + strip ----------------------------------------------------------
[ -d "$WT/codex-rs/code-mode-host" ] \
  || die "tag $UPSTREAM_TAG has no codex-rs/code-mode-host crate — codex of this era resolves that binary next to its own executable, so we refuse to ship codex without it"
step "cargo build --release (-p codex-cli --bin codex, -p codex-code-mode-host) for $TARGET"
( cd "$WT/codex-rs" && cargo build --release --target "$TARGET" -p codex-cli --bin codex -p codex-code-mode-host --bin codex-code-mode-host )
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  BIN="$CARGO_TARGET_DIR/$TARGET/release/codex"
  HOST_BIN="$CARGO_TARGET_DIR/$TARGET/release/codex-code-mode-host"
else
  BIN="$WT/codex-rs/target/$TARGET/release/codex"
  HOST_BIN="$WT/codex-rs/target/$TARGET/release/codex-code-mode-host"
fi
[ -f "$BIN" ] || die "build produced no binary at $BIN"
[ -f "$HOST_BIN" ] || die "build produced no binary at $HOST_BIN"
if [ "$HOST_OS" = "linux" ]; then
  strip --strip-debug --strip-unneeded "$BIN" "$HOST_BIN"
else
  for b in "$BIN" "$HOST_BIN"; do strip -S -x "$b" 2>/dev/null || strip -S "$b" 2>/dev/null || true; done
fi
ok "built + stripped → codex $(du -h "$BIN" | cut -f1), code-mode-host $(du -h "$HOST_BIN" | cut -f1)"

# ---- publish ----------------------------------------------------------------
github_api_bool() {
  if [ "$1" = "1" ]; then
    printf true
  else
    printf false
  fi
}

create_release() {
  gh api -X POST "repos/$REPO/releases" \
    -f tag_name="$PATCHED_TAG" \
    -f name="$PATCHED_TAG" \
    -f body="codex $UPSTREAM_TAG with the runcomputing talk-channel patch (extras/codex-channel.patch). Built per-platform via extras/release.sh." \
    -F prerelease="$(github_api_bool "$PRERELEASE")" >/dev/null
}

upload_release_asset() {
  local tag="$1"
  local asset_path="$2"
  local asset_name="$3"
  local release_id
  local asset_id
  local token

  release_id="$(gh api "repos/$REPO/releases/tags/$tag" --jq '.id')"
  asset_id="$(gh api "repos/$REPO/releases/tags/$tag" --jq ".assets[]? | select(.name == \"$asset_name\") | .id" 2>/dev/null || true)"
  if [ -n "$asset_id" ]; then
    gh api -X DELETE "repos/$REPO/releases/assets/$asset_id" >/dev/null
  fi

  token="$(gh auth token)"
  curl --fail-with-body --silent --show-error --retry 3 --retry-delay 5 \
    -X POST \
    -H "Authorization: Bearer $token" \
    -H "Accept: application/vnd.github+json" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    -H "Content-Type: application/octet-stream" \
    --data-binary "@$asset_path" \
    "https://uploads.github.com/repos/$REPO/releases/$release_id/assets?name=$asset_name" >/dev/null
}

step "publish $ASSET → $REPO release $PATCHED_TAG"
# Tag the patched tree once (idempotent); whichever platform runs first creates it.
REMOTE_TAG_EXISTS=0
if gh api "repos/$REPO/git/ref/tags/$PATCHED_TAG" >/dev/null 2>&1; then
  REMOTE_TAG_EXISTS=1
elif git -C "$ROOT" ls-remote --tags origin "refs/tags/$PATCHED_TAG" 2>/dev/null | grep -q .; then
  REMOTE_TAG_EXISTS=1
fi
if [ "$REMOTE_TAG_EXISTS" = 0 ]; then
  ( cd "$WT" && git add -A \
      && git -c user.email=release@runcomputing.dev -c user.name="runcomputing release" \
           commit -q -m "codex $UPSTREAM_TAG + runcomputing channel patch" \
      && git tag "$PATCHED_TAG" && git push -q origin "$PATCHED_TAG" )
  ok "pushed tag $PATCHED_TAG"
else
  ok "remote tag $PATCHED_TAG already exists"
fi
if ! gh api "repos/$REPO/releases/tags/$PATCHED_TAG" >/dev/null 2>&1; then
  create_release
  ok "created release $PATCHED_TAG"
fi
STAGE="$RUNNER_TEMP/$ASSET"; cp "$BIN" "$STAGE"; chmod +x "$STAGE"
upload_release_asset "$PATCHED_TAG" "$STAGE" "$ASSET"
ok "uploaded $ASSET"
HOST_STAGE="$RUNNER_TEMP/$HOST_ASSET"; cp "$HOST_BIN" "$HOST_STAGE"; chmod +x "$HOST_STAGE"
upload_release_asset "$PATCHED_TAG" "$HOST_STAGE" "$HOST_ASSET"
ok "uploaded $HOST_ASSET"

step "done → https://github.com/$REPO/releases/download/$PATCHED_TAG/$ASSET (+ $HOST_ASSET)"
