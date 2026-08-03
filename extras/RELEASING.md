# Releasing the patched codex CLI — agent runbook

**Self-contained runbook.** Goal: build and publish the patched `codex` CLI (upstream
codex + local extras customizations) for a new **stable** upstream release. Every platform
ships **two** binaries — `codex` and its companion `codex-code-mode-host` — from the
same build. You can run this from anywhere — if you're not already in a
`runcomputing/codex` checkout, **start at step 0**; it gets you there.

The customizations live in a single patch file applied onto an upstream **release tag** at
build time — we never maintain a patched branch, and never patch `main`. It currently
carries MCP channel support, the custom status-line command, and related configuration
validation.

---

## 0. Get into the repo

```bash
# Already inside the fork's checkout? If not, clone it (blobless = fast).
git remote -v 2>/dev/null | grep -q 'runcomputing/codex' || {
  gh repo clone runcomputing/codex -- --filter=blob:none && cd codex
}
git fetch origin main --tags --prune
git switch main 2>/dev/null || git checkout main
git reset --hard origin/main   # main is rebased onto upstream daily (force-pushed)
```

You're now on `main` with `extras/` and all `rust-v*` tags. `gh repo clone` also wires
git auth so the later tag push works.

## 1. Check prerequisites

- **`gh auth status`** — must be authenticated with a token that can **write to
  `runcomputing/codex`** (publish releases + push tags). If not: `gh auth login`.
- **macOS build:** `cargo`, `rustup`, `python3` (3.11+) on PATH.
- **Linux build (container path):** Docker running with ~24 GB — check `docker info`.
  `extras/release-linux.sh` builds inside an amd64 ubuntu container; codex's big crates
  want the RAM. If a crate OOMs, raise Docker's memory and re-run.

## 2. Pick the latest STABLE tag

```bash
git tag --list 'rust-v*' | grep -vE '\-(alpha|beta|rc|pre|dev|nightly)' | sort -V | tail
```

Take the highest one — call it `<tag>` (e.g. `rust-v0.142.2`). `release.sh` refuses
prereleases anyway (devx's `install.py` only considers non-prerelease `-patched`
releases, and marking a deliberate prerelease `--prerelease` keeps it out of
`releases/latest` for anyone hitting that URL directly). **If nothing is newer than
what's already published, stop — there's nothing to build** (compare against
`gh release list -R runcomputing/codex`).

## 3. Build + publish both platforms

```bash
extras/release.sh        <tag>     # macOS arm64  → codex-aarch64-apple-darwin + codex-code-mode-host-aarch64-apple-darwin
extras/release-linux.sh  <tag>     # linux x86_64 → codex-x86_64-unknown-linux-musl + codex-code-mode-host-x86_64-unknown-linux-musl
```

Each platform ships **two** binaries: `codex` and `codex-code-mode-host`. Codex resolves
the host as a sibling of its own executable and spawns it whenever a session uses code
mode (code_mode_only models, MCP tools routed through code mode) — a release without it
fails at runtime with "failed to spawn code-mode host". Both are built from the same
patched worktree in one cargo invocation, so the codex↔host stdio protocol always
matches.

Each applies `extras/codex-channel.patch` onto `<tag>` in a throwaway worktree, builds,
strips, and uploads to the `<tag>-patched` release (created on the first run,
`--clobber`ed on re-runs). Run them in sequence. On a real linux x86_64 host you can run
`extras/release.sh <tag>` directly instead of the container (needs `zig` 0.14).

Expected wall time:

- macOS arm64: about 15–20 min (`rust-v0.142.4` took 16m 56s).
- Linux x86_64 container: about 25–35 min cold (`rust-v0.142.4` took 24m 01s with
  newly-created Docker cache volumes; a fully disposable run took 33m 44s before upload
  failed). Warm reruns should be faster because `release-linux.sh` keeps Cargo registry,
  Cargo git, build output, and release helper caches in named Docker volumes.
- (Those timings predate `codex-code-mode-host`; it shares codex's dependency tree and
  is built in the same cargo invocation, so it adds roughly one extra codegen+link step,
  not a second full build.)

The Linux wrapper reuses these Docker volumes across runs:

