use anyhow::Result;

use crate::effects::Effects;
use crate::hash::compute_merkle_hash;
use crate::state::State;

pub fn status(state: &State, fx: &dyn Effects) -> Result<()> {
    // Merkle hash
    let hash = compute_merkle_hash(state, fx)?;
    fx.println(&format!("State hash: {}", hash));
    fx.println("");

    // Users
    fx.println(&format!("Users ({}):", state.settings.users.len()));
    if state.settings.users.is_empty() {
        fx.println("  (none)");
    } else {
        for (i, u) in state.settings.users.iter().enumerate() {
            fx.println(&format!("  {}. {}", i + 1, u));
        }
    }
    fx.println("");

    // Keys with activation status
    fx.println(&format!("Keys ({}):", state.keys.len()));
    if state.keys.is_empty() {
        fx.println("  (none)");
        return Ok(());
    }

    // Get currently loaded fingerprints from agent
    let agent_output = fx.ssh_add_list().unwrap_or_default();
    let agent_fps: Vec<&str> = agent_output
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();

    for (i, key) in state.keys.iter().enumerate() {
        let pub_path = key.path.join("key.pub");
        let active = if fx.path_exists(&pub_path) {
            match fx.ssh_keygen_fingerprint(&pub_path) {
                Ok(fp) => agent_fps.contains(&fp.as_str()),
                Err(_) => false,
            }
        } else {
            false
        };

        let status_str = if active { "ACTIVE" } else { "inactive" };
        fx.println(&format!("  {}. [{}] {}", i + 1, status_str, key.dir_name));
    }

    Ok(())
}
