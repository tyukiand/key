use anyhow::Result;

use crate::hash::compute_merkle_hash;
use crate::ssh;
use crate::state::State;

pub fn status(state: &State) -> Result<()> {
    // Merkle hash
    let hash = compute_merkle_hash(state)?;
    println!("State hash: {}", hash);
    println!();

    // Users
    println!("Users ({}):", state.settings.users.len());
    if state.settings.users.is_empty() {
        println!("  (none)");
    } else {
        for (i, u) in state.settings.users.iter().enumerate() {
            println!("  {}. {}", i + 1, u);
        }
    }
    println!();

    // Keys with activation status
    println!("Keys ({}):", state.keys.len());
    if state.keys.is_empty() {
        println!("  (none)");
        return Ok(());
    }

    // Get currently loaded fingerprints from agent
    let agent_output = ssh::list_agent_keys().unwrap_or_default();
    let agent_fps: Vec<&str> = agent_output
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();

    for (i, key) in state.keys.iter().enumerate() {
        let pub_path = key.path.join("key.pub");
        let active = if pub_path.exists() {
            match ssh::pubkey_fingerprint(&pub_path) {
                Ok(fp) => agent_fps.contains(&fp.as_str()),
                Err(_) => false,
            }
        } else {
            false
        };

        let status_str = if active { "ACTIVE" } else { "inactive" };
        println!("  {}. [{}] {}", i + 1, status_str, key.dir_name);
    }

    Ok(())
}
