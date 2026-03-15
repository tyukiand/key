use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Check that ssh-keygen and ssh-add are on PATH.
pub fn check_prereqs() -> Result<()> {
    for tool in &["ssh-keygen", "ssh-add"] {
        let status = Command::new("which")
            .arg(tool)
            .output()
            .with_context(|| format!("Checking for {}", tool))?;
        if !status.status.success() {
            bail!(
                "Required tool '{}' not found on PATH. Please install OpenSSH.",
                tool
            );
        }
    }
    Ok(())
}

/// Run ssh-keygen to generate a new ed25519 key.
/// Does NOT pass -N so ssh-keygen will interactively prompt for passphrase.
pub fn generate_key(key_path: &Path, comment: &str) -> Result<()> {
    let status = Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            key_path.to_str().expect("key path is valid UTF-8"),
            "-C",
            comment,
        ])
        .status()
        .context("Running ssh-keygen")?;

    if !status.success() {
        bail!("ssh-keygen exited with status {}", status);
    }

    // Defence-in-depth: enforce 0o600 regardless of umask or ssh-keygen version.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))
            .context("Enforcing 0o600 on private key")?;
    }

    Ok(())
}

/// Run ssh-add to add a key to the agent.
pub fn add_to_agent(key_path: &Path) -> Result<()> {
    let status = Command::new("ssh-add")
        .arg(key_path)
        .status()
        .context("Running ssh-add")?;

    if !status.success() {
        bail!("ssh-add exited with status {}", status);
    }
    Ok(())
}

/// Run `ssh-add -l` and return the raw output.
pub fn list_agent_keys() -> Result<String> {
    let output = Command::new("ssh-add")
        .arg("-l")
        .output()
        .context("Running ssh-add -l")?;

    // Exit code 1 = no identities; that's OK, return empty string.
    if output.status.code() == Some(1) {
        return Ok(String::new());
    }
    if !output.status.success() {
        bail!("ssh-add -l exited with status {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Compute the SHA256 fingerprint of a public key file.
pub fn pubkey_fingerprint(pub_path: &Path) -> Result<String> {
    let output = Command::new("ssh-keygen")
        .args([
            "-l",
            "-E",
            "sha256",
            "-f",
            pub_path.to_str().expect("pub path is valid UTF-8"),
        ])
        .output()
        .context("Running ssh-keygen -l")?;

    if !output.status.success() {
        bail!("ssh-keygen -l failed for {}", pub_path.display());
    }
    // Output looks like: 256 SHA256:xxxxx comment (ED25519)
    let line = String::from_utf8_lossy(&output.stdout);
    let fp = line
        .split_whitespace()
        .nth(1)
        .map(|s| s.to_string())
        .unwrap_or_default();
    Ok(fp)
}
