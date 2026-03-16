use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "key",
    about = "Porcelain wrapper over ssh-keygen and ssh-add for managing SSH keys",
    version
)]
pub struct Cli {
    /// Prevent any state mutations (read-only commands only)
    #[arg(long, global = true)]
    pub read_only: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manage known users
    #[command(subcommand)]
    User(UserCommand),

    /// List existing SSH keys
    #[command(name = "list")]
    List {
        /// Verbose output: show creation date, password storage hint, comment
        #[arg(short, long)]
        verbose: bool,
    },

    /// Add a new SSH key
    #[command(name = "add")]
    Add {
        /// Key identifier (without date); prompted if omitted
        key_id: Option<String>,
    },

    /// Delete an SSH key
    #[command(name = "delete")]
    Delete {
        /// Key identifier; interactive picker if omitted
        key_id: Option<String>,
    },

    /// Activate an SSH key via ssh-add
    #[command(name = "activate")]
    Activate {
        /// Key identifier; interactive picker if omitted
        key_id: Option<String>,
    },

    /// Show merkle hash of state, users, and key activation status
    #[command(name = "status")]
    Status,

    /// Print the public key, wrapped in copy-guide delimiters.
    ///
    /// Usage: key pubkey [KEY_ID]
    ///
    /// Prints the contents of key.pub between two marker lines so you can
    /// see exactly what to copy into GitHub / GitLab / authorized_keys.
    /// KEY_ID is the key identifier; if omitted, an interactive picker is shown.
    #[command(name = "pubkey")]
    Pubkey {
        /// Key identifier; interactive picker if omitted
        key_id: Option<String>,
    },

    /// Amend mutable metadata for an existing key.
    ///
    /// Usage: key amend <FIELD> <VALUE> [KEY_ID]
    ///
    /// FIELD is one of: password-storage, comment
    /// VALUE is the new value (use "" to clear the comment)
    /// KEY_ID is the key identifier; if omitted, an interactive picker is shown
    ///
    /// Examples:
    ///   key amend password-storage "1Password > SSH > work"
    ///   key amend comment "main work key" github-work
    ///   key amend comment "" github-work   # clears the comment
    #[command(name = "amend")]
    Amend {
        /// Field to update: password-storage or comment
        field: AmendField,
        /// New value (use "" to clear the comment field)
        value: String,
        /// Key identifier; interactive picker if omitted
        key_id: Option<String>,
    },

    /// Add this executable's directory to PATH in the shell RC file
    #[command(name = "setup")]
    Setup,
}

/// Mutable fields that can be changed with `key amend`.
/// The creation date is intentionally excluded.
#[derive(ValueEnum, Clone, Debug)]
pub enum AmendField {
    /// The password-manager hint for where the passphrase is stored
    #[value(name = "password-storage")]
    PasswordStorage,
    /// Free-form comment attached to the key
    #[value(name = "comment")]
    Comment,
}

#[derive(Subcommand, Debug)]
pub enum UserCommand {
    /// List known users
    #[command(name = "list")]
    List,

    /// Add a user (e.g. alice@github)
    #[command(name = "add")]
    Add {
        /// User name in name@where format
        name: String,
    },

    /// Delete a user
    #[command(name = "delete")]
    Delete {
        /// User name; interactive picker if omitted
        name: Option<String>,
    },
}
