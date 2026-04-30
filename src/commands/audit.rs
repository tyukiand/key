use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::cli::{AuditCommand, ProjectCommand};
use crate::effects::Effects;
use crate::rules::ast::{ControlFile, RuleFailure, TestExpectation};
use crate::rules::evaluate::{evaluate, evaluate_with_ctx};
use crate::rules::fixture::parse_fixture_collect_warnings;
use crate::rules::generate::{generate_control_file, generate_test_file};
use crate::rules::parse::{parse_control_file, parse_test_file};
use crate::rules::pseudo::EvalContext;

pub fn dispatch(cmd: &AuditCommand, home_dir: &Path, fx: &dyn Effects) -> Result<()> {
    match cmd {
        AuditCommand::Run {
            file,
            ignore,
            warn_only,
        } => run_audit(file, home_dir, ignore, warn_only, fx),
        AuditCommand::New { yaml_path } => new_audit(yaml_path, fx),
        AuditCommand::Add { yaml_path } => add_control(yaml_path, fx),
        AuditCommand::Guide {
            verbose,
            feature,
            emit_project,
        } => guide(*verbose, feature.as_deref(), emit_project.as_deref(), fx),
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

/// Spec/0012 §3.1 — when a typed id is unknown, propose a short canonical id
/// reachable by stripping a `-`-separated prefix and/or suffix. The prior
/// spec text says "suffix"; the §4.3 test cases (`predicate-shell-exports-
/// variable` → `shell-exports`) require trimming from both ends, so we walk
/// every contiguous `-`-separated subsequence and pick the longest known
/// match (ties broken by preferring the rightmost match — this keeps the
/// leaf-most term, e.g. `pseudo-file-env` → `env`, not `file`). No
/// fuzzy-matching crate.
fn suggest_canonical_id(typed: &str) -> Option<&'static str> {
    use crate::guide_edsl::features::Feature;
    let known: std::collections::BTreeSet<&'static str> =
        Feature::all().iter().map(|f| f.canonical_id()).collect();

    let segments: Vec<&str> = typed.split('-').collect();
    let n = segments.len();
    let mut best: Option<(usize, usize, &'static str)> = None; // (length, end-index, id)
    for start in 0..n {
        for end in (start + 1)..=n {
            // Skip the full original id — only proper subsequences count.
            if start == 0 && end == n {
                continue;
            }
            let candidate = segments[start..end].join("-");
            if let Some(id) = known.get(candidate.as_str()) {
                let length = end - start;
                let better = match best {
                    None => true,
                    Some((bl, be, _)) => length > bl || (length == bl && end > be),
                };
                if better {
                    best = Some((length, end, *id));
                }
            }
        }
    }
    best.map(|(_, _, id)| id)
}

fn guide(
    verbose: bool,
    feature_id: Option<&str>,
    emit_project_dir: Option<&str>,
    fx: &dyn Effects,
) -> Result<()> {
    use crate::guide_edsl::features::Feature;
    let mode = if verbose {
        crate::guide_edsl::text::Mode::Verbose
    } else {
        crate::guide_edsl::text::Mode::Terse
    };
    let tree = crate::guide_edsl::tree::root();

    let to_render = if let Some(id) = feature_id {
        let target = Feature::from_canonical_id(id).ok_or_else(|| {
            // Spec/0011 §B.4 — unknown id exits non-zero with a clear error
            // listing at least the root feature ids.
            let mut roots: Vec<&'static str> = Feature::all()
                .iter()
                .filter(|f| f.parent().is_none())
                .map(|f| f.canonical_id())
                .collect();
            roots.sort();
            // Spec/0012 §3.1 — did-you-mean: if the typed id has a known
            // canonical id as a suffix after stripping a `-`-separated prefix
            // (e.g. `pseudo-file-env` → strip `pseudo-file-` → suffix `env`
            // matches), suggest the matched short id. No fuzzy-matching crate.
            let hint = suggest_canonical_id(id);
            let hint_line = match hint {
                Some(s) => format!("did you mean: {}?\n", s),
                None => String::new(),
            };
            anyhow::anyhow!(
                "unknown --feature=<id>: {:?}\n\
                 {}\
                 Valid root feature ids:\n  {}\n\
                 (descendant ids are also accepted; pass `-v` to see the full guide.)",
                id,
                hint_line,
                roots.join("\n  "),
            )
        })?;
        match crate::guide_edsl::filter::filter_tree(&tree, target) {
            Some(t) => t,
            None => bail!(
                "feature {:?} (id {}) had no documented content — this is a guide bug.",
                target.name(),
                target.canonical_id(),
            ),
        }
    } else {
        tree
    };

    if let Some(out_dir) = emit_project_dir {
        // Spec/0013 §B.1 — materialize a full audit-project layout under
        // <out_dir> from the same EDSL tree the guide renders.
        let summary =
            crate::guide_edsl::emit_project::emit_project(&to_render, mode, Path::new(out_dir))?;
        fx.println(&format!(
            "Emitted audit project to {} \
             ({} control(s), {} fixture(s), {} test entry(ies))",
            out_dir, summary.control_count, summary.fixture_count, summary.test_count,
        ));
        return Ok(());
    }

    let text = crate::guide_edsl::text::render(&to_render, mode);
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

            // Spec/0013 §B.1 — load pseudo-file overrides from the fixture
            // dir if `pseudo-file-overrides.yaml` is present. Without
            // overrides, fall through to the legacy real-env behavior.
            let overrides_path = fixture_dir.join("pseudo-file-overrides.yaml");
            let eval_result = if overrides_path.is_file() {
                let yaml = std::fs::read_to_string(&overrides_path)
                    .with_context(|| format!("reading {}", overrides_path.display()))?;
                let (fixture, _warnings) = parse_fixture_collect_warnings(&yaml)
                    .with_context(|| format!("parsing {}", overrides_path.display()))?;
                let ctx = EvalContext::with_fixture(fixture_dir.clone(), fixture);
                evaluate_with_ctx(&control.check, &ctx)
            } else {
                evaluate(&control.check, &fixture_dir)
            };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide_edsl::features::Feature;

    /// Spec/0012 §3.1 / §4.3 — did-you-mean: stripped-prefix-and-suffix
    /// substring of the typed id matches a known short canonical id.
    #[test]
    fn did_you_mean_for_old_ids() {
        assert_eq!(suggest_canonical_id("pseudo-file-env"), Some("env"));
        assert_eq!(
            suggest_canonical_id("pseudo-file-executable"),
            Some("executable")
        );
        assert_eq!(suggest_canonical_id("proposition-forall"), Some("forall"));
        assert_eq!(
            suggest_canonical_id("predicate-shell-exports-variable"),
            Some("shell-exports"),
        );
        assert_eq!(
            suggest_canonical_id("predicate-shell-defines-variable"),
            Some("shell-defines"),
        );
        assert_eq!(suggest_canonical_id("cli-audit-run"), Some("audit-run"));
        assert_eq!(
            suggest_canonical_id("test-fixture-env-override"),
            Some("env-override")
        );
    }

    /// Spec/0012 §3.1 — unrelated typo / no `-`-separated subsequence
    /// match → no hint.
    #[test]
    fn did_you_mean_returns_none_for_unrelated() {
        assert_eq!(suggest_canonical_id("totally-bogus"), None);
        assert_eq!(suggest_canonical_id("xyz"), None);
        // The full id is excluded (we only suggest a *different* short id).
        assert_eq!(suggest_canonical_id("env"), None);
    }

    /// Spec/0012 §4.3 — the unknown-id error path itself emits the hint.
    /// We exercise this via the `guide()` function with a fake Effects.
    #[test]
    fn unknown_feature_id_error_contains_hint() {
        use crate::effects::CannedEffects;
        let fx = CannedEffects::new();
        let err = guide(false, Some("pseudo-file-env"), None, &fx).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("did you mean: env?"),
            "expected did-you-mean hint, got:\n{}",
            msg
        );
    }

    /// Spec/0012 §4.3 — `predicate-shell-exports-variable` and
    /// `proposition-forall` also produce hints.
    #[test]
    fn unknown_feature_id_error_contains_hint_for_other_old_ids() {
        use crate::effects::CannedEffects;
        let fx = CannedEffects::new();

        let err = guide(false, Some("predicate-shell-exports-variable"), None, &fx).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("did you mean: shell-exports?"),
            "expected hint shell-exports, got:\n{}",
            msg
        );

        let err = guide(false, Some("proposition-forall"), None, &fx).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("did you mean: forall?"),
            "expected hint forall, got:\n{}",
            msg
        );
    }

    /// Spec/0012 §4.2 — every renamed id is reachable via `--feature=<id>`
    /// and produces non-trivial output (functional regression). Combined
    /// with the round-trip test in features.rs, this confirms the rename
    /// did not break the filter pipeline.
    #[test]
    fn every_new_id_is_reachable_via_feature_flag() {
        use crate::effects::CannedEffects;
        for f in Feature::all() {
            let fx = CannedEffects::new();
            guide(false, Some(f.canonical_id()), None, &fx)
                .unwrap_or_else(|e| panic!("--feature={} failed: {:#}", f.canonical_id(), e));
            // Some output was emitted (terse drilled-in tree is non-empty).
            assert!(
                !fx.output_lines().is_empty(),
                "--feature={} produced no output",
                f.canonical_id(),
            );
        }
    }

    /// Spec/0012 §4.4 — terse rerun lines now contain the new short ids,
    /// not the old `category-prefix-name` form.
    #[test]
    fn terse_rerun_lines_use_new_short_ids() {
        let r = crate::guide_edsl::tree::root();
        let terse = crate::guide_edsl::text::render(&r, crate::guide_edsl::text::Mode::Terse);
        // New ids are present in some rerun line.
        assert!(
            terse.contains("--feature=env"),
            "expected --feature=env in terse output:\n{}",
            terse
        );
        // No old `pseudo-file-` / `predicate-shell-` / `cli-audit-` ids leak.
        for stale in &[
            "--feature=pseudo-file-env",
            "--feature=pseudo-file-executable",
            "--feature=proposition-forall",
            "--feature=predicate-shell-exports-variable",
            "--feature=cli-audit-run",
        ] {
            assert!(
                !terse.contains(stale),
                "stale id {:?} leaked into terse output:\n{}",
                stale,
                terse
            );
        }
    }
}
