use anyhow::{bail, Context, Result};
#[cfg(feature = "testing")]
use std::path::PathBuf;

use crate::interactive::{force_retype, pick_from_list, prompt_optional, prompt_text};
use crate::mutation::MutationToken;
use crate::state::{self, KeyInfo, State};
use crate::ssh;

pub struct AddOpts {
    pub key_id: Option<String>,
    #[cfg(feature = "testing")]
    pub canned_keys_dir: Option<PathBuf>,
    #[cfg(feature = "testing")]
    pub test_user: Option<String>,
    #[cfg(feature = "testing")]
    pub test_password_storage: Option<String>,
    #[cfg(feature = "testing")]
    pub test_comment: Option<String>,
}

pub fn print_pubkey(key_path: &std::path::Path) -> Result<()> {
    let pub_path = key_path.with_extension("pub");
    let content = std::fs::read_to_string(&pub_path)
        .with_context(|| format!("Reading {}", pub_path.display()))?;
    println!("--- public key start (do not copy this line) ---");
    print!("{}", content);
    println!("--- public key end (do not copy this line) ---");
    Ok(())
}

pub fn pubkey(state: &State, key_id: Option<String>) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys found");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => pick_from_list("Select key", &dir_names)?,
    };

    print_pubkey(&state.keys[idx].path.join("key"))
}

pub fn list(state: &State, verbose: bool) -> Result<()> {
    if state.keys.is_empty() {
        println!("No keys found.");
        return Ok(());
    }
    for (i, key) in state.keys.iter().enumerate() {
        if verbose {
            println!("  {}. {}", i + 1, key.dir_name);
            println!("      created:  {}", key.info.creation_date);
            println!("      password: {}", key.info.password_storage);
            if let Some(comment) = &key.info.comment {
                println!("      comment:  {}", comment);
            }
        } else {
            println!("  {}. {}", i + 1, key.dir_name);
        }
    }
    Ok(())
}

