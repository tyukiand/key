#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo test --features testing
HAIKU_OFFLINE=1 cargo test --features testing --test integration haiku
