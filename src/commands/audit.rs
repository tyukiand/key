use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::cli::{AuditCommand, ProjectCommand};
use crate::effects::Effects;
use crate::rules::ast::{ControlFile, RuleFailure, TestExpectation};
use crate::rules::evaluate::evaluate;
use crate::rules::generate::{generate_control_file, generate_test_file};
use crate::rules::parse::{parse_control_file, parse_test_file};

pub fn dispatch(cmd: &AuditCommand, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    match cmd {
        AuditCommand::Run {
            file,
            ignore,
            warn_only,
        } => run_audit(file, home_dir, ignore, warn_only, fx),
        AuditCommand::New { yaml_path } => new_audit(yaml_path, fx),
        AuditCommand::Add { yaml_path } => add_control(yaml_path, fx),
        AuditCommand::Guide => guide(fx),
        AuditCommand::Test {
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
        AuditCommand::List { yaml_path, short } => list_controls(yaml_path, *short, fx),
        AuditCommand::Delete { file, id } => delete_control(file, id.as_deref(), fx),
        AuditCommand::Install { yaml_path } => install_config(yaml_path, home_dir, fx),
        AuditCommand::Project(_) => unreachable!("handled in main.rs"),
    }
}

fn load_control_file(yaml_path: &str) -> Result<ControlFile> {
    let content = std::fs::read_to_string(yaml_path)
        .with_context(|| format!("Cannot read audit file: {}", yaml_path))?;
    if content.trim().is_empty() {
        bail!(
            "Audit file is empty: {}\n\
             Use `key audit new {}` to create a valid empty audit file, \
             then add controls with `key audit add {}`.",
            yaml_path,
            yaml_path,
            yaml_path
        );
    }
    parse_control_file(&content).with_context(|| format!("Invalid audit file: {}", yaml_path))
}

fn new_audit(yaml_path: &str, fx: &dyn Effects) -> Result<()> {
    if std::path::Path::new(yaml_path).exists() {
        bail!("File already exists: {}", yaml_path);
    }
    let empty = generate_control_file(&ControlFile { controls: vec![] });
    std::fs::write(yaml_path, &empty)
        .with_context(|| format!("Cannot write audit file: {}", yaml_path))?;
    fx.println(&format!("Created empty audit file: {}", yaml_path));
    Ok(())
}

fn run_audit(
    yaml_path: &str,
    home_dir: &Path,
    ignore: &[String],
    warn_only: &[String],
    fx: &dyn Effects,
) -> Result<()> {
    let cf = load_control_file(yaml_path)?;
    let mut total_pass = 0usize;
    let mut total_fail = 0usize;
    let mut total_warn = 0usize;
    let mut has_failure = false;

    for control in &cf.controls {
        if ignore.contains(&control.id) {
            continue;
        }
        let is_warn = warn_only.contains(&control.id);
        match evaluate(&control.check, home_dir) {
            Ok(()) => {
                total_pass += 1;
                fx.println(&format!(
                    "\x1b[32m[PASS]\x1b[0m {} \u{2014} {}",
                    control.id, control.title
                ));
            }
            Err(failures) => {
                if is_warn {
                    total_warn += 1;
                    fx.println(&format!(
                        "\x1b[33m[WARN]\x1b[0m {} \u{2014} {}",
                        control.id, control.title
                    ));
                } else {
                    total_fail += 1;
                    has_failure = true;
                    fx.println(&format!(
                        "\x1b[31m[FAIL]\x1b[0m {} \u{2014} {}",
                        control.id, control.title
                    ));
                }
                fx.println(&format!("       Description: {}", control.description));
                fx.println(&format!("       Remediation: {}", control.remediation));
                for f in &failures {
                    fx.println(&format!("       - {}", f));
                }
            }
        }
    }

    fx.println(&format!(
        "\n{} passed, {} failed, {} warnings",
        total_pass, total_fail, total_warn
    ));

    if has_failure {
        bail!("{} control(s) failed.", total_fail);
    }
    Ok(())
}

fn guide(fx: &dyn Effects) -> Result<()> {
    let text = crate::rules::scenario_guide::render_guide();
    fx.println(&text);
    Ok(())
}

fn add_control(yaml_path: &str, fx: &dyn Effects) -> Result<()> {
    let new_control = crate::rules::interactive::run_interactive_add_control(fx)?;

    let cf = if std::path::Path::new(yaml_path).exists() {
        let mut existing = load_control_file(yaml_path)?;
        // Check ID uniqueness
        for c in &existing.controls {
            if c.id == new_control.id {
                bail!(
                    "Control ID {:?} already exists in {}",
                    new_control.id,
                    yaml_path
                );
            }
        }
        existing.controls.push(new_control);
        existing
    } else {
        ControlFile {
            controls: vec![new_control],
        }
    };

    let yaml_str = generate_control_file(&cf);
    std::fs::write(yaml_path, &yaml_str)
        .with_context(|| format!("Cannot write audit file: {}", yaml_path))?;
    fx.println(&format!("Control written to {}", yaml_path));
    Ok(())
}

fn test(
    yaml_path: &str,
    fake_home: &str,
    expect_failure_messages: &[String],
    expect_num_failures: &Option<usize>,
    fx: &dyn Effects,
) -> Result<()> {
    let cf = load_control_file(yaml_path)?;
    let home = Path::new(fake_home);
    if !home.is_dir() {
        bail!("Fake home directory does not exist: {}", fake_home);
    }

    let expect_pass = expect_failure_messages.is_empty() && expect_num_failures.is_none();

    // Evaluate all controls, collecting all failures
    let mut all_failures: Vec<RuleFailure> = Vec::new();
    for control in &cf.controls {
        if let Err(failures) = evaluate(&control.check, home) {
            all_failures.extend(failures);
        }
    }

    if all_failures.is_empty() {
        if expect_pass {
            fx.println("All checks passed.");
            Ok(())
        } else {
            bail!(
                "Expected failures but all checks passed. \
                 Use without --expect-failure-message / --expect-failures \
                 if the controls should pass."
            );
        }
    } else {
        if expect_pass {
            for f in &all_failures {
                fx.eprintln(&format!("FAIL {}", f));
            }
            bail!("{} check(s) failed.", all_failures.len());
        }

        // Check expected number of failures
        if let Some(expected_count) = expect_num_failures {
            if all_failures.len() != *expected_count {
                for f in &all_failures {
                    fx.eprintln(&format!("FAIL {}", f));
                }
                bail!(
                    "Expected {} failure(s) but got {}.",
                    expected_count,
                    all_failures.len()
                );
            }
        }

        // Check expected failure messages
        let all_output: String = all_failures
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        for expected_msg in expect_failure_messages {
            if !all_output.contains(expected_msg.as_str()) {
                for f in &all_failures {
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
            all_failures.len()
        ));
        Ok(())
    }
}

fn list_controls(yaml_path: &str, short: bool, fx: &dyn Effects) -> Result<()> {
    let cf = load_control_file(yaml_path)?;
    if cf.controls.is_empty() {
        fx.println("No controls defined.");
        return Ok(());
    }
    for control in &cf.controls {
        if short {
            fx.println(&format!("[{}] {}", control.id, control.title));
        } else {
            fx.println(&format!("[{}] {}", control.id, control.title));
            fx.println(&format!("  Description: {}", control.description));
            fx.println(&format!("  Remediation: {}", control.remediation));
            fx.println("");
        }
    }
    Ok(())
}

fn delete_control(yaml_path: &str, id: Option<&str>, fx: &dyn Effects) -> Result<()> {
    let mut cf = load_control_file(yaml_path)?;
    if cf.controls.is_empty() {
        bail!("No controls to delete in {}", yaml_path);
    }

    let idx = match id {
        Some(id_str) => cf
            .controls
            .iter()
            .position(|c| c.id == id_str)
            .ok_or_else(|| anyhow::anyhow!("Control ID {:?} not found in {}", id_str, yaml_path))?,
        None => {
            let items: Vec<String> = cf
                .controls
                .iter()
                .map(|c| format!("[{}] {}", c.id, c.title))
                .collect();
            fx.pick_from_list("Select control to delete", &items)?
        }
    };

    let removed = cf.controls.remove(idx);
    let yaml_str = generate_control_file(&cf);
    std::fs::write(yaml_path, &yaml_str)
        .with_context(|| format!("Cannot write audit file: {}", yaml_path))?;
    fx.println(&format!(
        "Deleted control: [{}] {}",
        removed.id, removed.title
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 4: install + pick
// ---------------------------------------------------------------------------

pub fn install_config(yaml_path: &str, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    // Validate the file parses
    let _cf = load_control_file(yaml_path)?;

    let configs_dir = home_dir.join(".key/audit-configs");
    std::fs::create_dir_all(&configs_dir)
        .with_context(|| format!("Cannot create {}", configs_dir.display()))?;

    let file_name = std::path::Path::new(yaml_path)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine file name from: {}", yaml_path))?;
    let dest = configs_dir.join(file_name);
    std::fs::copy(yaml_path, &dest)
        .with_context(|| format!("Cannot copy to {}", dest.display()))?;

    fx.println(&format!("Installed: {}", dest.display()));
    Ok(())
}

pub fn dispatch_pick(home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let configs_dir = home_dir.join(".key/audit-configs");
    if !configs_dir.is_dir() {
        bail!("No audit configs installed. Use `key audit install <file.yaml>` first.");
    }

    let mut yamls: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&configs_dir)
        .with_context(|| format!("Cannot read {}", configs_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".yaml") || name.ends_with(".yml") {
            yamls.push(name);
        }
    }
    yamls.sort();

    if yamls.is_empty() {
        bail!(
            "No audit configs found in {}. Use `key audit install <file.yaml>` first.",
            configs_dir.display()
        );
    }

    let idx = fx.pick_from_list("Select audit config to run", &yamls)?;
    let chosen = configs_dir.join(&yamls[idx]);
    let chosen_str = chosen.to_string_lossy().to_string();
    run_audit(&chosen_str, home_dir, &[], &[], fx)
}

// ---------------------------------------------------------------------------
// Phase 5: project commands
// ---------------------------------------------------------------------------

pub fn dispatch_project(cmd: &ProjectCommand, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    match cmd {
        ProjectCommand::New { name } => project_new(name, fx),
        ProjectCommand::Test => project_test(&std::env::current_dir()?, fx),
        ProjectCommand::Build => project_build(&std::env::current_dir()?, fx),
        ProjectCommand::Clean => project_clean(&std::env::current_dir()?, fx),
        ProjectCommand::Run { home } => {
            let home_path = match home {
                Some(h) => PathBuf::from(h),
                None => home_dir.to_path_buf(),
            };
            project_run(&std::env::current_dir()?, &home_path, fx)
        }
    }
}

pub fn project_new(name: &str, fx: &dyn Effects) -> Result<()> {
    // Validate name
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!(
            "Invalid project name: {:?}. Must be simple, no path separators.",
            name
        );
    }

    let base = PathBuf::from(name);
    if base.exists() {
        bail!("Directory already exists: {}", name);
    }

    let main_dir = base.join("src/main");
    let test_dir = base.join("src/test");
    let resources_dir = test_dir.join("resources");

    std::fs::create_dir_all(&main_dir)
        .with_context(|| format!("Cannot create {}", main_dir.display()))?;
    std::fs::create_dir_all(&resources_dir)
        .with_context(|| format!("Cannot create {}", resources_dir.display()))?;

    // Write empty control file
    let empty_cf = generate_control_file(&ControlFile { controls: vec![] });
    let cf_path = main_dir.join(format!("{}.yaml", name));
    std::fs::write(&cf_path, &empty_cf)
        .with_context(|| format!("Cannot write {}", cf_path.display()))?;

    // Write empty test file
    let empty_tf = generate_test_file(&crate::rules::ast::TestFile {
        test_suites: vec![],
    });
    let tf_path = test_dir.join("tests.yaml");
    std::fs::write(&tf_path, &empty_tf)
        .with_context(|| format!("Cannot write {}", tf_path.display()))?;

    // Write .gitignore
    std::fs::write(base.join(".gitignore"), "target/\n")
        .with_context(|| "Cannot write .gitignore")?;

    fx.println(&format!("Created audit project: {}", name));
    Ok(())
}

/// Find the single main YAML control file in src/main/.
fn find_main_yaml(project_dir: &Path) -> Result<PathBuf> {
    let main_dir = project_dir.join("src/main");
    if !main_dir.is_dir() {
        bail!("Not an audit project: src/main/ not found");
    }
    let mut yamls = Vec::new();
    for entry in std::fs::read_dir(&main_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".yaml") || name.ends_with(".yml") {
            yamls.push(entry.path());
        }
    }
    if yamls.is_empty() {
        bail!("No YAML file found in src/main/");
    }
    if yamls.len() > 1 {
        bail!(
            "Expected exactly one YAML file in src/main/, found {}",
            yamls.len()
        );
    }
    Ok(yamls.into_iter().next().unwrap())
}

pub fn project_test(project_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let main_yaml = find_main_yaml(project_dir)?;
    let cf = load_control_file(&main_yaml.to_string_lossy())?;

    let tests_path = project_dir.join("src/test/tests.yaml");
    let tests_content = std::fs::read_to_string(&tests_path)
        .with_context(|| format!("Cannot read {}", tests_path.display()))?;
    let tf = parse_test_file(&tests_content)
        .with_context(|| format!("Invalid test file: {}", tests_path.display()))?;

    let resources_dir = project_dir.join("src/test/resources");
    let mut total_pass = 0usize;
    let mut total_fail = 0usize;

    for suite in &tf.test_suites {
        if let Some(ref desc) = suite.description {
            fx.println(&format!("Suite: {} — {}", suite.name, desc));
        } else {
            fx.println(&format!("Suite: {}", suite.name));
        }

        for tc in &suite.tests {
            // Find control by ID
            let control = cf
                .controls
                .iter()
                .find(|c| c.id == tc.control_id)
                .ok_or_else(|| {
                    let available: Vec<&str> = cf.controls.iter().map(|c| c.id.as_str()).collect();
                    anyhow::anyhow!(
                        "Test references unknown control ID {:?}; available: {}",
                        tc.control_id,
                        available.join(", ")
                    )
                })?;

            // Resolve fixture
            let fixture_dir = resources_dir.join(&tc.fixture);
            if !fixture_dir.is_dir() {
                bail!(
                    "Fixture {:?} not found in {}",
                    tc.fixture,
                    resources_dir.display()
                );
            }

            // Evaluate control against fixture
            let eval_result = evaluate(&control.check, &fixture_dir);

            // Compare against expectation
            match (&tc.expect, &eval_result) {
                (TestExpectation::Pass, Ok(())) => {
                    // Case 1: expected pass, got pass
                    total_pass += 1;
                    fx.println(&format!("  [PASS] {} — {}", tc.control_id, tc.description));
                }
                (TestExpectation::Pass, Err(failures)) => {
                    // Case 2: expected pass but control reported failures (FP)
                    total_fail += 1;
                    fx.println(&format!(
                        "  [FAIL] {} on fixture {}: expected pass but control reported {} failure(s):",
                        tc.control_id, tc.fixture, failures.len()
                    ));
                    for f in failures {
                        fx.println(&format!("    - {}", f));
                    }
                }
                (TestExpectation::Fail(_), Ok(())) => {
                    // Case 3: expected failure but control passed (FN)
                    total_fail += 1;
                    fx.println(&format!(
                        "  [FAIL] {} on fixture {}: expected failure but control passed",
                        tc.control_id, tc.fixture
                    ));
                }
                (TestExpectation::Fail(fail_exp), Err(failures)) => {
                    // Case 4: expected failure, got failure — check details
                    let mut case_ok = true;

                    if let Some(expected_count) = fail_exp.count {
                        if failures.len() != expected_count {
                            total_fail += 1;
                            case_ok = false;
                            fx.println(&format!(
                                "  [FAIL] {} on fixture {}: expected {} failure(s) but got {}",
                                tc.control_id,
                                tc.fixture,
                                expected_count,
                                failures.len()
                            ));
                        }
                    }

                    if case_ok && !fail_exp.messages.is_empty() {
                        let all_output: String = failures
                            .iter()
                            .map(|f| f.to_string())
                            .collect::<Vec<_>>()
                            .join("\n");
                        for expected_msg in &fail_exp.messages {
                            if !all_output.contains(expected_msg.as_str()) {
                                total_fail += 1;
                                case_ok = false;
                                fx.println(&format!(
                                    "  [FAIL] {} on fixture {}: expected failure containing {:?} but not found in output",
                                    tc.control_id, tc.fixture, expected_msg
                                ));
                                break;
                            }
                        }
                    }

                    if case_ok {
                        total_pass += 1;
                        fx.println(&format!("  [PASS] {} — {}", tc.control_id, tc.description));
                    }
                }
            }
        }
    }

    fx.println(&format!("\n{} passed, {} failed", total_pass, total_fail));
    if total_fail > 0 {
        bail!("{} test(s) failed.", total_fail);
    }
    Ok(())
}

pub fn project_build(project_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let main_yaml = find_main_yaml(project_dir)?;

    // Parse-check main yaml
    let _cf = load_control_file(&main_yaml.to_string_lossy())?;

    // Parse-check tests.yaml
    let tests_path = project_dir.join("src/test/tests.yaml");
    if tests_path.exists() {
        let tests_content = std::fs::read_to_string(&tests_path)
            .with_context(|| format!("Cannot read {}", tests_path.display()))?;
        let _tf = parse_test_file(&tests_content)
            .with_context(|| format!("Invalid test file: {}", tests_path.display()))?;
    }

    // Run tests
    project_test(project_dir, fx)?;

    // Copy to target/
    let target_dir = project_dir.join("target");
    std::fs::create_dir_all(&target_dir)
        .with_context(|| format!("Cannot create {}", target_dir.display()))?;
    let dest = target_dir.join(main_yaml.file_name().unwrap());
    std::fs::copy(&main_yaml, &dest)
        .with_context(|| format!("Cannot copy to {}", dest.display()))?;

    fx.println(&format!("Build output: {}", dest.display()));
    Ok(())
}

pub fn project_clean(project_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let target_dir = project_dir.join("target");
    if target_dir.is_dir() {
        std::fs::remove_dir_all(&target_dir)
            .with_context(|| format!("Cannot remove {}", target_dir.display()))?;
        fx.println("Cleaned target/");
    } else {
        fx.println("Nothing to clean.");
    }
    Ok(())
}

pub fn project_run(project_dir: &Path, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    let main_yaml = find_main_yaml(project_dir)?;
    let yaml_str = main_yaml.to_string_lossy().to_string();
    run_audit(&yaml_str, home_dir, &[], &[], fx)
}
