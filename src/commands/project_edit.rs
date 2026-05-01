//! `key audit project edit <dir>` — fdisk-style interactive REPL.
//!
//! Spec/0016 §B.2-§B.3. Loads a Project, presents a top-level Menu::Pick
//! whose tags are the 12 kebab-case command tokens, drives sub-Interactions
//! for the Add*/Delete* dialogs, and writes the Project back on `save`.
//!
//! The REPL is implemented as a flat imperative loop over the
//! `Interaction<LowLevelInput, _>` primitives from spec/0015 (`ask_pick`,
//! `ask_free`, `ask_yesno`). Each iteration:
//!   - prompts for a top-level command (Menu::Pick),
//!   - dispatches to a sub-dialog returning a new Project (mutators) or
//!     to an observational op (run-tests, run-audit, save, quit).
//!
//! See spec/0016 §B.5 for the round-trip extension: every observational op
//! is a no-op on the Project state, so the REPL transcript round-trips
//! through the ProjectMutation layer ignoring report ops.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::effects::{Effects, OsEffectsRw};
use crate::interaction::{
    ask_free, ask_pick, ask_yesno, FreeKind, LowLevelInput, Menu, MenuOption, Step,
};
use crate::project::{ControlFileName, FixtureFile, FixtureFileName, Project, ProjectMutation};
use crate::rules::ast::{
    validate_control_id, Control, FilePredicateAst, Proposition, SimplePath, TestCase,
    TestExpectation,
};
use crate::security::unredacted::UnredactedMatcher;

/// Top-level entry point for `key audit project edit <dir>`.
pub fn project_edit(dir: &Path, fx: &dyn Effects, os: &dyn OsEffectsRw) -> Result<()> {
    let mut project = Project::load_from_dir(dir, fx)
        .with_context(|| format!("loading project at {}", dir.display()))?;
    let mut dirty = false;
    fx.println(&format!(
        "Editing project at {}. Type `help` for the menu.",
        dir.display()
    ));
    print_menu(fx);
    loop {
        let cmd = match prompt_pick(fx, "edit", top_level_options())? {
            Some(c) => c,
            None => {
                // EOF at top-level — treat as quit (no save).
                fx.println("(EOF — quitting without save)");
                return Ok(());
            }
        };
        match cmd.as_str() {
            "help" => print_menu(fx),
            "list" => print_project(fx, &project),
            "add-control" => match add_control_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "delete-control" => match delete_control_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "add-fixture" => match add_fixture_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "delete-fixture" => match delete_fixture_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "add-test-entry" => match add_test_entry_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "delete-test-entry" => match delete_test_entry_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "add-unredacted-matcher" => match add_unredacted_matcher_dialog(fx, project.clone())? {
                Some(p) => {
                    project = p;
                    dirty = true;
                }
                None => fx.println("(cancelled)"),
            },
            "delete-unredacted-matcher" => {
                match delete_unredacted_matcher_dialog(fx, project.clone())? {
                    Some(p) => {
                        project = p;
                        dirty = true;
                    }
                    None => fx.println("(cancelled)"),
                }
            }
            "run-tests" => print_tests_report(fx, &project, os),
            "run-audit" => print_audit_report(fx, &project),
            "save" => {
                project
                    .write_to_dir(dir, fx)
                    .with_context(|| format!("writing {}", dir.display()))?;
                fx.println(&format!("Saved to {}.", dir.display()));
                dirty = false;
            }
            "quit" => {
                if dirty {
                    let confirm = match prompt_yesno(fx, "discard unsaved changes? [y/N]")? {
                        Some(b) => b,
                        None => false,
                    };
                    if !confirm {
                        fx.println("(quit cancelled — type `save` to write, then `quit`.)");
                        continue;
                    }
                }
                fx.println("(quitting)");
                return Ok(());
            }
            other => {
                fx.println(&format!("unknown command: {}", other));
            }
        }
    }
}

