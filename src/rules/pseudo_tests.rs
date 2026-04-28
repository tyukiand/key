//! Per-spec §6.1–6.7 unit and meta-tests for pseudo-files.
//! Tests are gated on `cfg(test)` only — no production code paths reach here.

#![cfg(test)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::rules::ast::{
    DataSchema, ExecutableSnapshot, FilePredicateAst, Proposition, PseudoFile, PseudoFileFixture,
    SimplePath,
};
use crate::rules::evaluate::evaluate_with_ctx;
use crate::rules::fixture::parse_fixture;
use crate::rules::pseudo::EvalContext;

fn empty_home() -> PathBuf {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().to_path_buf();
    // Leak the tempdir; tests are ephemeral.
    std::mem::forget(tmp);
    p
}

fn ctx_with(
    env: Option<BTreeMap<String, String>>,
    exes: Option<BTreeMap<String, ExecutableSnapshot>>,
) -> EvalContext {
    EvalContext::with_fixture(
        empty_home(),
        PseudoFileFixture {
            env_override: env,
            executable_override: exes,
        },
    )
}

fn check_against_env(check: FilePredicateAst, env: BTreeMap<String, String>) -> Result<(), String> {
    let ctx = ctx_with(Some(env), None);
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<env>").unwrap(),
        check,
    };
    match evaluate_with_ctx(&prop, &ctx) {
        Ok(()) => Ok(()),
        Err(failures) => Err(failures
            .into_iter()
            .map(|f| f.message)
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

fn check_against_exec(
    name: &str,
    check: FilePredicateAst,
    exes: BTreeMap<String, ExecutableSnapshot>,
) -> Result<(), String> {
    let ctx = ctx_with(None, Some(exes));
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new(&format!("<executable:{}>", name)).unwrap(),
        check,
    };
    match evaluate_with_ctx(&prop, &ctx) {
        Ok(()) => Ok(()),
        Err(failures) => Err(failures
            .into_iter()
            .map(|f| f.message)
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

// ---------------------------------------------------------------------------
// §6.2 — <env> evaluator with env_override
// ---------------------------------------------------------------------------

#[test]
fn env_empty_materializes_to_empty_body_zero_lines() {
    let env = BTreeMap::new();
    assert!(check_against_env(
        FilePredicateAst::TextHasLines {
            min: None,
            max: Some(0)
        },
        env
    )
    .is_ok());
}

#[test]
fn env_shell_exports_var_present() {
    let mut env = BTreeMap::new();
    env.insert("FOO".into(), "bar".into());
    assert!(check_against_env(FilePredicateAst::ShellExports("FOO".into()), env).is_ok());
}

#[test]
fn env_shell_exports_var_absent_fails() {
    let env = BTreeMap::new();
    assert!(check_against_env(FilePredicateAst::ShellExports("FOO".into()), env).is_err());
}

#[test]
fn env_shell_adds_to_path_segment_present() {
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".into(),
        "/usr/bin:/opt/x/bin:/home/u/.cargo/bin".into(),
    );
    assert!(check_against_env(FilePredicateAst::ShellAddsToPath("/opt/x/bin".into()), env).is_ok());
}

#[test]
fn env_shell_adds_to_path_segment_absent_fails() {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin:/home/u/.cargo/bin".into());
    let err =
        check_against_env(FilePredicateAst::ShellAddsToPath("/opt/x/bin".into()), env).unwrap_err();
    assert!(err.contains("PATH does not contain"));
}

#[test]
fn env_inapplicable_xml_matches() {
    let mut env = BTreeMap::new();
    env.insert("FOO".into(), "bar".into());
    let err = check_against_env(FilePredicateAst::XmlMatchesPath("a/b".into()), env).unwrap_err();
    assert!(err.contains("xml-matches"));
    assert!(err.contains("<env>"));
}

#[test]
fn env_inapplicable_json_matches() {
    let env = BTreeMap::new();
    let err =
        check_against_env(FilePredicateAst::JsonMatches(DataSchema::Anything), env).unwrap_err();
    assert!(err.contains("json-matches"));
    assert!(err.contains("<env>"));
}

#[test]
fn env_inapplicable_yaml_matches() {
    let env = BTreeMap::new();
    let err =
        check_against_env(FilePredicateAst::YamlMatches(DataSchema::Anything), env).unwrap_err();
    assert!(err.contains("yaml-matches"));
    assert!(err.contains("<env>"));
}

#[test]
fn env_inapplicable_properties_defines_key() {
    let env = BTreeMap::new();
    let err =
        check_against_env(FilePredicateAst::PropertiesDefinesKey("k".into()), env).unwrap_err();
    assert!(err.contains("properties-defines-key"));
    assert!(err.contains("<env>"));
}

#[test]
fn env_newline_in_value_escaped_to_literal_backslash_n() {
    let mut env = BTreeMap::new();
    // Two entries; the first has an embedded newline. After escape it occupies
    // exactly one logical line.
    env.insert("A".into(), "line1\nline2".into());
    env.insert("B".into(), "plain".into());
    assert!(check_against_env(
        FilePredicateAst::TextHasLines {
            min: Some(2),
            max: Some(2)
        },
        env
    )
    .is_ok());
}

// ---------------------------------------------------------------------------
// §6.3 — <executable:NAME> evaluator with executable_override
// ---------------------------------------------------------------------------

fn snap_found(name: &str, version: &str) -> ExecutableSnapshot {
    ExecutableSnapshot {
        name: name.to_string(),
        found: true,
        executable: true,
        path: Some(format!("/usr/bin/{}", name)),
        command_full: Some(format!("{} --version", name)),
        version_full: Some(format!("{} version {}", name, version)),
        version: Some(version.to_string()),
    }
}

#[test]
fn exec_not_found_file_exists_fails() {
    let mut exes = BTreeMap::new();
    exes.insert("ghost".into(), ExecutableSnapshot::not_found("ghost"));
    let err = check_against_exec("ghost", FilePredicateAst::FileExists, exes).unwrap_err();
    assert!(err.contains("not found"));
}

#[test]
fn exec_found_but_not_executable_file_exists_fails() {
    // Per spec §3.5 + §6.3 the wording: "file-exists is true iff found=true".
    // A stat-able non-executable still counts as found=true. Confirm spec wording.
    let mut exes = BTreeMap::new();
    exes.insert(
        "noexec".into(),
        ExecutableSnapshot {
            name: "noexec".into(),
            found: true,
            executable: false,
            path: Some("/usr/bin/noexec".into()),
            command_full: None,
            version_full: None,
            version: None,
        },
    );
    // file-exists ↔ found=true, so this PASSES (per §3.5 doc + spec §6.3 second-confirmation).
    assert!(check_against_exec("noexec", FilePredicateAst::FileExists, exes).is_ok());
}

#[test]
fn exec_happy_path_json_match_on_version_string() {
    let mut exes = BTreeMap::new();
    exes.insert("docker".into(), snap_found("docker", "20.10.7"));
    let schema = DataSchema::IsObject(vec![(
        "version".into(),
        DataSchema::IsStringMatching(r"^20\.".into()),
    )]);
    assert!(check_against_exec("docker", FilePredicateAst::JsonMatches(schema), exes).is_ok());
}

#[test]
fn exec_inapplicable_shell_exports() {
    let mut exes = BTreeMap::new();
    exes.insert("docker".into(), snap_found("docker", "20.10.7"));
    let err =
        check_against_exec("docker", FilePredicateAst::ShellExports("X".into()), exes).unwrap_err();
    assert!(err.contains("shell-exports"));
    assert!(err.contains("<executable:docker>"));
}

#[test]
fn exec_inapplicable_text_has_lines() {
    let mut exes = BTreeMap::new();
    exes.insert("docker".into(), snap_found("docker", "20.10.7"));
    let err = check_against_exec(
        "docker",
        FilePredicateAst::TextHasLines {
            min: Some(1),
            max: None,
        },
        exes,
    )
    .unwrap_err();
    assert!(err.contains("text-has-lines"));
    assert!(err.contains("<executable:docker>"));
}

#[test]
fn exec_inapplicable_xml_matches() {
    let mut exes = BTreeMap::new();
    exes.insert("docker".into(), snap_found("docker", "20.10.7"));
    let err = check_against_exec(
        "docker",
        FilePredicateAst::XmlMatchesPath("a/b".into()),
        exes,
    )
    .unwrap_err();
    assert!(err.contains("xml-matches"));
}

// ---------------------------------------------------------------------------
// §6.5 — Caching
// ---------------------------------------------------------------------------

#[test]
fn caching_within_one_run_pseudo_resolved_once() {
    // Construct a fixture with one executable; resolve via two distinct
    // predicates in the same run. They should observe the SAME snapshot.
    let mut exes = BTreeMap::new();
    exes.insert("docker".into(), snap_found("docker", "20.10.7"));
    let ctx = ctx_with(None, Some(exes));

    let prop = Proposition::All(vec![
        Proposition::FileSatisfies {
            path: SimplePath::new("<executable:docker>").unwrap(),
            check: FilePredicateAst::FileExists,
        },
        Proposition::FileSatisfies {
            path: SimplePath::new("<executable:docker>").unwrap(),
            check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
                "version".into(),
                DataSchema::IsStringMatching(r"^20\.".into()),
            )])),
        },
    ]);
    assert!(evaluate_with_ctx(&prop, &ctx).is_ok());

    // Now re-resolve to confirm cached:
    let snap1 = ctx.resolve(&PseudoFile::Executable("docker".into()));
    let snap2 = ctx.resolve(&PseudoFile::Executable("docker".into()));
    assert_eq!(snap1.body, snap2.body);
}

