use anyhow::{bail, Result};

use crate::effects::Effects;
use crate::state::State;

pub fn activate(state: &State, key_id: Option<String>, fx: &dyn Effects) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys available to activate");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => fx.pick_from_list("Select key to activate", &dir_names)?,
    };

    let key = &state.keys[idx];

    // Print metadata
    fx.println(&format!("Key:      {}", key.dir_name));
    fx.println(&format!("Created:  {}", key.info.creation_date));
    fx.println(&format!("Password: {}", key.info.password_storage));
    if let Some(comment) = &key.info.comment {
        fx.println(&format!("Comment:  {}", comment));
    }
    fx.println("");

    // Check if key is already active
    let pub_path = key.path.join("key.pub");
    let fingerprint = fx.ssh_keygen_fingerprint(&pub_path)?;
    let agent_list = fx.ssh_add_list()?;
    let already_active = agent_list
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .any(|fp| fp == fingerprint);
    if already_active {
        fx.eprintln("Note: key is already active in the SSH agent.");
    }

    // Delegate to ssh-add
    fx.ssh_add(&key.path.join("key"))?;
    Ok(())
}