fn top_level_options() -> Vec<MenuOption> {
    vec![
        MenuOption::new("help", "reprint the menu"),
        MenuOption::new("list", "show controls / fixtures / test entries"),
        MenuOption::new("add-control", "add a new control"),
        MenuOption::new("delete-control", "delete a control by id"),
        MenuOption::new("add-fixture", "add a new fixture"),
        MenuOption::new("delete-fixture", "delete a fixture by name"),
        MenuOption::new("add-test-entry", "add a test entry"),
        MenuOption::new("delete-test-entry", "delete a test entry"),
        MenuOption::new(
            "add-unredacted-matcher",
            "append a literal opt-out matcher (value or prefix)",
        ),
        MenuOption::new(
            "delete-unredacted-matcher",
            "remove a previously-added matcher",
        ),
        MenuOption::new("run-tests", "run in-memory tests"),
        MenuOption::new("run-audit", "run audit against host FS"),
        MenuOption::new("save", "write project to disk"),
        MenuOption::new("quit", "quit (asks if dirty)"),
    ]
}

fn print_menu(fx: &dyn Effects) {
    fx.println("");
    fx.println("Commands:");
    for opt in top_level_options() {
        fx.println(&format!("  {:<18} {}", opt.tag, opt.label));
    }
    fx.println("");
}

fn print_project(fx: &dyn Effects, p: &Project) {
    fx.println("Controls:");
    if p.controls.is_empty() {
        fx.println("  (none)");
    } else {
        for (file, cf) in &p.controls {
            for c in &cf.controls {
                fx.println(&format!("  {} (in {}.yaml)", c.id, file.as_str()));
            }
        }
    }
    fx.println("Fixtures:");
    if p.fixtures.is_empty() {
        fx.println("  (none)");
    } else {
        for name in p.fixtures.keys() {
            fx.println(&format!("  {}", name.as_str()));
        }
    }
    fx.println("Test entries:");
    if p.tests.inner.test_suites.is_empty() {
        fx.println("  (none)");
    } else {
        for suite in &p.tests.inner.test_suites {
            fx.println(&format!("  [{}]", suite.name));
            for tc in &suite.tests {
                fx.println(&format!(
                    "    {} on {} ({})",
                    tc.control_id,
                    tc.fixture,
                    match &tc.expect {
                        TestExpectation::Pass => "expect pass".to_string(),
                        TestExpectation::Fail(_) => "expect fail".to_string(),
                    }
                ));
            }
        }
    }
    fx.println("Unredacted matchers:");
    if p.unredacted.is_empty() {
        fx.println("  (none)");
    } else {
        for m in &p.unredacted {
            fx.println(&format!("  {}: {}", m.kind(), m.literal()));
        }
    }
}

fn print_tests_report(fx: &dyn Effects, p: &Project, os: &dyn OsEffectsRw) {
    match p.run_tests(os) {
        Ok(report) => {
            fx.println(&format!(
                "TestsReport: {} passed, {} failed",
                report.passed, report.failed
            ));
            for m in &report.failure_messages {
                fx.println(&format!("  - {}", m));
            }
        }
        Err(e) => fx.println(&format!("run-tests failed: {:#}", e)),
    }
}

fn print_audit_report(fx: &dyn Effects, p: &Project) {
    let home = match fx.home_dir() {
        Ok(h) => h,
        Err(e) => {
            fx.println(&format!("run-audit failed: {:#}", e));
            return;
        }
    };
    let report = p.run_audit_against_filesystem(Path::new(&home), &[], &[]);
    fx.println(&format!(
        "AuditReport: {} passed, {} failed, {} warned",
        report.passed, report.failed, report.warned
    ));
    for m in &report.failure_messages {
        fx.println(&format!("  - {}", m));
    }
}

// ---------------------------------------------------------------------------
// Sub-dialogs — each returns Ok(Some(new_project)) on completion,
// Ok(None) on user-cancellation, Err(...) on hard failure.
// ---------------------------------------------------------------------------

