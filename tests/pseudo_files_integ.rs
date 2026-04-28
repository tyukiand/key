//! Integration tests for `<executable:NAME>` pseudo-files (spec/0009 §6.4).
//!
//! Run real subprocesses against vendored shell-script fixtures under
//! `tests/fixtures/bin/`. PATH is sandboxed to the fixture directory so the
//! host environment doesn't pollute the test.
//!
//! Capped at five cases per spec.

#![cfg(feature = "testing")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use key::rules::ast::{
    DataSchema, FilePredicateAst, Proposition, PseudoFile, PseudoFileFixture, SimplePath,
};
use key::rules::evaluate::evaluate_with_ctx;
use key::rules::pseudo::EvalContext;

/// Path to vendored shell-script fixtures (`tests/fixtures/bin/`).
fn fixtures_bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("bin")
}

/// Tests in this file mutate $PATH; serialize them to avoid races.
static PATH_LOCK: Mutex<()> = Mutex::new(());

struct PathRestore(Option<std::ffi::OsString>);
impl Drop for PathRestore {
    fn drop(&mut self) {
        match self.0.take() {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn with_sandboxed_path() -> PathRestore {
    let saved = std::env::var_os("PATH");
    std::env::set_var("PATH", fixtures_bin_dir());
    PathRestore(saved)
}

fn empty_home() -> PathBuf {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().to_path_buf();
    std::mem::forget(tmp);
    p
}

fn fresh_ctx() -> EvalContext {
    EvalContext::with_fixture(empty_home(), PseudoFileFixture::default())
}

#[test]
fn integ_generic_fallback_extracts_dummy_tool_version() {
    let _g = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = with_sandboxed_path();

    let ctx = fresh_ctx();
    let snap = ctx.resolve(&PseudoFile::Executable("dummy-tool".into()));
    assert!(
        snap.body.contains(r#""version": "1.2.3""#),
        "snapshot:\n{}",
        snap.body
    );
    assert!(
        snap.body
            .contains(r#""command-full": "dummy-tool --version""#),
        "snapshot:\n{}",
        snap.body
    );
}

#[test]
fn integ_custom_extractor_runs_against_shadowed_git() {
    let _g = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = with_sandboxed_path();

    // Our shadow git prints `git version 9.8.7`; the custom extractor's regex
    // `git version (\d+\.\d+\.\d+)` should capture 9.8.7.
    let ctx = fresh_ctx();
    let snap = ctx.resolve(&PseudoFile::Executable("git".into()));
    assert!(
        snap.body.contains(r#""version": "9.8.7""#),
        "snapshot:\n{}",
        snap.body
    );
}

#[test]
fn integ_fallback_loop_reaches_help_flag() {
    let _g = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = with_sandboxed_path();

    // help-only-tool only responds to --help. The fallback loop should walk
    // --version, -version, -V, --help in order until `--help` succeeds.
    let ctx = fresh_ctx();
    let snap = ctx.resolve(&PseudoFile::Executable("help-only-tool".into()));
    assert!(
        snap.body.contains(r#""version": "9.9.9""#),
        "expected to extract 9.9.9 from --help output:\n{}",
        snap.body
    );
    assert!(
        snap.body
            .contains(r#""command-full": "help-only-tool --help""#),
        "expected --help to be the winning flag:\n{}",
        snap.body
    );
}

#[test]
fn integ_predicate_evaluation_against_real_executable() {
    let _g = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = with_sandboxed_path();

    let ctx = fresh_ctx();
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<executable:dummy-tool>").unwrap(),
        check: FilePredicateAst::All(vec![
            FilePredicateAst::FileExists,
            FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
                "version".into(),
                DataSchema::IsStringMatching(r"^1\.2\.3$".into()),
            )])),
        ]),
    };
    assert!(evaluate_with_ctx(&prop, &ctx).is_ok());
}

#[test]
fn integ_not_found_when_path_does_not_contain_name() {
    let _g = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = with_sandboxed_path();

    // Sandbox PATH to fixtures/bin; this directory doesn't contain
    // `definitely-no-such-tool-12345`.
    let ctx = fresh_ctx();
    let snap = ctx.resolve(&PseudoFile::Executable(
        "definitely-no-such-tool-12345".into(),
    ));
    assert!(
        snap.body.contains(r#""found": false"#),
        "snapshot should report found=false:\n{}",
        snap.body
    );
}

// ---------------------------------------------------------------------------
// §3.8.1 / §6.7 — meta-tests that prove the override path is hermetic vs PATH.
// ---------------------------------------------------------------------------

#[test]
fn meta_override_total_when_path_is_empty() {
    let _g = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Drop PATH entirely — non-overridden lookups would not find anything.
    let saved = std::env::var_os("PATH");
    std::env::remove_var("PATH");
    let _restore = PathRestore(saved);

    let mut exes = BTreeMap::new();
    exes.insert(
        "made-up-tool-12345".into(),
        key::rules::ast::ExecutableSnapshot {
            name: "made-up-tool-12345".into(),
            found: true,
            executable: true,
            path: Some("/fake/made-up-tool-12345".into()),
            command_full: Some("made-up-tool-12345 --version".into()),
            version_full: Some("made-up-tool-12345 9.9.9".into()),
            version: Some("9.9.9".into()),
        },
    );

    let ctx = EvalContext::with_fixture(
        empty_home(),
        PseudoFileFixture {
            env_override: None,
            executable_override: Some(exes),
        },
    );
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<executable:made-up-tool-12345>").unwrap(),
        check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
            "version".into(),
            DataSchema::IsStringMatching(r"^9\.9\.9$".into()),
        )])),
    };
    assert!(
        evaluate_with_ctx(&prop, &ctx).is_ok(),
        "override must observe version=9.9.9 even with PATH unset"
    );
}
