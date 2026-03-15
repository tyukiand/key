use anyhow::{bail, Result};

use crate::interactive::{force_retype, pick_from_list};
use crate::mutation::MutationToken;
use crate::state::State;

pub fn list(state: &State) -> Result<()> {
    if state.settings.users.is_empty() {
        println!("No users configured.");
        return Ok(());
    }
    for (i, user) in state.settings.users.iter().enumerate() {
        println!("  {}. {}", i + 1, user);
    }
    Ok(())
}

pub fn add(state: &mut State, name: String, _token: &MutationToken) -> Result<()> {
    if state.settings.users.contains(&name) {
        bail!("User '{}' already exists", name);
    }
    state.settings.users.push(name.clone());
    state.save_settings()?;
    println!("Added user: {}", name);
    Ok(())
}

pub fn delete(state: &mut State, name: Option<String>, _token: &MutationToken) -> Result<()> {
    if state.settings.users.is_empty() {
        bail!("No users to delete");
    }

    let target = match name {
        Some(n) => n,
        None => {
            let idx = pick_from_list("Select user to delete", &state.settings.users)?;
            state.settings.users[idx].clone()
        }
    };

    if !state.settings.users.contains(&target) {
        bail!("User '{}' not found", target);
    }

    force_retype(
        &format!("You are about to delete user '{}'.", target),
        &target,
    )?;

    state.settings.users.retain(|u| u != &target);
    state.save_settings()?;
    println!("Deleted user: {}", target);
    Ok(())
}
