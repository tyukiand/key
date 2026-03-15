#!/usr/bin/env bash
# Runs all CI checks. Executed both locally (inside Docker) and in GitHub Actions.
set -euo pipefail

apt-get update -qq && apt-get install -y -qq openssh-client
rustup component add rustfmt

cargo fmt --check
cargo test --features testing
