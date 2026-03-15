use anyhow::{bail, Result};
use dialoguer::{theme::ColorfulTheme, Select};
use std::io::{BufRead, Write};

/// Present a numbered list and return the selected index.
/// Uses dialoguer Select (requires a TTY / arrow keys).
pub fn pick_from_list(prompt: &str, items: &[String]) -> Result<usize> {
    if items.is_empty() {
        bail!("No items to pick from");
    }
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()?;
    Ok(selection)
}

/// Force user to type the exact `expected` string before proceeding.
/// Uses raw stdin — works both interactively and with piped input in tests.
pub fn force_retype(prompt: &str, expected: &str) -> Result<()> {
    println!("{}", prompt);
    loop {
        print!("Type '{}' to confirm: ", expected);
        std::io::stdout().flush()?;
        let mut line = String::new();
        let n = std::io::stdin().lock().read_line(&mut line)?;
        if n == 0 {
            anyhow::bail!("Aborted: EOF before confirmation was typed");
        }
        if line.trim() == expected {
            return Ok(());
        }
        println!("Input did not match. Try again (Ctrl-C to abort).");
    }
}

/// Prompt for a single required text value.
/// Uses raw stdin — works with piped input in tests.
pub fn prompt_text(prompt: &str) -> Result<String> {
    print!("{}: ", prompt);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Prompt for an optional text value (empty → None).
/// Uses raw stdin — works with piped input in tests.
pub fn prompt_optional(prompt: &str) -> Result<Option<String>> {
    print!("{} (leave empty to skip): ", prompt);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}
