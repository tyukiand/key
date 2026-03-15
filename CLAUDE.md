# CLAUDE.md

## After every task

Run all tests and rebuild the release binary:

```
./test.sh
cargo build --release
```

Tests require `--features testing` — `./test.sh` handles this. Do not run bare `cargo test`.

## Project overview

`key` is a CLI tool wrapping `ssh-keygen` and `ssh-add` for managing SSH keys with metadata (creation date, password-manager hint, optional comment).

Keys are stored under `~/.key/keys/<key-id>_<date>/` with a `key`, `key.pub`, and `info.json` per key.
