use anyhow::{bail, Result};

use crate::effects::Effects;
use crate::mutation::MutationToken;
use crate::state::State;

pub fn list(state: &State, fx: &dyn Effects) -> Result<()> {
    if state.settings.users.is_empty() {
        fx.println("No users configured.");
        return Ok(());
    }
    for (i, user) in state.settings.users.iter().enumerate() {
        fx.println(&format!("  {}. {}", i + 1, user));
    }
    Ok(())
}

pub fn add(
    state: &mut State,
    name: String,
    fx: &dyn Effects,
    _token: &MutationToken,
) -> Result<()> {
    if state.settings.users.contains(&name) {
        bail!("User '{}' already exists", name);
    }
    state.settings.users.push(name.clone());
    state.save_settings(fx)?;
    fx.println(&format!("Added user: {}", name));
    Ok(())
}

pub fn delete(
    state: &mut State,
    name: Option<String>,
    fx: &dyn Effects,
    _token: &MutationToken,
) -> Result<()> {
    if state.settings.users.is_empty() {
        bail!("No users to delete");
    }

    let target = match name {
        Some(n) => n,
        None => {
            let idx = fx.pick_from_list("Select user to delete", &state.settings.users)?;
            state.settings.users[idx].clone()
        }
    };

    if !state.settings.users.contains(&target) {
        bail!("User '{}' not found", target);
    }

    fx.force_retype(
        &format!("You are about to delete user '{}'.", target),
        &target,
    )?;

    state.settings.users.retain(|u| u != &target);
    state.save_settings(fx)?;
    fx.println(&format!("Deleted user: {}", target));
    Ok(())
}
