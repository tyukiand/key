# key

Porcelain wrapper over `ssh-keygen` and `ssh-add` for managing SSH keys.

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

## Usage

```
key add [KEY_ID]           # create a new SSH key
key list [-v]              # list keys
key activate [KEY_ID]      # load a key into ssh-agent
key pubkey [KEY_ID]        # print the public key to copy to GitHub/GitLab
key amend <FIELD> <VALUE> [KEY_ID]  # update password-storage or comment
key delete [KEY_ID]        # permanently delete a key
key user add <NAME>        # manage known users
key user list
key user delete [NAME]
key setup                  # add key's directory to PATH in shell RC
key status                 # merkle hash of current state
```
