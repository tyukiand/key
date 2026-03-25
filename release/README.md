# key

Porcelain wrapper over `ssh-keygen` and `ssh-add` for managing SSH keys.

## Which download?

| File | Who it's for |
|------|-------------|
| `key-vX.Y.Z-linux-x86_64.zip` | Linux on a 64-bit Intel or AMD processor |
| `key-vX.Y.Z-macos-intel.zip` | macOS on an Intel Mac |
| `key-vX.Y.Z-macos-apple-silicon.zip` | macOS on an Apple Silicon Mac (M1, M2, M3, M4, …) |

Not sure which Mac you have? Apple menu → About This Mac. If "Chip" says "Apple M…" pick Apple Silicon; if it says "Intel" pick Intel.

## Getting started

### 1. Add `key` to your PATH

After unpacking, `cd` into the `bin` directory and run `key setup` once:

```
cd key-vX.Y.Z-your-os/bin && ./key setup
```

This adds the binary's directory to your shell RC file (`~/.zshrc` or `~/.bashrc`).
Open a new terminal (or `source` your RC file), and `key` will be available everywhere.

### 2. Add a known user

Keys are associated with a user. Add yourself before creating any keys:

```
key user add <NAME>
key user list
key user delete [NAME]
```

### 3. Create and manage keys

```
key add [KEY_ID]                    create a new SSH key (prompts for passphrase)
key list [-v]                       list all keys
key delete [KEY_ID]                 permanently delete a key
```

### 4. Use keys

```
key activate [KEY_ID]               load a key into ssh-agent
key pubkey [KEY_ID]                 print the public key to copy to GitHub/GitLab
key amend <FIELD> <VALUE> [KEY_ID]  update password-storage hint or comment
```

### 5. Check state

```
key status                          run this to understand current state
key help                            show usage
```

## Key storage

Keys are stored under `~/.key/keys/<key-id>_<date>/` with a `key`, `key.pub`, and `info.json` per key.
