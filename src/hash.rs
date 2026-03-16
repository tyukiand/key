use crate::effects::Effects;
use crate::state::State;
use anyhow::Result;
use sha2::{Digest, Sha256};

/// Compute a location-invariant merkle hash of all state.
pub fn compute_merkle_hash(state: &State, fx: &dyn Effects) -> Result<String> {
    let mut leaf_hashes: Vec<Vec<u8>> = Vec::new();

    // 1. Users (sorted alphabetically)
    let mut users = state.settings.users.clone();
    users.sort();
    for user in &users {
        leaf_hashes.push(sha256_bytes(user.as_bytes()));
    }

    // 2. Key dirs (already sorted by dir_name in State::load)
    for key in &state.keys {
        // Hash dir name (relative, not absolute)
        leaf_hashes.push(sha256_bytes(key.dir_name.as_bytes()));

        // Hash info.json content
        let info_content = fx.read_file(&key.path.join("info.json"))?;
        leaf_hashes.push(sha256_bytes(&info_content));

        // Hash key.pub content
        let pub_path = key.path.join("key.pub");
        if fx.path_exists(&pub_path) {
            let pub_content = fx.read_file(&pub_path)?;
            leaf_hashes.push(sha256_bytes(&pub_content));
        }
    }

    // Combine all leaf hashes
    let mut combined = Sha256::new();
    for leaf in &leaf_hashes {
        combined.update(leaf);
    }
    let result = combined.finalize();
    Ok(hex::encode_bytes(&result))
}

fn sha256_bytes(data: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().to_vec()
}

mod hex {
    pub fn encode_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