pub fn add(state: &mut State, opts: AddOpts, _token: &MutationToken) -> Result<()> {
    // 1. Get key-id
    let key_id = match opts.key_id {
        Some(id) => id,
        None => prompt_text("Enter key ID (without date)")?,
    };

    // Validate no existing key has same id prefix
    for existing in &state.keys {
        if existing.key_id() == key_id {
            bail!(
                "A key with ID '{}' already exists: {}",
                key_id,
                existing.dir_name
            );
        }
    }

    // 2. Pick user
    #[cfg(feature = "testing")]
    let user = match opts.test_user {
        Some(u) => u,
        None => pick_user(state)?,
    };
    #[cfg(not(feature = "testing"))]
    let user = pick_user(state)?;

    // 3. Password storage hint
    #[cfg(feature = "testing")]
    let password_storage = match opts.test_password_storage {
        Some(p) => p,
        None => prompt_text("Where is the password stored? (hint)")?,
    };
    #[cfg(not(feature = "testing"))]
    let password_storage = prompt_text("Where is the password stored? (hint)")?;

    // 4. Optional comment
    #[cfg(feature = "testing")]
    let comment = match opts.test_comment {
        Some(c) if c.is_empty() => None,
        Some(c) => Some(c),
        None => prompt_optional("Comment (optional)")?,
    };
    #[cfg(not(feature = "testing"))]
    let comment = prompt_optional("Comment (optional)")?;

    // 5. Build full dir name
    let date_str = state::current_date_string();
    let dir_name = format!("{}_{}", key_id, date_str);
    let dir_path = state.keys_path().join(&dir_name);

    std::fs::create_dir_all(&dir_path)
        .with_context(|| format!("Creating key directory {}", dir_path.display()))?;

    // 6. Generate or copy key pair
    println!("Creating {}", dir_name);
    let key_path = dir_path.join("key");

    #[cfg(feature = "testing")]
    {
        if let Some(canned_dir) = opts.canned_keys_dir {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(key_id.as_bytes());
            hasher.update(user.as_bytes());
            hasher.update(password_storage.as_bytes());
            if let Some(c) = &comment {
                hasher.update(c.as_bytes());
            }
            let hash = hasher.finalize();
            let hash_val = u64::from_le_bytes(hash[..8].try_into().unwrap());

            // Collect private key paths from canned dir.
            // Layout: flat files (no .pub extension) OR subdir/key pairs.
            let priv_keys: Vec<PathBuf> = {
                let mut keys = Vec::new();
                let subdirs: Vec<_> = std::fs::read_dir(&canned_dir)?
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .collect();

                if !subdirs.is_empty() {
                    for d in &subdirs {
                        let k = d.path().join("key");
                        if k.exists() {
                            keys.push(k);
                        }
                    }
                } else {
                    for e in std::fs::read_dir(&canned_dir)?.filter_map(|e| e.ok()) {
                        let p = e.path();
                        if p.is_file() && p.extension().map(|x| x != "pub").unwrap_or(true) {
                            keys.push(p);
                        }
                    }
                }
                keys.sort();
                keys
            };

            if priv_keys.is_empty() {
                bail!("No canned keys found in {}", canned_dir.display());
            }

            let idx = (hash_val as usize) % priv_keys.len();
            let canned_priv = &priv_keys[idx];
            let canned_pub = canned_priv.with_extension("pub");

            std::fs::copy(canned_priv, &key_path).context("Copying canned private key")?;
            std::fs::copy(&canned_pub, dir_path.join("key.pub")).context("Copying canned public key")?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
            }

            println!("(test mode) Used canned key #{}", idx);
        } else {
            ssh::generate_key(&key_path, &user)?;
        }
    }
    #[cfg(not(feature = "testing"))]
    ssh::generate_key(&key_path, &user)?;

    // 7. Write info.json
    let info = KeyInfo {
        creation_date: date_str,
        password_storage,
        comment,
    };
    state::write_info(&dir_path, &info)?;

    println!("Key created: {}", dir_name);
    print_pubkey(&key_path)?;
    Ok(())
}

pub fn amend(
    state: &mut State,
    key_id: Option<String>,
    field: crate::cli::AmendField,
    value: String,
    _token: &MutationToken,
) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys to amend");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => pick_from_list("Select key to amend", &dir_names)?,
    };

    let key = &mut state.keys[idx];

    match field {
        crate::cli::AmendField::PasswordStorage => {
            key.info.password_storage = value.clone();
        }
        crate::cli::AmendField::Comment => {
            key.info.comment = if value.is_empty() { None } else { Some(value.clone()) };
        }
    }

    state::write_info(&key.path, &key.info)?;
    println!("Updated {}: {}", key.dir_name, value);
    Ok(())
}

pub fn delete(state: &mut State, key_id: Option<String>, _token: &MutationToken) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys to delete");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let target_idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => pick_from_list("Select key to delete", &dir_names)?,
    };

    let target = &state.keys[target_idx];
    let dir_name = target.dir_name.clone();
    let path = target.path.clone();

    force_retype(
        &format!("You are about to permanently delete key '{}'.", dir_name),
        &dir_name,
    )?;

    std::fs::remove_dir_all(&path)
        .with_context(|| format!("Removing {}", path.display()))?;

    println!("Deleted key: {}", dir_name);
    Ok(())
}

/// Pick or create a user interactively.
fn pick_user(state: &State) -> Result<String> {
    if state.settings.users.is_empty() {
        println!("No known users. Enter a user name:");
        return prompt_text("User (e.g. alice@github)");
    }

    let mut options = state.settings.users.clone();
    options.push("[ type a new user ]".to_string());

    let idx = pick_from_list("Select user", &options)?;
    if idx == options.len() - 1 {
        prompt_text("User (e.g. alice@github)")
    } else {
        Ok(options[idx].clone())
    }
}
