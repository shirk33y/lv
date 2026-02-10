#!/bin/sh
# cargo-cache-clean.sh — Prune cargo cache mounts in Docker builds.
#
# Equivalent to Swatinem/rust-cache cleanup logic, rewritten in POSIX shell.
# Designed to run inside a Dockerfile after `cargo build` to keep
# --mount=type=cache volumes lean across builds.
#
# Usage:
#   cargo-cache-clean.sh [OPTIONS]
#
# Options:
#   --target DIR       target directory (default: /src/target)
#   --cargo-home DIR   cargo home (default: $CARGO_HOME or /root/.cargo)
#   --manifest DIR     directory containing Cargo.toml/Cargo.lock (default: /src)
#   --crate NAME       crate name to remove from target (default: read from Cargo.toml)
#   --dry-run          print what would be deleted without deleting
#
# What it does:
#   1. Removes own crate's build artifacts from target/ (deps stay cached)
#   2. Prunes registry: keeps only .crate files + -sys sources for actual deps
#   3. Prunes git checkouts: keeps only repos referenced by Cargo.lock
#   4. Cleans registry index .cache for sparse registries
set -eu

# ── Defaults ─────────────────────────────────────────────────────────
TARGET_DIR="/src/target"
CARGO_HOME="${CARGO_HOME:-/root/.cargo}"
MANIFEST_DIR="/src"
CRATE_NAME=""
DRY_RUN=false

while [ $# -gt 0 ]; do
  case "$1" in
    --target)     TARGET_DIR="$2";     shift 2 ;;
    --cargo-home) CARGO_HOME="$2";     shift 2 ;;
    --manifest)   MANIFEST_DIR="$2";   shift 2 ;;
    --crate)      CRATE_NAME="$2";     shift 2 ;;
    --dry-run)    DRY_RUN=true;        shift   ;;
    *)            echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# ── Helpers ──────────────────────────────────────────────────────────
log()    { echo "  [cache-clean] $*"; }
remove() {
  if [ "$DRY_RUN" = true ]; then
    log "(dry-run) would remove: $1"
  else
    rm -rf "$1"
  fi
}

# Get crate name from Cargo.toml if not specified
if [ -z "$CRATE_NAME" ]; then
  CRATE_NAME=$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$MANIFEST_DIR/Cargo.toml" | head -1)
fi
# Cargo normalizes hyphens to underscores in artifact filenames
CRATE_NAME_UNDER=$(echo "$CRATE_NAME" | tr '-' '_')

log "crate=$CRATE_NAME target=$TARGET_DIR cargo_home=$CARGO_HOME"

# ── Collect dependency package names from Cargo.lock ─────────────────
# We parse [[package]] entries: name and name-version pairs.
# This is the source of truth for what to keep.
DEP_NAMES=$(mktemp)
DEP_NAME_VERSIONS=$(mktemp)
DEP_SYS_NAME_VERSIONS=$(mktemp)
trap 'rm -f "$DEP_NAMES" "$DEP_NAME_VERSIONS" "$DEP_SYS_NAME_VERSIONS"' EXIT

