#!/usr/bin/env bash
# Cut a new release of `key`.
#
#   ./release.sh --major   # X.0.0 ← (X+1).0.0
#   ./release.sh --minor   # X.Y.0 ← X.(Y+1).0
#   ./release.sh --patch   # X.Y.Z ← X.Y.(Z+1)
#
# What it does:
#   1. Sanity-checks: clean tree, on main, up to date with origin.
#   2. Reads current version from Cargo.toml's [package] section
#      (and ONLY that section — never `bitflags` or any other
#      transitive crate's version line).
#   3. Computes the new version per the bump flag.
#   4. Updates Cargo.toml's [package].version.
#   5. Runs `cargo build --release` so cargo updates Cargo.lock's
#      local-crate entry. This is the SAFE way to keep Cargo.lock
#      consistent — never edit Cargo.lock with sed.
#   6. Runs the full ci.sh equivalent (fmt, release build, tests)
#      to confirm green BEFORE touching git.
#   7. Commits "bump version to X.Y.Z", tags vX.Y.Z, pushes both.
#
# Why this exists: hand-editing Cargo.lock with sed is how key's
# v2.12.0 release got bitflags collaterally bumped from 2.11.0 to
# 2.12.0, breaking CI. Letting cargo regenerate Cargo.lock from a
# Cargo.toml bump is the only safe path.

set -euo pipefail

usage() {
    echo "usage: $0 --major | --minor | --patch" >&2
    exit 2
}

[[ $# -eq 1 ]] || usage
case "$1" in
    --major|--minor|--patch) BUMP="${1#--}" ;;
    *) usage ;;
esac

cd "$(dirname "$0")"

# 1. Sanity checks.
if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is dirty. Commit or stash first." >&2
    git status --short >&2
    exit 1
fi
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
    echo "error: not on main (currently on '$BRANCH'). Switch to main first." >&2
    exit 1
fi
git fetch origin main --quiet
LOCAL="$(git rev-parse main)"
REMOTE="$(git rev-parse origin/main)"
if [[ "$LOCAL" != "$REMOTE" ]]; then
    echo "error: main is not in sync with origin/main." >&2
    echo "  local:  $LOCAL" >&2
    echo "  remote: $REMOTE" >&2
    echo "  pull or push first." >&2
    exit 1
fi

# 2. Read current version from the [package] section ONLY.
#    awk: enter [package] block, parse first version= line via
#    split-on-quote (greedy regex strip would delete the whole
#    line in POSIX awk), stop.
CURRENT="$(awk '
    /^\[package\]/ { in_pkg = 1; next }
    /^\[/          { in_pkg = 0 }
    in_pkg && /^version[[:space:]]*=/ {
        n = split($0, a, "\"")
        if (n >= 3) print a[2]
        exit
    }
' Cargo.toml)"

if [[ ! "$CURRENT" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    echo "error: could not parse current version '$CURRENT' from Cargo.toml [package]" >&2
    exit 1
fi
MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
PATCH="${BASH_REMATCH[3]}"

# 3. Compute new version.
case "$BUMP" in
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    patch) PATCH=$((PATCH + 1)) ;;
esac
NEW="$MAJOR.$MINOR.$PATCH"
TAG="v$NEW"

if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "error: tag '$TAG' already exists." >&2
    exit 1
fi

echo "Bumping $CURRENT → $NEW ($BUMP)"

# 4. Update Cargo.toml's [package].version. Use awk so we touch
#    EXACTLY the [package] section's version, never anything else.
awk -v new="$NEW" '
    BEGIN { in_pkg = 0; bumped = 0 }
    /^\[package\]/ { in_pkg = 1; print; next }
    /^\[/          { in_pkg = 0; print; next }
    in_pkg && !bumped && /^version[[:space:]]*=/ {
        print "version = \"" new "\""
        bumped = 1
        next
    }
    { print }
    END {
        if (!bumped) {
            print "release.sh: failed to bump Cargo.toml [package].version" > "/dev/stderr"
            exit 1
        }
    }
' Cargo.toml > Cargo.toml.new
mv Cargo.toml.new Cargo.toml

# 5. Let cargo regenerate Cargo.lock for the local crate. This is
#    the ONLY safe way to keep Cargo.lock consistent — never sed it.
cargo build --release --quiet

# 6. Reproduce ci.sh end-to-end. Fail fast on red.
cargo fmt --check
RUSTFLAGS="-D warnings" cargo build --release
RUSTFLAGS="-D warnings" cargo test --features testing

# 7. Commit, tag, push.
git add Cargo.toml Cargo.lock
git commit -m "bump version to $NEW"
git tag "$TAG"
git push origin main "$TAG"

echo
echo "Released $TAG."