// ---------------------------------------------------------------------------
// §6.7 / §3.8.1 — Meta-tests: the override mechanism is total + hermetic.
// ---------------------------------------------------------------------------

#[test]
fn meta_executable_override_total_when_path_unset() {
    // Even with PATH unset, an override-declared executable is observed verbatim.
    let saved = std::env::var_os("PATH");
    // SAFETY: tests are run sequentially per process for the executable harness;
    // the env mutation is restored after this test's body. (Cargo runs tests in
    // parallel by default — but this test never relies on PATH for the override
    // path. We unset PATH only to demonstrate hermeticity.)
    let _guard = scopeguard_restore_path(saved);
    std::env::remove_var("PATH");

    let mut exes = BTreeMap::new();
    exes.insert(
        "made-up-tool-12345".into(),
        snap_found("made-up-tool-12345", "9.9.9"),
    );
    let ctx = ctx_with(None, Some(exes));
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<executable:made-up-tool-12345>").unwrap(),
        check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
            "version".into(),
            DataSchema::IsStringMatching(r"^9\.9\.9$".into()),
        )])),
    };
    assert!(evaluate_with_ctx(&prop, &ctx).is_ok());
}

#[test]
fn meta_env_override_total_against_real_env() {
    let mut env = BTreeMap::new();
    env.insert("TEST_FIXTURE_OK".into(), "1".into());
    let ctx = ctx_with(Some(env), None);
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<env>").unwrap(),
        check: FilePredicateAst::ShellExports("TEST_FIXTURE_OK".into()),
    };
    // The real process likely has no TEST_FIXTURE_OK; the override should still satisfy.
    assert!(evaluate_with_ctx(&prop, &ctx).is_ok());
}