fn add_control_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    let file = match prompt_text(fx, "control-file (e.g. main)", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let cfn = match ControlFileName::new(&file) {
        Ok(n) => n,
        Err(e) => {
            fx.println(&format!("invalid control-file name: {}", e));
            return Ok(None);
        }
    };

    let id = match prompt_text(fx, "control id (e.g. CTRL-1)", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    if let Err(e) = validate_control_id(&id) {
        fx.println(&format!("invalid control id: {:#}", e));
        return Ok(None);
    }
    let title = match prompt_text(fx, "title", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let description = match prompt_text(fx, "description", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let remediation = match prompt_text(fx, "remediation", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let path = match prompt_text(fx, "path (e.g. ~/.ssh/config)", FreeKind::Path)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let path = match SimplePath::new(&path) {
        Ok(p) => p,
        Err(e) => {
            fx.println(&format!("invalid path: {}", e));
            return Ok(None);
        }
    };

    // Predicate: just file-exists for v1 (the EDSL guide spec/0016 §B.3 calls
    // for a full sub-dialog; this v1 shipping the file-exists predicate keeps
    // the new surface tractable while still demonstrating the round-trip).
    let pred = match prompt_pick(fx, "predicate", predicate_options())? {
        Some(s) => s,
        None => return Ok(None),
    };
    let check_pred = match pred.as_str() {
        "file-exists" => FilePredicateAst::FileExists,
        "text-matches" => {
            let re = match prompt_text(fx, "regex", FreeKind::Regex)? {
                Some(s) => s,
                None => return Ok(None),
            };
            FilePredicateAst::TextMatchesRegex(re)
        }
        "text-contains" => {
            let needle = match prompt_text(fx, "literal substring", FreeKind::Text)? {
                Some(s) => s,
                None => return Ok(None),
            };
            FilePredicateAst::TextContains(needle)
        }
        _ => return Ok(None),
    };

    let control = Control {
        id,
        title,
        description,
        remediation,
        check: Proposition::FileSatisfies {
            path,
            check: check_pred,
        },
    };
    match project.apply_mutation(ProjectMutation::AddControl { file: cfn, control }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("add-control failed: {}", e));
            Ok(None)
        }
    }
}

fn predicate_options() -> Vec<MenuOption> {
    vec![
        MenuOption::new("file-exists", "the file at <path> exists"),
        MenuOption::new("text-matches", "some line matches a regex"),
        MenuOption::new("text-contains", "the file contains a literal substring"),
    ]
}

fn delete_control_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    if project.controls.is_empty() {
        fx.println("(no controls to delete)");
        return Ok(None);
    }
    let opts: Vec<MenuOption> = project
        .controls
        .keys()
        .map(|k| MenuOption::new(k.as_str().to_string(), k.as_str().to_string()))
        .collect();
    let chosen = match prompt_pick(fx, "control-file to delete", opts)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let cfn = match ControlFileName::new(&chosen) {
        Ok(n) => n,
        Err(e) => bail!("invalid control file name: {}", e),
    };
    match project.apply_mutation(ProjectMutation::DeleteControl { file: cfn }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("delete-control failed: {}", e));
            Ok(None)
        }
    }
}

fn add_fixture_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    let name = match prompt_text(fx, "fixture name", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let fxn = match FixtureFileName::new(&name) {
        Ok(n) => n,
        Err(e) => {
            fx.println(&format!("invalid fixture name: {}", e));
            return Ok(None);
        }
    };
    match project.apply_mutation(ProjectMutation::AddFixture {
        name: fxn,
        fixture: FixtureFile::default(),
    }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("add-fixture failed: {}", e));
            Ok(None)
        }
    }
}

fn delete_fixture_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    if project.fixtures.is_empty() {
        fx.println("(no fixtures to delete)");
        return Ok(None);
    }
    let opts: Vec<MenuOption> = project
        .fixtures
        .keys()
        .map(|k| MenuOption::new(k.as_str().to_string(), k.as_str().to_string()))
        .collect();
    let chosen = match prompt_pick(fx, "fixture to delete", opts)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let fxn = match FixtureFileName::new(&chosen) {
        Ok(n) => n,
        Err(e) => bail!("invalid fixture name: {}", e),
    };
    match project.apply_mutation(ProjectMutation::DeleteFixture { name: fxn }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("delete-fixture failed: {}", e));
            Ok(None)
        }
    }
}

