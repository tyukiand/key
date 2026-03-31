# key

Porcelain wrapper over `ssh-keygen` and `ssh-add` for managing SSH keys.

## Installation

Head over to the Releases (look on the right). Download the `.zip` for your platform and unpack it.

| File | Who it's for |
|------|-------------|
| `key-vX.Y.Z-linux-x86_64.zip` | Linux on a 64-bit Intel or AMD processor |
| `key-vX.Y.Z-macos-intel.zip` | macOS on an Intel Mac |
| `key-vX.Y.Z-macos-apple-silicon.zip` | macOS on an Apple Silicon Mac (M1, M2, M3, M4, …) |

Not sure which Mac you have? Apple menu → About This Mac. If "Chip" says "Apple M…" pick Apple Silicon; if it says "Intel" pick Intel.

```
# First-time setup (run from the unzipped release directory)
cd path/to/key-vX.Y.Z-your-os/bin && ./key setup  # adds key to PATH in .bashrc / .zshrc
```

## Usage
```
# Managing users
key user add <NAME>        # add a known user
key user list              # list known users
key user delete [NAME]     # remove a user

# Managing keys
key add [KEY_ID]           # create a new SSH key
key list [-v]              # list keys
key amend <FIELD> <VALUE> [KEY_ID]  # update password-storage or comment
key delete [KEY_ID]        # permanently delete a key
key pubkey [KEY_ID]        # print the public key to copy to GitHub/GitLab
key activate [KEY_ID]      # load a key into ssh-agent

# Rules — declarative config checks
key rules new <FILE>       # create an empty rules file
key rules add <FILE>       # interactively add a rule
key rules check <FILE>     # evaluate rules against $HOME
key rules test <FILE> <DIR> # evaluate rules against a fixture dir
key rules guide            # print the full rules syntax guide

# Diagnostics
key status                 # run this to understand current state
key help                   # show usage
```


## Building

```
cargo build --release
```

## Running tests

```
./test.sh
```

or directly:

```
cargo test --features testing
```

Plain `cargo test` (without `--features testing`) will fail — the integration tests spawn the binary with test-only flags that only exist when the feature is enabled.

## Releasing

Releases are triggered by pushing a tag named `vX.Y.Z` whose version matches `Cargo.toml`. The tag annotation becomes the GitHub release body.

1. Bump the version in `Cargo.toml`.
2. Commit the version bump (`git commit -m "bump version to vX.Y.Z"`).
3. Tag the commit with release notes as the annotation — **the tag annotation becomes the GitHub release body, not the commit message**:
   ```
   git tag -a vX.Y.Z
   ```
   Your editor opens; write human-readable release notes here (not the commit message).
   Or inline with `-m`:
   ```
   git tag -a vX.Y.Z -m "Release notes here"
   ```
4. Push the commit **and** the tag:
   ```
   git push origin main
   git push origin vX.Y.Z
   ```

CI will validate that the tag version matches `Cargo.toml`, build binaries for Linux (x86\_64), macOS Intel, and macOS Apple Silicon, and publish a GitHub release with all three attached.