if [ -f "$MANIFEST_DIR/Cargo.lock" ]; then
  # Extract name = "..." lines that follow [[package]] headers
  awk '
    /^\[\[package\]\]/ { in_pkg=1; name=""; version=""; next }
    in_pkg && /^name *= *"/ { gsub(/.*"/, "", $0); gsub(/".*/, "", $0); name=$0 }
    in_pkg && /^version *= *"/ { gsub(/.*"/, "", $0); gsub(/".*/, "", $0); version=$0 }
    in_pkg && /^$/ {
      if (name != "") {
        print name > NAMES
        print name "-" version > NV
        if (name ~ /-sys$/) print name "-" version > SYS
      }
      in_pkg=0
    }
    END {
      if (name != "") {
        print name > NAMES
        print name "-" version > NV
        if (name ~ /-sys$/) print name "-" version > SYS
      }
    }
  ' NAMES="$DEP_NAMES" NV="$DEP_NAME_VERSIONS" SYS="$DEP_SYS_NAME_VERSIONS" \
    "$MANIFEST_DIR/Cargo.lock"

  sort -u -o "$DEP_NAMES" "$DEP_NAMES"
  sort -u -o "$DEP_NAME_VERSIONS" "$DEP_NAME_VERSIONS"
  sort -u -o "$DEP_SYS_NAME_VERSIONS" "$DEP_SYS_NAME_VERSIONS"
else
  log "WARNING: no Cargo.lock found, skipping registry/git pruning"
fi

DEP_COUNT=$(wc -l < "$DEP_NAMES")
log "found $DEP_COUNT dependencies in Cargo.lock"

# ── 1. Clean target directory ────────────────────────────────────────
# Remove own crate's artifacts from all profile dirs (release, debug, etc.)
# Keep dependency artifacts (build/, .fingerprint/, deps/ entries for deps)
clean_target() {
  log "cleaning target directory..."
  REMOVED=0

  for profile_dir in "$TARGET_DIR"/*/; do
    [ -d "$profile_dir" ] || continue
    base=$(basename "$profile_dir")

    # Skip non-profile dirs (CACHEDIR.TAG, .rustc_info.json, etc.)
    case "$base" in
      .*|CACHEDIR.TAG) continue ;;
    esac

    # If it's a nested target dir (has CACHEDIR.TAG or .rustc_info.json), recurse
    if [ -f "$profile_dir/CACHEDIR.TAG" ] || [ -f "$profile_dir/.rustc_info.json" ]; then
      TARGET_DIR="$profile_dir" clean_target
      continue
    fi

    # Remove top-level files in profile dir (e.g. the final binary)
    for f in "$profile_dir"*; do
      [ -f "$f" ] || continue
      remove "$f"
      REMOVED=$((REMOVED + 1))
    done

    # Clean deps/ — remove own crate's .rlib/.d files
    if [ -d "$profile_dir/deps" ]; then
      for f in "$profile_dir/deps/$CRATE_NAME_UNDER"-* \
               "$profile_dir/deps/lib${CRATE_NAME_UNDER}"-*; do
        [ -e "$f" ] || continue
        remove "$f"
        REMOVED=$((REMOVED + 1))
      done
    fi

    # Clean .fingerprint/ — remove own crate's fingerprint
    if [ -d "$profile_dir/.fingerprint" ]; then
      for d in "$profile_dir/.fingerprint/$CRATE_NAME"-*; do
        [ -e "$d" ] || continue
        remove "$d"
        REMOVED=$((REMOVED + 1))
      done
    fi

    # Clean build/ — remove own crate's build script output
    if [ -d "$profile_dir/build" ]; then
      for d in "$profile_dir/build/$CRATE_NAME"-*; do
        [ -e "$d" ] || continue
        remove "$d"
        REMOVED=$((REMOVED + 1))
      done
    fi

    # Remove examples/, incremental/ (not useful in CI)
    for subdir in examples incremental; do
      if [ -d "$profile_dir/$subdir" ]; then
        remove "$profile_dir/$subdir"
        REMOVED=$((REMOVED + 1))
      fi
    done
  done

  log "target: removed $REMOVED items"
}

# ── 2. Clean registry ───────────────────────────────────────────────
# - registry/cache/: keep only .crate files for actual deps
# - registry/src/: keep only -sys crate sources (they check timestamps)
# - registry/index/: clean .cache for sparse registries
clean_registry() {
  log "cleaning cargo registry..."
  REMOVED=0

  REGISTRY="$CARGO_HOME/registry"
  [ -d "$REGISTRY" ] || return 0

  # registry/cache/ — keep only .crate files matching deps
  if [ -d "$REGISTRY/cache" ]; then
    for index_dir in "$REGISTRY/cache"/*/; do
      [ -d "$index_dir" ] || continue
      for crate_file in "$index_dir"*.crate; do
        [ -f "$crate_file" ] || continue
        base=$(basename "$crate_file" .crate)
        if ! grep -qFx "$base" "$DEP_NAME_VERSIONS" 2>/dev/null; then
          remove "$crate_file"
          REMOVED=$((REMOVED + 1))
        fi
      done
    done
  fi

  # registry/src/ — keep only -sys crate sources
  if [ -d "$REGISTRY/src" ]; then
    for index_dir in "$REGISTRY/src"/*/; do
      [ -d "$index_dir" ] || continue
      for src_dir in "$index_dir"*/; do
        [ -d "$src_dir" ] || continue
        base=$(basename "$src_dir")
        if ! grep -qFx "$base" "$DEP_SYS_NAME_VERSIONS" 2>/dev/null; then
          remove "$src_dir"
          REMOVED=$((REMOVED + 1))
        fi
      done
    done
  fi

  # registry/index/ — clean .cache dirs for sparse registries
  if [ -d "$REGISTRY/index" ]; then
    for index_dir in "$REGISTRY/index"/*/; do
      [ -d "$index_dir" ] || continue
      # Git-based registries: remove .cache (cargo recreates from .git)
      if [ -d "$index_dir/.git" ]; then
        if [ -d "$index_dir/.cache" ]; then
          remove "$index_dir/.cache"
          REMOVED=$((REMOVED + 1))
        fi
      fi
    done
  fi

  log "registry: removed $REMOVED items"
}

# ── 3. Clean git checkouts ──────────────────────────────────────────
# Parse Cargo.lock for git sources, keep only those repos + refs
clean_git() {
  log "cleaning cargo git cache..."
  REMOVED=0

  GIT_DIR="$CARGO_HOME/git"
  [ -d "$GIT_DIR" ] || return 0

  # Collect git repo URLs from Cargo.lock source fields
  GIT_REPOS=$(mktemp)
  if [ -f "$MANIFEST_DIR/Cargo.lock" ]; then
    # source = "git+https://github.com/user/repo?rev=abc#hash"
    # The db dir name is derived from the repo URL
    grep '^source = "git+' "$MANIFEST_DIR/Cargo.lock" 2>/dev/null | \
      sed 's/.*git+//; s/[?#].*//' | sort -u > "$GIT_REPOS" || true
  fi

  # Clean db/ — remove repos not in Cargo.lock
  if [ -d "$GIT_DIR/db" ]; then
    for repo_dir in "$GIT_DIR/db"/*/; do
      [ -d "$repo_dir" ] || continue
      repo_name=$(basename "$repo_dir")
      # repo_name is like "repo-name-HEXHASH" — we can't easily reverse the hash,
      # so if there are no git deps at all, clean everything
      if [ ! -s "$GIT_REPOS" ]; then
        remove "$repo_dir"
        REMOVED=$((REMOVED + 1))
      fi
    done
  fi

  # Clean checkouts/ — remove refs older than 7 days
  if [ -d "$GIT_DIR/checkouts" ]; then
    for repo_dir in "$GIT_DIR/checkouts"/*/; do
      [ -d "$repo_dir" ] || continue
      if [ ! -s "$GIT_REPOS" ]; then
        remove "$repo_dir"
        REMOVED=$((REMOVED + 1))
      fi
    done
  fi

  rm -f "$GIT_REPOS"
  log "git: removed $REMOVED items"
}

# ── Run ──────────────────────────────────────────────────────────────
BEFORE_SIZE=0
if command -v du >/dev/null 2>&1; then
  BEFORE_SIZE=$(du -sm "$TARGET_DIR" "$CARGO_HOME/registry" "$CARGO_HOME/git" 2>/dev/null | \
    awk '{s+=$1} END {print s+0}')
fi

clean_target
clean_registry
clean_git

if command -v du >/dev/null 2>&1; then
  AFTER_SIZE=$(du -sm "$TARGET_DIR" "$CARGO_HOME/registry" "$CARGO_HOME/git" 2>/dev/null | \
    awk '{s+=$1} END {print s+0}')
  SAVED=$((BEFORE_SIZE - AFTER_SIZE))
  log "total: ${BEFORE_SIZE}MB -> ${AFTER_SIZE}MB (saved ${SAVED}MB)"
fi
