//! Spec/0016 §D — end-to-end test for `key audit project edit`.
//!
//! Loads an empty Project, drives the full Add-Control sub-dialog through
//! the project_edit menu, runs tests (passes, since no test entries),
//! saves, reloads from disk, and asserts the loaded Project state matches
//! the edits.
//!
//! Hermeticity: the user-facing FS uses CannedEffects (in-memory map);
//! Project::run_tests uses MockOsEffects (in-memory tempdir). Neither
//! touches the host FS.

#![cfg(feature = "testing")]

use std::path::Path;

use key::commands::project_edit::project_edit;
use key::effects::{CannedEffects, MockOsEffects};
use key::project::{ControlFileName, Project};

/// Build a CannedEffects pre-seeded with the answers for:
///   1. add-control flow (file=main, id=CTRL-1, …, predicate=file-exists)
///   2. run-tests
///   3. save
///   4. quit
fn seeded_canned() -> CannedEffects {
    CannedEffects::new().with_prompt_answers(vec![
        // Top-level: add-control
        "add-control".into(),
        // Sub-dialog: control-file
        "main".into(),
        // Control id
        "CTRL-1".into(),
        // Title
        "Sample Control".into(),
        // Description
        "Verifies the SSH config exists.".into(),
        // Remediation
        "Create ~/.ssh/config if it is missing.".into(),
        // Path
        "~/.ssh/config".into(),
        // Predicate (Pick menu — accepts lexical match on "file-exists")
        "file-exists".into(),
        // Top-level: run-tests (no test entries → trivially passes)
        "run-tests".into(),
        // Top-level: save
        "save".into(),
        // Top-level: quit (project is no longer dirty post-save, but the
        // `quit` token still terminates the loop cleanly).
        "quit".into(),
    ])
}

fn seed_empty_project(fx: &CannedEffects, dir: &Path) {
    use key::effects::Effects;
    fx.create_dir_all(&dir.join("src/main")).unwrap();
    fx.create_dir_all(&dir.join("src/test/resources")).unwrap();
    // Empty tests.yaml so load_from_dir succeeds without controls.
    fx.write_file(&dir.join("src/test/tests.yaml"), b"test-suites: []\n")
        .unwrap();
}

#[test]
fn project_edit_add_control_then_save_then_reload_round_trips() {
    let fx = seeded_canned();
    let os = MockOsEffects::new();
    let dir = Path::new("/proj");
    seed_empty_project(&fx, dir);

    project_edit(dir, &fx, &os).expect("project_edit must succeed");

    // Reload from the in-memory FS.
    let reloaded = Project::load_from_dir(dir, &fx).expect("reload");
    let main_cfn = ControlFileName::new("main").unwrap();
    let cf = reloaded
        .controls
        .get(&main_cfn)
        .expect("main.yaml must contain the saved control");
    assert_eq!(cf.controls.len(), 1, "exactly one control was added");
    let c = &cf.controls[0];
    assert_eq!(c.id, "CTRL-1");
    assert_eq!(c.title, "Sample Control");
    assert_eq!(c.description, "Verifies the SSH config exists.");
    assert_eq!(c.remediation, "Create ~/.ssh/config if it is missing.");

    // Check the predicate shape: file-exists on ~/.ssh/config.
    use key::rules::ast::{FilePredicateAst, Proposition};
    match &c.check {
        Proposition::FileSatisfies { path, check } => {
            assert_eq!(path.as_str(), "~/.ssh/config");
            assert!(matches!(check, FilePredicateAst::FileExists));
        }
        other => panic!("unexpected proposition shape: {:?}", other),
    }

    // The captured stdout must mention the save line, demonstrating that
    // the `save` op fired and reported success inline.
    let out = fx.output();
    assert!(
        out.contains("Saved to /proj"),
        "expected save confirmation in output, got:\n{}",
        out
    );

    // run-tests was reached: the TestsReport line must be present (0 tests
    // since no test entries were added — the report shows 0 passed/0 failed).
    assert!(
        out.contains("TestsReport: 0 passed, 0 failed"),
        "expected TestsReport line, got:\n{}",
        out
    );
}

#[test]
fn project_edit_quit_dirty_without_save_discards() {
    // Add a control, then quit and confirm "y" — Project on disk should
    // remain empty (no main.yaml controls).
    let fx = CannedEffects::new().with_prompt_answers(vec![
        "add-control".into(),
        "main".into(),
        "CTRL-DIRTY".into(),
        "Title".into(),
        "Description".into(),
        "Remediation".into(),
        "~/.ssh/config".into(),
        "file-exists".into(),
        // quit while dirty
        "quit".into(),
        // confirm discard
        "y".into(),
    ]);
    let os = MockOsEffects::new();
    let dir = Path::new("/proj-dirty");
    seed_empty_project(&fx, dir);

    project_edit(dir, &fx, &os).expect("project_edit must succeed (discard path)");

    // The on-disk project must still have NO main.yaml control written
    // (the in-memory edit was discarded on quit-with-confirmation).
    let reloaded = Project::load_from_dir(dir, &fx).expect("reload");
    assert!(
        reloaded.controls.is_empty(),
        "discarded edit must not persist; got {} controls",
        reloaded.controls.len()
    );
}
