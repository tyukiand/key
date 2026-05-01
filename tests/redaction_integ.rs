//! Spec/0017 §D — integration tests for the OsEffects redaction kernel.
//!
//! Covers:
//!   D.2 — OsEffects integration via MockOsEffects (token / path / normal vars
//!         + content lines; assert redacted shapes; allowlist suppression).
//!   D.4 — `looks-like-password` predicate across env + file content.
//!   D.5 — round-trip on Projects with AddUnredactedMatcher /
//!         DeleteUnredactedMatcher.

#![cfg(feature = "testing")]

use std::path::Path;

use key::effects::OsEffectsRo;
use key::project::{compile_project, Project, ProjectMutation};
use key::rules::ast::{
    Control, ControlFile, FilePredicateAst, Proposition, PseudoFileFixture, SimplePath,
};
use key::rules::evaluate::evaluate_with_ctx;
use key::rules::pseudo::EvalContext;
use key::security::unredacted::UnredactedMatcher;
use key::test_support::mock_os_effects::MockOsEffects;

const GHP: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB";

// ---------------------------------------------------------------------------
// D.2 — OsEffects integration
// ---------------------------------------------------------------------------

#[test]
fn d2_env_vars_redacts_token_keeps_path_and_normal_verbatim() {
    let mock = MockOsEffects::new();
    mock.set_env("GITHUB_TOKEN", GHP);
    mock.set_env("PATH", "/usr/bin");
    mock.set_env("NORMAL_VAR", "hello");

    let pairs = mock.env_vars();
    let map: std::collections::BTreeMap<String, String> = pairs.into_iter().collect();
    assert!(
        map["GITHUB_TOKEN"].starts_with("REDACTED42"),
        "GITHUB_TOKEN should be redacted: {:?}",
        map["GITHUB_TOKEN"]
    );
    assert_eq!(map["GITHUB_TOKEN"].len(), GHP.len());
    assert_eq!(map["PATH"], "/usr/bin");
    assert_eq!(map["NORMAL_VAR"], "hello");
}

#[test]
fn d2_allowlist_suppresses_redaction() {
    let mock = MockOsEffects::with_unredacted(vec![UnredactedMatcher::value(GHP).unwrap()]);
    mock.set_env("GITHUB_TOKEN", GHP);
    let pairs = mock.env_vars();
    let map: std::collections::BTreeMap<String, String> = pairs.into_iter().collect();
    assert_eq!(
        map["GITHUB_TOKEN"], GHP,
        "allowlist must suppress redaction"
    );
}

#[test]
fn d2_read_to_string_redacts_inline_token() {
    let mock = MockOsEffects::new();
    let content = format!("password = {}\n", GHP);
    mock.seed_file("/etc/cfg.toml", content.as_bytes());
    let body = mock.read_to_string(Path::new("/etc/cfg.toml")).unwrap();
    assert!(body.starts_with("password = "));
    assert!(body.contains("REDACTED42"), "got body: {:?}", body);
    assert_eq!(body.len(), content.len(), "length must be preserved");
}

#[test]
fn d2_read_file_redacts_inline_token() {
    let mock = MockOsEffects::new();
    let content = format!("api_key = {}\n", GHP);
    mock.seed_file("/etc/cfg.toml", content.as_bytes());
    let bytes = mock.read_file(Path::new("/etc/cfg.toml")).unwrap();
    let body = String::from_utf8(bytes).unwrap();
    assert!(body.contains("REDACTED42"));
}

// ---------------------------------------------------------------------------
// D.4 — looks-like-password predicate
// ---------------------------------------------------------------------------

fn looks_like_password_proposition(path: &str) -> Proposition {
    Proposition::FileSatisfies {
        path: SimplePath::new(path).unwrap(),
        check: FilePredicateAst::LooksLikePassword,
    }
}

#[test]
fn d4_looks_like_password_pass_on_env_with_token() {
    let mock = MockOsEffects::new();
    mock.set_env("GITHUB_TOKEN", GHP);
    mock.set_env("PATH", "/usr/bin");

    let ctx = EvalContext::with_fixture_and_os(
        std::env::temp_dir(),
        PseudoFileFixture::default(),
        Box::new(mock),
    );
    let prop = looks_like_password_proposition("<env>");
    let res = evaluate_with_ctx(&prop, &ctx);
    assert!(
        res.is_ok(),
        "expected PASS on env containing a token; got {:?}",
        res
    );
}