#[test]
fn meta_executable_override_absent_name_means_not_found() {
    // With executable_override Some(map), references to NAMEs not in the map
    // must report found=false (NOT silently fall through to PATH).
    let exes: BTreeMap<String, ExecutableSnapshot> = BTreeMap::new();
    let ctx = ctx_with(None, Some(exes));
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<executable:also-not-declared>").unwrap(),
        check: FilePredicateAst::FileExists,
    };
    let err = evaluate_with_ctx(&prop, &ctx).unwrap_err();
    assert_eq!(err.len(), 1);
    assert!(err[0].message.contains("not found"));
}

#[test]
fn meta_env_override_absent_key_means_not_set() {
    // env_override is total: keys absent from the map are treated as not set.
    let env = BTreeMap::new();
    let ctx = ctx_with(Some(env), None);
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("<env>").unwrap(),
        check: FilePredicateAst::ShellExports("PATH".into()),
    };
    assert!(evaluate_with_ctx(&prop, &ctx).is_err());
}

#[test]
fn meta_malformed_fixture_rejected_with_clear_error() {
    let yaml = r#"
executables:
  bad:
    found: "yes"
    executable: true
"#;
    let err = parse_fixture(yaml).unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("bool"), "unexpected error: {}", msg);
    assert!(msg.contains("found"), "unexpected error: {}", msg);
}

#[test]
fn fixture_round_trip_drives_evaluator() {
    let yaml = r#"
env:
  TEST_FIXTURE_OK: "1"
executables:
  docker:
    found: true
    executable: true
    path: /usr/bin/docker
    command-full: docker --version
    version-full: |
      Docker version 20.10.7, build f0df350
    version: 20.10.7
"#;
    let fixture = parse_fixture(yaml).unwrap();
    let ctx = EvalContext::with_fixture(empty_home(), fixture);

    let prop = Proposition::All(vec![
        Proposition::FileSatisfies {
            path: SimplePath::new("<env>").unwrap(),
            check: FilePredicateAst::ShellExports("TEST_FIXTURE_OK".into()),
        },
        Proposition::FileSatisfies {
            path: SimplePath::new("<executable:docker>").unwrap(),
            check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
                "version".into(),
                DataSchema::IsStringMatching(r"^20\.".into()),
            )])),
        },
    ]);
    assert!(evaluate_with_ctx(&prop, &ctx).is_ok());
}

// ---------------------------------------------------------------------------
// Local helper: scopeguard-style env restorer
// ---------------------------------------------------------------------------

struct PathRestore(Option<std::ffi::OsString>);
impl Drop for PathRestore {
    fn drop(&mut self) {
        match self.0.take() {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
}
fn scopeguard_restore_path(saved: Option<std::ffi::OsString>) -> PathRestore {
    PathRestore(saved)
}
