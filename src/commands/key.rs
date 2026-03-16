use anyhow::{bail, Context, Result};

use crate::effects::Effects;
use crate::mutation::MutationToken;
use crate::state::{KeyInfo, State};

pub fn print_pubkey(key_path: &std::path::Path, fx: &dyn Effects) -> Result<()> {
    let pub_path = key_path.with_extension("pub");
    let content = fx
        .read_file_string(&pub_path)
        .with_context(|| format!("Reading {}", pub_path.display()))?;
    fx.println("--- public key start (do not copy this line) ---");
    fx.println(&content.trim_end());
    fx.println("--- public key end (do not copy this line) ---");
    Ok(())
}

pub fn pubkey(state: &State, key_id: Option<String>, fx: &dyn Effects) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys found");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => fx.pick_from_list("Select key", &dir_names)?,
    };

    print_pubkey(&state.keys[idx].path.join("key"), fx)
}

pub fn list(state: &State, verbose: bool, fx: &dyn Effects) -> Result<()> {
    if state.keys.is_empty() {
        fx.println("No keys found.");
        return Ok(());
    }
    for (i, key) in state.keys.iter().enumerate() {
        if verbose {
            fx.println(&format!("  {}. {}", i + 1, key.dir_name));
            fx.println(&format!("      created:  {}", key.info.creation_date));
            fx.println(&format!("      password: {}", key.info.password_storage));
            if let Some(comment) = &key.info.comment {
                fx.println(&format!("      comment:  {}", comment));
            }
        } else {
            fx.println(&format!("  {}. {}", i + 1, key.dir_name));
        }
    }
    Ok(())
}

pub fn add(
    state: &mut State,
    key_id: Option<String>,
    fx: &dyn Effects,
    _token: &MutationToken,
) -> Result<()> {
    // 1. Get key-id
    let key_id = match key_id {
        Some(id) => id,
        None => fx.prompt_text("Enter key ID (without date)")?,
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
    let user = pick_user(state, fx)?;

    // 3. Password storage hint
    let password_storage = fx.prompt_text("Where is the password stored? (hint)")?;

    // 4. Optional comment
    let comment = fx.prompt_optional("Comment (optional)")?;

    // 5. Build full dir name
    let date_str = fx.current_date_string();
    let dir_name = format!("{}_{}", key_id, date_str);
    let dir_path = state.keys_path().join(&dir_name);

    fx.create_dir_all(&dir_path)
        .with_context(|| format!("Creating key directory {}", dir_path.display()))?;

    // 6. Generate key pair
    fx.println(&format!("Creating {}", dir_name));
    let key_path = dir_path.join("key");
    fx.ssh_keygen_generate(&key_path, &user)?;

    // 7. Write info.json
    let info = KeyInfo {
        creation_date: date_str,
        password_storage,
        comment,
    };
    crate::state::write_info(&dir_path, &info, fx)?;

    fx.println(&format!("Key created: {}", dir_name));
    print_pubkey(&key_path, fx)?;
    Ok(())
}

pub fn amend(
    state: &mut State,
    key_id: Option<String>,
    field: crate::cli::AmendField,
    value: String,
    fx: &dyn Effects,
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
        None => fx.pick_from_list("Select key to amend", &dir_names)?,
    };

    let key = &mut state.keys[idx];

    match field {
        crate::cli::AmendField::PasswordStorage => {
            key.info.password_storage = value.clone();
        }
        crate::cli::AmendField::Comment => {
            key.info.comment = if value.is_empty() {
                None
            } else {
                Some(value.clone())
            };
        }
    }

    crate::state::write_info(&key.path, &key.info, fx)?;
    fx.println(&format!("Updated {}: {}", key.dir_name, value));
    Ok(())
}

pub fn delete(
    state: &mut State,
    key_id: Option<String>,
    fx: &dyn Effects,
    _token: &MutationToken,
) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys to delete");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let target_idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => fx.pick_from_list("Select key to delete", &dir_names)?,
    };

    let target = &state.keys[target_idx];
    let dir_name = target.dir_name.clone();
    let path = target.path.clone();

    fx.force_retype(
        &format!("You are about to permanently delete key '{}'.", dir_name),
        &dir_name,
    )?;

    fx.remove_dir_all(&path)
        .with_context(|| format!("Removing {}", path.display()))?;

    fx.println(&format!("Deleted key: {}", dir_name));
    Ok(())
}

/// Pick or create a user interactively.
fn pick_user(state: &State, fx: &dyn Effects) -> Result<String> {
    if state.settings.users.is_empty() {
        fx.println("No known users. Enter a user name:");
        return fx.prompt_text("User (e.g. alice@github)");
    }

    let mut options = state.settings.users.clone();
    options.push("[ type a new user ]".to_string());

    let idx = fx.pick_from_list("Select user", &options)?;
    if idx == options.len() - 1 {
        fx.prompt_text("User (e.g. alice@github)")
    } else {
        Ok(options[idx].clone())
    }
}
