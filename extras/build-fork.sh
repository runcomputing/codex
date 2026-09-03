#!/usr/bin/env bash
# Build the runcomputing codex fork from an isolated, cached source tree.
#
# The real repo is never touched: this clones it locally (fast, but dissociated so
# the clone owns its objects) into a build root, applies every extras/*.patch
# there, and builds.
# The clone and the cargo target dir persist between runs.
#
#   ./extras/build-fork.sh              # reset, patch, quick build
#   ./extras/build-fork.sh --release    # optimized build
#   ./extras/build-fork.sh --keep       # rebuild current source as-is (after fixing conflicts)
#   ./extras/build-fork.sh --fresh      # throw the build root away first
#   ./extras/build-fork.sh --base main  # base the source tree on another ref
#   ./extras/build-fork.sh --no-build   # patch only
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_ROOT="${CODEX_BUILD_ROOT:-$HOME/.cache/codex-fork-build}"
SRC="$BUILD_ROOT/src"
export CARGO_TARGET_DIR="$BUILD_ROOT/target"

BASE_REF="main"
PROFILE="dev-small"
DO_BUILD=1
KEEP=0
FRESH=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ $# -ge 2 && "$2" != --* ]] || { echo "missing value for --base" >&2; exit 2; }
      BASE_REF="$2"; shift 2 ;;
    --release) PROFILE="release"; shift ;;
    --debug) PROFILE="dev"; shift ;;
    --profile)
      [[ $# -ge 2 && "$2" != --* ]] || { echo "missing value for --profile" >&2; exit 2; }
      PROFILE="$2"; shift 2 ;;
    --no-build) DO_BUILD=0; shift ;;
    --keep) KEEP=1; shift ;;
    --fresh) FRESH=1; shift ;;
    -h|--help) sed -n '2,14p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

say() { printf '\n==> %s\n' "$*"; }

[[ $FRESH -eq 1 ]] && { say "removing $BUILD_ROOT"; rm -rf "$BUILD_ROOT"; }
mkdir -p "$BUILD_ROOT"

if [[ ! -d "$SRC/.git" ]]; then
  say "cloning $REPO -> $SRC"
  # --dissociate: keep the clone independent of $REPO, so a gc there cannot
  # prune objects this long-lived build tree still needs.
  git clone --shared --no-hardlinks --dissociate "$REPO" "$SRC"
fi

if [[ $KEEP -eq 0 ]]; then
  say "resetting source tree to $BASE_REF"
  git -C "$SRC" fetch --quiet origin "+refs/heads/*:refs/remotes/origin/*"
  # Clear the previous run's applied patches first; checkout refuses to move a dirty tree.
  git -C "$SRC" reset --quiet --hard
  git -C "$SRC" clean -qfdx
  git -C "$SRC" checkout --quiet --detach "$(git -C "$REPO" rev-parse "$BASE_REF")"

  shopt -s nullglob
  patches=("$REPO"/extras/*.patch)
  shopt -u nullglob
  [[ ${#patches[@]} -eq 0 ]] && { echo "no patches in $REPO/extras" >&2; exit 1; }

  failed=0
  for p in "${patches[@]}"; do
    say "applying $(basename "$p")"
    if git -C "$SRC" apply --3way "$p" 2>/dev/null; then
      echo "    clean (3-way)"
    elif git -C "$SRC" apply --reject "$p"; then
      echo "    applied"
    else
      echo "    applied with rejects" >&2
      failed=1
    fi
  done

  rejects=$(cd "$SRC" && find . -name '*.rej' -not -path './.git/*')
  if [[ -n "$rejects" ]]; then
    say "REJECTED HUNKS -- resolve these by hand, then rerun with --keep"
    printf '%s\n' "$rejects" | sed "s|^\./|  $SRC/|"
    [[ $DO_BUILD -eq 1 ]] && { echo; echo "not building while rejects remain" >&2; }
    exit 1
  fi
  [[ $failed -eq 0 ]] || exit 1
fi

if [[ $DO_BUILD -eq 1 ]]; then
  say "cargo build --profile $PROFILE -p codex-cli --bin codex"
  ( cd "$SRC/codex-rs" && cargo build --profile "$PROFILE" -p codex-cli --bin codex )

  out_dir="$CARGO_TARGET_DIR/$PROFILE"
  [[ "$PROFILE" == "dev" ]] && out_dir="$CARGO_TARGET_DIR/debug"
  say "binary: $out_dir/codex"
fi
