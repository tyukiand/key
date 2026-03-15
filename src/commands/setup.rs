use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const MARKER: &str = "# __added_by_key";
const START_LINE: &str = "# [ADDED BY key] START # __added_by_key";
const END_LINE: &str = "# [ADDED by key] END   # __added_by_key";

pub fn setup(test_only_home: Option<&Path>, test_only_exe_dir: Option<&Path>) -> Result<()> {
    let exe_path_buf;
    let exe_dir: &Path = if let Some(dir) = test_only_exe_dir {
        dir
    } else {
        exe_path_buf = std::env::current_exe().context("Failed to determine executable path")?;
        exe_path_buf
            .parent()
            .context("Executable has no parent directory")?
    };

    let rc_path = detect_shell_rc(test_only_home)?;

    update_rc_file(&rc_path, exe_dir)?;

    println!("Updated {}", rc_path.display());
    println!("Added {} to PATH in shell config.", exe_dir.display());
    println!("Restart your shell or run: source {}", rc_path.display());

    Ok(())
}

fn detect_shell_rc(test_only_home: Option<&Path>) -> Result<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let home = test_only_home
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()));

    let rc_name = if shell.contains("zsh") {
        ".zshrc"
    } else if shell.contains("bash") {
        ".bashrc"
    } else if shell.is_empty() {
        bail!("$SHELL is not set; cannot determine shell RC file");
    } else {
        bail!(
            "Unsupported shell '{}'; only bash and zsh are supported",
            shell
        );
    };

    Ok(PathBuf::from(home).join(rc_name))
}

fn update_rc_file(rc_path: &Path, exe_dir: &Path) -> Result<()> {
    let existing = if rc_path.exists() {
        std::fs::read_to_string(rc_path)
            .with_context(|| format!("Failed to read {}", rc_path.display()))?
    } else {
        String::new()
    };

    let cleaned = remove_key_block(&existing);

    let new_block = format!(
        "{start}\nexport PATH=\"{dir}:$PATH\" {marker}\n{end}\n",
        start = START_LINE,
        dir = exe_dir.display(),
        marker = MARKER,
        end = END_LINE,
    );

    let result = if cleaned.is_empty() {
        new_block
    } else if cleaned.ends_with('\n') {
        format!("{}{}", cleaned, new_block)
    } else {
        format!("{}\n{}", cleaned, new_block)
    };

    std::fs::write(rc_path, &result)
        .with_context(|| format!("Failed to write {}", rc_path.display()))?;

    Ok(())
}

/// Remove any previously added block (lines containing `# __added_by_key`).
/// Handles both block-style (START…END) and stray individual lines.
fn remove_key_block(content: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut inside_block = false;

    for line in content.lines() {
        if line.trim_end() == START_LINE {
            inside_block = true;
            continue;
        }
        if inside_block {
            if line.trim_end() == END_LINE {
                inside_block = false;
            }
            continue;
        }
        // Remove any stray lines that contain the marker (shouldn't normally exist)
        if line.contains(MARKER) {
            continue;
        }
        lines.push(line);
    }

    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}
