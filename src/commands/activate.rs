use anyhow::{bail, Result};

use crate::interactive::pick_from_list;
use crate::ssh;
use crate::state::State;

pub fn activate(state: &State, key_id: Option<String>) -> Result<()> {
    if state.keys.is_empty() {
        bail!("No keys available to activate");
    }

    let dir_names: Vec<String> = state.keys.iter().map(|k| k.dir_name.clone()).collect();

    let idx = match key_id {
        Some(ref id) => dir_names
            .iter()
            .position(|n| n == id || n.starts_with(&format!("{}_", id)))
            .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", id))?,
        None => pick_from_list("Select key to activate", &dir_names)?,
    };

    let key = &state.keys[idx];

    // Print metadata
    println!("Key:      {}", key.dir_name);
    println!("Created:  {}", key.info.creation_date);
    println!("Password: {}", key.info.password_storage);
    if let Some(comment) = &key.info.comment {
        println!("Comment:  {}", comment);
    }
    println!();

    // Delegate to ssh-add
    ssh::add_to_agent(&key.path.join("key"))?;
    Ok(())
}
