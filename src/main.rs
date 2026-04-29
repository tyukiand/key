mod cli;
mod commands;
mod effects;
mod guide_edsl;
mod hash;
mod mutation;
mod rules;
mod state;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use cli::{AuditCommand, Cli, Command, UserCommand};
use effects::{Effects, RealEffects};
use mutation::MutationToken;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let fx = RealEffects;

    // Audit commands don't need SSH or key state
    if let Command::Audit { ref command } = cli.command {
        let home = match cli.test_only_home_dir {
            Some(ref h) => PathBuf::from(h),
            None => PathBuf::from(fx.home_dir()?),
        };
        return match command {
            None => commands::audit::dispatch_pick(&home, &fx),
            Some(AuditCommand::Project(ref proj_cmd)) => {
                commands::audit::dispatch_project(proj_cmd, &home, &fx)
            }
            Some(ref cmd) => commands::audit::dispatch(cmd, &home, &fx),
        };
    }

    // Check prerequisites
    fx.check_ssh_prereqs()?;

    // Resolve key directory
    let key_dir = resolve_key_dir(&fx)?;

    // Load state
    let mut state = state::State::load(&key_dir, &fx)?;

    match cli.command {
        Command::User(user_cmd) => match user_cmd {
            UserCommand::List => commands::user::list(&state, &fx)?,

            UserCommand::Add { name } => {
                let token = MutationToken::acquire(cli.read_only)?;
                commands::user::add(&mut state, name, &fx, &token)?;
            }

            UserCommand::Delete { name } => {
                let token = MutationToken::acquire(cli.read_only)?;
                commands::user::delete(&mut state, name, &fx, &token)?;
            }
        },

        Command::List { verbose } => {
            commands::key::list(&state, verbose, &fx)?;
        }

        Command::Add { key_id } => {
            let token = MutationToken::acquire(cli.read_only)?;
            commands::key::add(&mut state, key_id, &fx, &token)?;
        }

        Command::Pubkey { key_id } => {
            commands::key::pubkey(&state, key_id, &fx)?;
        }

        Command::Amend {
            field,
            value,
            key_id,
        } => {
            let token = MutationToken::acquire(cli.read_only)?;
            commands::key::amend(&mut state, key_id, field, value, &fx, &token)?;
        }

        Command::Delete { key_id } => {
            let token = MutationToken::acquire(cli.read_only)?;
            commands::key::delete(&mut state, key_id, &fx, &token)?;
        }

        Command::Activate { key_id } => {
            commands::activate::activate(&state, key_id, &fx)?;
        }

        Command::Status => {
            commands::status::status(&state, &fx)?;
        }

        Command::Setup => {
            commands::setup::setup(&fx)?;
        }

        Command::Audit { .. } => unreachable!("handled above"),
    }

    Ok(())
}

fn resolve_key_dir(fx: &dyn Effects) -> Result<PathBuf> {
    let home = fx.home_dir()?;
    Ok(PathBuf::from(home).join(".key"))
}