fn add_test_entry_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    let suite = match prompt_text(fx, "suite name", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let control_id = match prompt_text(fx, "control id", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let fixture = match prompt_text(fx, "fixture name", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let description = match prompt_text(fx, "description", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let expect_pass = match prompt_yesno(fx, "expect pass? [y/n]")? {
        Some(b) => b,
        None => return Ok(None),
    };
    let expect = if expect_pass {
        TestExpectation::Pass
    } else {
        TestExpectation::Fail(crate::rules::ast::FailExpectation {
            count: None,
            messages: vec![],
        })
    };
    match project.apply_mutation(ProjectMutation::AddTestEntry {
        suite,
        tc: TestCase {
            control_id,
            description,
            fixture,
            expect,
        },
    }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("add-test-entry failed: {}", e));
            Ok(None)
        }
    }
}

fn delete_test_entry_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    if project.tests.inner.test_suites.is_empty() {
        fx.println("(no test entries to delete)");
        return Ok(None);
    }
    let suite_opts: Vec<MenuOption> = project
        .tests
        .inner
        .test_suites
        .iter()
        .map(|s| MenuOption::new(s.name.clone(), s.name.clone()))
        .collect();
    let suite = match prompt_pick(fx, "suite", suite_opts)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let entry_opts: Vec<MenuOption> = project
        .tests
        .inner
        .test_suites
        .iter()
        .find(|s| s.name == suite)
        .map(|s| {
            s.tests
                .iter()
                .map(|tc| {
                    let tag = format!("{}::{}", tc.control_id, tc.fixture);
                    MenuOption::new(tag.clone(), format!("{} on {}", tc.control_id, tc.fixture))
                })
                .collect()
        })
        .unwrap_or_default();
    let chosen = match prompt_pick(fx, "entry to delete", entry_opts)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let mut parts = chosen.splitn(2, "::");
    let control_id = parts.next().unwrap_or("").to_string();
    let fixture = parts.next().unwrap_or("").to_string();
    match project.apply_mutation(ProjectMutation::DeleteTestEntry {
        suite,
        control_id,
        fixture,
    }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("delete-test-entry failed: {}", e));
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Spec/0017 §C.2 — Add/DeleteUnredactedMatcher dialogs.
// ---------------------------------------------------------------------------

fn matcher_kind_options() -> Vec<MenuOption> {
    vec![
        MenuOption::new("value", "exact-value literal opt-out"),
        MenuOption::new("prefix", "starts-with literal opt-out"),
    ]
}

fn add_unredacted_matcher_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    let kind = match prompt_pick(fx, "matcher kind", matcher_kind_options())? {
        Some(s) => s,
        None => return Ok(None),
    };
    let literal = match prompt_text(fx, "literal", FreeKind::Text)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let matcher = match kind.as_str() {
        "value" => UnredactedMatcher::value(literal),
        "prefix" => UnredactedMatcher::prefix(literal),
        _ => return Ok(None),
    };
    let matcher = match matcher {
        Ok(m) => m,
        Err(e) => {
            fx.println(&format!("invalid matcher: {}", e));
            return Ok(None);
        }
    };
    match project.apply_mutation(ProjectMutation::AddUnredactedMatcher { matcher }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("add-unredacted-matcher failed: {}", e));
            Ok(None)
        }
    }
}

