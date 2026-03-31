use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::cli::RulesCommand;
use crate::effects::Effects;
use crate::rules::evaluate::evaluate;
use crate::rules::parse::parse_proposition_from_str;

pub fn dispatch(cmd: &RulesCommand, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    match cmd {
        RulesCommand::Check { yaml_path } => check(yaml_path, home_dir, fx),
        RulesCommand::Add { yaml_path } => add(yaml_path, home_dir, fx),
        RulesCommand::New { yaml_path } => new_rules(yaml_path, fx),
        RulesCommand::Guide => guide(fx),
        RulesCommand::Test {
            yaml_path,
            fake_home,
            expect_failure_messages,
            expect_num_failures,
        } => test(
            yaml_path,
            fake_home,
            expect_failure_messages,
            expect_num_failures,
            fx,
        ),
    }
}

fn load_proposition(yaml_path: &str) -> Result<crate::rules::ast::Proposition> {
    let content = std::fs::read_to_string(yaml_path)
        .with_context(|| format!("Cannot read rules file: {}", yaml_path))?;
    if content.trim().is_empty() {
        bail!(
            "Rules file is empty: {}\n\
             Use `key rules new {}` to create a valid empty rules file, \
             then add rules with `key rules add {}`.",
            yaml_path,
            yaml_path,
            yaml_path
        );
    }
    parse_proposition_from_str(&content)
        .with_context(|| format!("Invalid rules file: {}", yaml_path))
}

fn new_rules(yaml_path: &str, fx: &dyn Effects) -> Result<()> {
    if std::path::Path::new(yaml_path).exists() {
        bail!("File already exists: {}", yaml_path);
    }
    let empty = crate::rules::generate::generate_proposition_string(
        &crate::rules::ast::Proposition::All(vec![]),
    );
    std::fs::write(yaml_path, &empty)
        .with_context(|| format!("Cannot write rules file: {}", yaml_path))?;
    fx.println(&format!("Created empty rules file: {}", yaml_path));
    Ok(())
}

fn check(yaml_path: &str, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let prop = load_proposition(yaml_path)?;
    match evaluate(&prop, home_dir) {
        Ok(()) => {
            fx.println("All checks passed.");
            Ok(())
        }
        Err(failures) => {
            for f in &failures {
                fx.eprintln(&format!("FAIL {}", f));
            }
            bail!("{} check(s) failed.", failures.len());
        }
    }
}

fn guide(fx: &dyn Effects) -> Result<()> {
    let text = crate::rules::scenario_guide::render_guide();
    fx.println(&text);
    Ok(())
}

fn add(yaml_path: &str, _home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let new_prop = crate::rules::interactive::run_interactive_add(fx)?;

    let yaml_str = if std::path::Path::new(yaml_path).exists() {
        let existing_content = std::fs::read_to_string(yaml_path)
            .with_context(|| format!("Cannot read rules file: {}", yaml_path))?;
        let existing = parse_proposition_from_str(&existing_content)
            .with_context(|| format!("Invalid existing rules file: {}", yaml_path))?;

        // Merge: if existing is All, append; otherwise wrap both in All
        let merged = match existing {
            crate::rules::ast::Proposition::All(mut items) => {
                items.push(new_prop);
                crate::rules::ast::Proposition::All(items)
            }
            other => crate::rules::ast::Proposition::All(vec![other, new_prop]),
        };
        crate::rules::generate::generate_proposition_string(&merged)
    } else {
        crate::rules::generate::generate_proposition_string(&new_prop)
    };

    std::fs::write(yaml_path, &yaml_str)
        .with_context(|| format!("Cannot write rules file: {}", yaml_path))?;
    fx.println(&format!("Rule written to {}", yaml_path));
    Ok(())
}

fn test(
    yaml_path: &str,
    fake_home: &str,
    expect_failure_messages: &[String],
    expect_num_failures: &Option<usize>,
    fx: &dyn Effects,
) -> Result<()> {
    let prop = load_proposition(yaml_path)?;
    let home = Path::new(fake_home);
    if !home.is_dir() {
        bail!("Fake home directory does not exist: {}", fake_home);
    }

    let expect_pass = expect_failure_messages.is_empty() && expect_num_failures.is_none();

    match evaluate(&prop, home) {
        Ok(()) => {
            if expect_pass {
                fx.println("All checks passed.");
                Ok(())
            } else {
                bail!(
                    "Expected failures but all checks passed. \
                     Use without --expect-failure-message / --expect-failures \
                     if the rules should pass."
                );
            }
        }
        Err(failures) => {
            if expect_pass {
                for f in &failures {
                    fx.eprintln(&format!("FAIL {}", f));
                }
                bail!("{} check(s) failed.", failures.len());
            }

            // Check expected number of failures
            if let Some(expected_count) = expect_num_failures {
                if failures.len() != *expected_count {
                    for f in &failures {
                        fx.eprintln(&format!("FAIL {}", f));
                    }
                    bail!(
                        "Expected {} failure(s) but got {}.",
                        expected_count,
                        failures.len()
                    );
                }
            }

            // Check expected failure messages
            let all_output: String = failures
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            for expected_msg in expect_failure_messages {
                if !all_output.contains(expected_msg.as_str()) {
                    for f in &failures {
                        fx.eprintln(&format!("FAIL {}", f));
                    }
                    bail!(
                        "Expected failure output to contain {:?} but it did not.\nActual failures:\n{}",
                        expected_msg,
                        all_output
                    );
                }
            }

            fx.println(&format!(
                "Test passed: {} expected failure(s) confirmed.",
                failures.len()
            ));
            Ok(())
        }
    }
}
