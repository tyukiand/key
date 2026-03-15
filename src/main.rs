mod cli;
mod commands;
mod hash;
mod interactive;
mod mutation;
mod ssh;
mod state;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use cli::{Cli, Command, UserCommand};
use commands::key::AddOpts;
use mutation::MutationToken;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Check prerequisites
    ssh::check_prereqs()?;

    // Resolve key directory
    #[cfg(feature = "testing")]
    let key_dir = resolve_key_dir(&cli.test_only_key_dir)?;
    #[cfg(not(feature = "testing"))]
    let key_dir = resolve_key_dir(&None)?;

    // Load state
    let mut state = state::State::load(&key_dir)?;

    match cli.command {
        Command::User(user_cmd) => match user_cmd {
            UserCommand::List => commands::user::list(&state)?,

            UserCommand::Add { name } => {
                let token = MutationToken::acquire(cli.read_only)?;
                commands::user::add(&mut state, name, &token)?;
            }

            UserCommand::Delete { name } => {
                let token = MutationToken::acquire(cli.read_only)?;
                commands::user::delete(&mut state, name, &token)?;
            }
        },

        Command::List { verbose } => {
            commands::key::list(&state, verbose)?;
        }

        Command::Add { key_id } => {
            let token = MutationToken::acquire(cli.read_only)?;
            commands::key::add(
                &mut state,
                AddOpts {
                    key_id,
                    #[cfg(feature = "testing")]
                    canned_keys_dir: cli.test_only_canned_keys,
                    #[cfg(feature = "testing")]
                    test_user: cli.test_only_user,
                    #[cfg(feature = "testing")]
                    test_password_storage: cli.test_only_password_storage,
                    #[cfg(feature = "testing")]
                    test_comment: cli.test_only_comment,
                    #[cfg(feature = "testing")]
                    test_date: cli.test_only_date,
                },
                &token,
            )?;
        }

        Command::Pubkey { key_id } => {
            commands::key::pubkey(&state, key_id)?;
        }

        Command::Amend {
            field,
            value,
            key_id,
        } => {
            let token = MutationToken::acquire(cli.read_only)?;
            commands::key::amend(&mut state, key_id, field, value, &token)?;
        }

        Command::Delete { key_id } => {
            let token = MutationToken::acquire(cli.read_only)?;
            commands::key::delete(&mut state, key_id, &token)?;
        }

        Command::Activate { key_id } => {
            commands::activate::activate(&state, key_id)?;
        }

        Command::Status => {
            commands::status::status(&state)?;
        }

        Command::Setup => {
            #[cfg(feature = "testing")]
            commands::setup::setup(
                cli.test_only_home.as_deref(),
                cli.test_only_exe_dir.as_deref(),
            )?;
            #[cfg(not(feature = "testing"))]
            commands::setup::setup(None, None)?;
        }
    }

    Ok(())
}

fn resolve_key_dir(override_dir: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir.clone());
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    Ok(PathBuf::from(home).join(".key"))
}