fn delete_unredacted_matcher_dialog(fx: &dyn Effects, project: Project) -> Result<Option<Project>> {
    if project.unredacted.is_empty() {
        fx.println("(no unredacted matchers to delete)");
        return Ok(None);
    }
    let opts: Vec<MenuOption> = project
        .unredacted
        .iter()
        .map(|m| {
            let tag = format!("{}:{}", m.kind(), m.literal());
            MenuOption::new(tag.clone(), tag)
        })
        .collect();
    let chosen = match prompt_pick(fx, "matcher to delete", opts)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let mut parts = chosen.splitn(2, ':');
    let kind = parts.next().unwrap_or("");
    let literal = parts.next().unwrap_or("");
    let matcher = match kind {
        "value" => UnredactedMatcher::value(literal.to_string()),
        "prefix" => UnredactedMatcher::prefix(literal.to_string()),
        _ => return Ok(None),
    };
    let matcher = match matcher {
        Ok(m) => m,
        Err(e) => bail!("invalid matcher: {}", e),
    };
    match project.apply_mutation(ProjectMutation::DeleteUnredactedMatcher { matcher }) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            fx.println(&format!("delete-unredacted-matcher failed: {}", e));
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal-backed driver for the Interaction primitives. Each call:
//   1. constructs the Interaction (ask_pick / ask_free / ask_yesno),
//   2. takes its single Suspended step,
//   3. reads a line from `fx.prompt_text`,
//   4. translates to LowLevelInput per the Menu kind,
//   5. resumes and returns Done(value) on success.
// EOF on stdin returns Ok(None) — the caller decides whether that's a
// cancellation or a quit.
// ---------------------------------------------------------------------------

fn prompt_pick(fx: &dyn Effects, prompt: &str, options: Vec<MenuOption>) -> Result<Option<String>> {
    let interaction = ask_pick(prompt.to_string(), options.clone());
    let step = interaction.step();
    match step {
        Step::Suspended { menu, resume } => {
            let line = match read_line(fx, &menu)? {
                Some(s) => s,
                None => return Ok(None),
            };
            let input = if line.is_empty() {
                LowLevelInput::Lexical(line)
            } else if let Ok(i) = line.parse::<usize>() {
                LowLevelInput::Index(i)
            } else {
                LowLevelInput::Lexical(line)
            };
            match resume(input) {
                Step::Done(idx) => Ok(Some(options[idx].tag.clone())),
                Step::Failed(e) => {
                    fx.println(&format!("invalid input: {}", e));
                    Ok(None)
                }
                Step::Suspended { .. } => bail!("ask_pick yielded a second suspension"),
            }
        }
        Step::Done(idx) => Ok(Some(options[idx].tag.clone())),
        Step::Failed(e) => bail!("ask_pick failed: {}", e),
    }
}

fn prompt_text(fx: &dyn Effects, prompt: &str, kind: FreeKind) -> Result<Option<String>> {
    let interaction = ask_free(prompt.to_string(), kind);
    let step = interaction.step();
    match step {
        Step::Suspended { menu, resume } => {
            let line = match read_line(fx, &menu)? {
                Some(s) => s,
                None => return Ok(None),
            };
            match resume(LowLevelInput::Text(line)) {
                Step::Done(s) => Ok(Some(s)),
                Step::Failed(e) => bail!("free prompt failed: {}", e),
                Step::Suspended { .. } => bail!("ask_free yielded a second suspension"),
            }
        }
        Step::Done(s) => Ok(Some(s)),
        Step::Failed(e) => bail!("ask_free failed: {}", e),
    }
}

fn prompt_yesno(fx: &dyn Effects, prompt: &str) -> Result<Option<bool>> {
    let interaction = ask_yesno(prompt.to_string());
    let step = interaction.step();
    match step {
        Step::Suspended { menu, resume } => {
            let line = match read_line(fx, &menu)? {
                Some(s) => s,
                None => return Ok(None),
            };
            let input = match line.to_ascii_lowercase().as_str() {
                "y" | "yes" => LowLevelInput::Yes,
                "n" | "no" | "" => LowLevelInput::No,
                _ => LowLevelInput::No,
            };
            match resume(input) {
                Step::Done(b) => Ok(Some(b)),
                Step::Failed(e) => bail!("yes/no failed: {}", e),
                Step::Suspended { .. } => bail!("ask_yesno yielded a second suspension"),
            }
        }
        Step::Done(b) => Ok(Some(b)),
        Step::Failed(e) => bail!("ask_yesno failed: {}", e),
    }
}

fn read_line(fx: &dyn Effects, menu: &Menu) -> Result<Option<String>> {
    let prompt = match menu {
        Menu::Pick { prompt, options } => {
            // List the options inline so a fresh agent / user sees what's
            // available without typing `help`.
            let tags: Vec<String> = options.iter().map(|o| o.tag.clone()).collect();
            format!("{} [{}]", prompt, tags.join("/"))
        }
        Menu::Free { prompt, .. } => prompt.clone(),
        Menu::YesNo { prompt } => prompt.clone(),
        Menu::Confirm { prompt, .. } => prompt.clone(),
    };
    match fx.prompt_text(&prompt) {
        Ok(s) => Ok(Some(s)),
        Err(_) => Ok(None),
    }
}