#[test]
fn d4_looks_like_password_fail_on_env_with_no_secrets() {
    let mock = MockOsEffects::new();
    mock.set_env("PATH", "/usr/bin");
    mock.set_env("HOME", "/home/u");
    mock.set_env("LANG", "en_US.UTF-8");

    let ctx = EvalContext::with_fixture_and_os(
        std::env::temp_dir(),
        PseudoFileFixture::default(),
        Box::new(mock),
    );
    let prop = looks_like_password_proposition("<env>");
    let res = evaluate_with_ctx(&prop, &ctx);
    assert!(res.is_err(), "expected FAIL on env with no secrets");
}

#[test]
fn d4_looks_like_password_pass_on_file_content_with_token() {
    let tmp = tempfile::tempdir().unwrap();
    let content = format!("api_key = {}\n", GHP);
    std::fs::write(tmp.path().join("secrets.txt"), content).unwrap();

    let prop = looks_like_password_proposition("~/secrets.txt");
    let res = key::rules::evaluate::evaluate(&prop, tmp.path());
    assert!(
        res.is_ok(),
        "expected PASS on file containing a token; got {:?}",
        res
    );
}

#[test]
fn d4_looks_like_password_fail_on_file_content_no_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("normal.txt"), "hello\nworld\n").unwrap();

    let prop = looks_like_password_proposition("~/normal.txt");
    let res = key::rules::evaluate::evaluate(&prop, tmp.path());
    assert!(res.is_err(), "expected FAIL on file with no secrets");
}

// ---------------------------------------------------------------------------
// D.5 — round-trip on Projects with AddUnredactedMatcher / DeleteUnredactedMatcher
// ---------------------------------------------------------------------------

fn sample_control(id: &str) -> Control {
    Control {
        id: id.to_string(),
        title: format!("title-{}", id),
        description: format!("desc-{}", id),
        remediation: format!("rem-{}", id),
        check: Proposition::FileSatisfies {
            path: SimplePath::new("~/x").unwrap(),
            check: FilePredicateAst::FileExists,
        },
    }
}

#[test]
fn d5_round_trip_project_with_unredacted_matchers() {
    // Build a project with a control + a few unredacted matchers, then
    // round-trip through compile_project / apply_mutations.
    let mut p = Project::empty()
        .with_control_added(
            "alpha".try_into().unwrap(),
            ControlFile {
                controls: vec![sample_control("X")],
            },
        )
        .unwrap()
        .with_unredacted_matcher_added(UnredactedMatcher::value("ghp_xxx").unwrap())
        .unwrap()
        .with_unredacted_matcher_added(UnredactedMatcher::prefix("sha256:").unwrap())
        .unwrap();
    p = p
        .with_unredacted_matcher_added(UnredactedMatcher::prefix("img_id_").unwrap())
        .unwrap();

    let ops = compile_project(&p);
    let rebuilt = Project::apply_mutations(Project::empty(), ops).unwrap();
    assert_eq!(rebuilt, p, "round-trip diff for project with unredacted");
}

#[test]
fn d5_torture_add_then_delete_same_matcher() {
    // Hand-curated torture case (per spec/0017 §C.3): add then delete the
    // same matcher; final state must equal the empty allowlist.
    let m = UnredactedMatcher::value("ghp_xxx").unwrap();
    let ops = vec![
        ProjectMutation::AddUnredactedMatcher { matcher: m.clone() },
        ProjectMutation::DeleteUnredactedMatcher { matcher: m },
        ProjectMutation::Done,
    ];
    let p = Project::apply_mutations(Project::empty(), ops).unwrap();
    assert!(
        p.unredacted.is_empty(),
        "add-then-delete should leave the allowlist empty"
    );
}

#[test]
fn d5_duplicate_unredacted_matcher_is_an_error() {
    let m = UnredactedMatcher::value("ghp_xxx").unwrap();
    let ops = vec![
        ProjectMutation::AddUnredactedMatcher { matcher: m.clone() },
        ProjectMutation::AddUnredactedMatcher { matcher: m },
        ProjectMutation::Done,
    ];
    let err = Project::apply_mutations(Project::empty(), ops).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("ghp_xxx"),
        "error must name the duplicate literal: {}",
        msg
    );
}

#[test]
fn d5_delete_missing_unredacted_matcher_is_an_error() {
    let m = UnredactedMatcher::prefix("nope_").unwrap();
    let ops = vec![
        ProjectMutation::DeleteUnredactedMatcher { matcher: m },
        ProjectMutation::Done,
    ];
    let err = Project::apply_mutations(Project::empty(), ops).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("nope_"),
        "error must name the missing matcher: {}",
        msg
    );
}
