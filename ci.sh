#!/usr/bin/env bash
# Runs all CI checks. Executed both locally (inside Docker) and in GitHub Actions.
set -euo pipefail

apt-get update -qq && apt-get install -y -qq openssh-client
rustup component add rustfmt

cargo fmt --check
# Build the release binary first so tests/emit_project_release.rs can
# subprocess-invoke target/release/key (per spec/0013 §B.6).
RUSTFLAGS="-D warnings" cargo build --release
RUSTFLAGS="-D warnings" cargo test --features testing
