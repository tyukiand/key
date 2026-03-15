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

    /// Override ~/.key directory (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "DIR")]
    pub test_only_key_dir: Option<std::path::PathBuf>,

    /// Use canned key pairs instead of running ssh-keygen (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "DIR")]
    pub test_only_canned_keys: Option<std::path::PathBuf>,

    /// Skip user picker in `key add`, use this user instead (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "USER", hide = true)]
    pub test_only_user: Option<String>,

    /// Skip password-storage prompt in `key add`, use this value instead (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "HINT", hide = true)]
    pub test_only_password_storage: Option<String>,

    /// Skip comment prompt in `key add`, use this value instead; empty = no comment (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "COMMENT", hide = true)]
    pub test_only_comment: Option<String>,

    /// Override HOME directory used by `key setup` (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "DIR", hide = true)]
    pub test_only_home: Option<std::path::PathBuf>,

    /// Override exe directory used by `key setup` (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "DIR", hide = true)]
    pub test_only_exe_dir: Option<std::path::PathBuf>,

    /// Override the current date string used by `key add` (test use only)
    #[cfg(feature = "testing")]
    #[arg(long, global = true, value_name = "DATE", hide = true)]
    pub test_only_date: Option<String>,

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