```bash
codex-musl-cargo-home
codex-musl-cargo-target
codex-musl-release-cache
```

If a cache ever looks corrupted, delete the volumes and rerun:

```bash
docker volume rm codex-musl-cargo-home codex-musl-cargo-target codex-musl-release-cache
```

To ship a prerelease on purpose: `ALLOW_PRERELEASE=1 extras/release.sh <tag>` — its
release is marked `--prerelease` so it stays out of `latest`.

### Waiting without spinning

Once Cargo starts, do not poll every few seconds. Let the command run for about five
minutes at a time, then check whether it has finished. Long quiet periods are normal near
the end of a release build:

- macOS can sit quietly in `dsymutil` while packaging debug info.
- Linux can sit quietly while final `rustc`/link work consumes many cores and 10+ GB RAM.

For macOS, after a five-minute wait, check whether Cargo or `dsymutil` is still active:

```bash
ps -o pid,ppid,etime,pcpu,pmem,comm -ax | rg 'cargo|rustc|dsymutil|ld'
```

For the Linux container path, after a five-minute wait, check the container once:

```bash
docker ps --filter ancestor=codex-musl-builder:ubuntu24
docker stats --no-stream $(docker ps --filter ancestor=codex-musl-builder:ubuntu24 -q)
```

If CPU and memory are moving, keep waiting in five-minute chunks. Only investigate more
deeply when the process exits nonzero, Docker reports no running builder container, or CPU
stays idle across repeated five-minute checks.

## 4. Verify

```bash
gh release view <tag>-patched -R runcomputing/codex --json assets --jq '.assets[].name'
# expect ALL FOUR:
#   codex-aarch64-apple-darwin            codex-code-mode-host-aarch64-apple-darwin
#   codex-x86_64-unknown-linux-musl       codex-code-mode-host-x86_64-unknown-linux-musl

gh api repos/runcomputing/codex/releases/latest --jq '.tag_name'
# expect: <tag>-patched
```

Optional runtime smoke (on a machine that installed via devx `install.py`) — force code
mode so codex MUST spawn the host binary:

```bash
~/.runcomputing/bin/codex exec --skip-git-repo-check -c 'features.code_mode_only=true' \
  'Run the shell command `echo host-ok` and report its exact output.'
# a "failed to spawn code-mode host" line means the release/install pair is broken
```

That's the whole job — devx's `install.py` picks the newest non-prerelease `-patched`
release carrying **all** the assets it needs (a partial release is skipped, not served),
so it hands out the new build automatically. **No devx change needed.**

---

## If the patch doesn't apply

`release.sh` aborts (before building) if upstream moved the patched code. Regenerate
the patch against the new tag, then re-run step 3:

```bash
REPO="$(git rev-parse --show-toplevel)"
git worktree add -d /tmp/cdx <tag>
cd /tmp/cdx
git apply --3way "$REPO/extras/codex-channel.patch"   # resolve any <<<< conflict markers
git diff -- codex-rs/ > "$REPO/extras/codex-channel.patch"
cd "$REPO" && git worktree remove --force /tmp/cdx
git add extras/codex-channel.patch
git commit -m "refresh codex-channel.patch for <tag>" && git push
```

## Layout / how it fits

| | |
| --- | --- |
| `main` (default) | upstream `main` **+** our `extras/` overlay as a single commit on top; the `mirror-upstream` workflow rebases that commit onto upstream's latest each day and copies new `rust-v*` tags. **No codex-rs edits.** |
| `extras/codex-channel.patch` | the only code change |
| `extras/release.sh` / `release-linux.sh` | build + publish both binaries, per platform |
| `extras/README.md` | Codex-desktop wiring (`CODEX_CLI_PATH`, the LaunchAgent plist) |

The patch currently carries MCP channel support, the custom status-line command, and
related configuration validation.

devx's `install.py` is a pure consumer: it downloads `codex` **and**
`codex-code-mode-host` from the same (newest complete, non-prerelease) `*-patched`
release into `~/.runcomputing/bin/` — codex spawns the host as a sibling of its own
executable, and the pair speaks a version-locked stdio protocol, so they must never come
from different releases. devx holds no codex source.
