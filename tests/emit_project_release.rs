//! Spec/0013 §B.6 — release-binary integration test.
//!
//! Subprocess-invokes `target/release/key audit guide --emit-project <tempdir>`,
//! then `target/release/key audit project test` against `<tempdir>`. Both
//! exit 0; the second's stdout contains the canonical pass marker.
//!
//! This is the only end-to-end test that runs the released binary against
//! EDSL output. If any later increment breaks the round-trip, this test
//! catches it before release.

use std::process::Command;

fn release_binary() -> std::path::PathBuf {
    // Anchor on the crate's CARGO_MANIFEST_DIR to be robust to test cwd.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let p = std::path::Path::new(manifest_dir).join("target/release/key");
    if !p.is_file() {
        panic!(
            "release binary not found at {} — run `cargo build --release` first",
            p.display(),
        );
    }
    p
}

fn run(
    bin: &std::path::Path,
    args: &[&str],
    cwd: Option<&std::path::Path>,
) -> (i32, String, String) {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = cmd.output().expect("subprocess");
    let code = out.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn emit_project_then_project_test_round_trip_release_binary() {
    let bin = release_binary();
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("emitted");

    // 1. Materialize the project.
    let (code, stdout, stderr) = run(
        &bin,
        &["audit", "guide", "--emit-project", target.to_str().unwrap()],
        None,
    );
    assert_eq!(
        code, 0,
        "`key audit guide --emit-project` exit non-zero (stdout: {}; stderr: {})",
        stdout, stderr,
    );
    assert!(
        stdout.contains("Emitted audit project"),
        "expected emit summary in stdout; got {:?}",
        stdout,
    );

    // 2. Run `key audit project test` from inside the emitted project.
    let (code, stdout, stderr) = run(&bin, &["audit", "project", "test"], Some(&target));
    assert_eq!(
        code, 0,
        "`key audit project test` failed on emitted project \
         (stdout: {}; stderr: {})",
        stdout, stderr,
    );
    // Canonical pass marker — the per-control PASS line emitted by project_test.
    assert!(
        stdout.contains("[PASS]"),
        "expected `[PASS]` marker in stdout; got {:?}",
        stdout,
    );
    assert!(
        stdout.contains("0 failed"),
        "expected `0 failed` in summary line; got {:?}",
        stdout,
    );
}

#[test]
fn emit_project_verbose_then_project_test_round_trip_release_binary() {
    // Spec/0013 §B.2 — `-v` variant must round-trip independently.
    let bin = release_binary();
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("emitted-v");

    let (code, _, stderr) = run(
        &bin,
        &[
            "audit",
            "guide",
            "-v",
            "--emit-project",
            target.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "verbose emit failed: {}", stderr);

    let (code, stdout, stderr) = run(&bin, &["audit", "project", "test"], Some(&target));
    assert_eq!(
        code, 0,
        "verbose emitted project test failed: stdout={}; stderr={}",
        stdout, stderr,
    );
    assert!(stdout.contains("0 failed"));
}

#[test]
fn emit_project_refuses_non_empty_dir_release_binary() {
    let bin = release_binary();
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("preexisting.txt"), "hi").unwrap();
    let (code, _stdout, stderr) = run(
        &bin,
        &[
            "audit",
            "guide",
            "--emit-project",
            tmp.path().to_str().unwrap(),
        ],
        None,
    );
    assert_ne!(code, 0, "expected non-zero exit on non-empty target");
    assert!(
        stderr.contains("non-empty") || stderr.contains("clobber"),
        "expected error mentioning non-empty / clobber; got {:?}",
        stderr,
    );
}
