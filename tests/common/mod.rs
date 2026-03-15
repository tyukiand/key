pub mod haiku_judge;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

static CANNED_KEYS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Isolated test environment: temp key dir + path to canned keys fixture.
pub struct TestEnv {
    pub key_dir: tempfile::TempDir,
    pub canned_keys_dir: PathBuf,
}

impl TestEnv {
    /// Create a new test environment.
    /// Ensures canned keys exist (generated exactly once across all parallel tests).
    pub fn new() -> Self {
        let canned_keys_dir = CANNED_KEYS_DIR.get_or_init(generate_canned_keys).clone();
        TestEnv {
            key_dir: tempfile::tempdir().expect("create temp dir"),
            canned_keys_dir,
        }
    }

    pub fn key_dir(&self) -> &Path {
        self.key_dir.path()
    }

    /// Run the `key` binary with the given args.
    /// Automatically injects --test-only-key-dir and --test-only-canned-keys.
    pub fn run(&self, args: &[&str]) -> CmdResult {
        self.run_with_stdin(args, b"")
    }

    /// Run with piped stdin bytes.
    pub fn run_with_stdin(&self, args: &[&str], stdin: &[u8]) -> CmdResult {
        self.run_with_stdin_and_env(args, stdin, &[])
    }

    /// Run with piped stdin bytes and extra environment variables.
    pub fn run_with_stdin_and_env(
        &self,
        args: &[&str],
        stdin: &[u8],
        env: &[(&str, &str)],
    ) -> CmdResult {
        let bin = bin_path();
        let mut cmd = Command::new(&bin);

        // Inject test-only flags before any subcommand args
        cmd.arg("--test-only-key-dir")
            .arg(self.key_dir())
            .arg("--test-only-canned-keys")
            .arg(&self.canned_keys_dir);

        for a in args {
            cmd.arg(a);
        }

        for (k, v) in env {
            cmd.env(k, v);
        }

        use std::io::Write;
        use std::process::Stdio;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().expect("spawn key binary");
        if !stdin.is_empty() {
            child.stdin.as_mut().unwrap().write_all(stdin).ok();
        }
        drop(child.stdin.take()); // close stdin

        let output = child.wait_with_output().expect("wait for key binary");
        CmdResult { output }
    }
}

pub struct CmdResult {
    pub output: Output,
}

impl CmdResult {
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    pub fn success(&self) -> bool {
        self.output.status.success()
    }

    pub fn exit_code(&self) -> i32 {
        self.output.status.code().unwrap_or(-1)
    }

    /// Assert command succeeded; print stdout/stderr on failure.
    #[track_caller]
    pub fn assert_success(&self) -> &Self {
        if !self.success() {
            panic!(
                "Command failed (exit {})\nstdout: {}\nstderr: {}",
                self.exit_code(),
                self.stdout(),
                self.stderr()
            );
        }
        self
    }

    /// Assert command failed.
    #[track_caller]
    pub fn assert_failure(&self) -> &Self {
        if self.success() {
            panic!(
                "Expected failure but command succeeded\nstdout: {}",
                self.stdout()
            );
        }
        self
    }

    /// Assert stdout contains the given substring.
    #[track_caller]
    pub fn assert_stdout_contains(&self, needle: &str) -> &Self {
        let out = self.stdout();
        if !out.contains(needle) {
            panic!(
                "Expected stdout to contain {:?}\nGot: {}",
                needle, out
            );
        }
        self
    }

    /// Assert stderr contains the given substring.
    #[track_caller]
    pub fn assert_stderr_contains(&self, needle: &str) -> &Self {
        let err = self.stderr();
        if !err.contains(needle) {
            panic!(
                "Expected stderr to contain {:?}\nGot: {}",
                needle, err
            );
        }
        self
    }
}

fn bin_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is the package root; binary is in target/debug/
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("target/debug/key")
}

/// Generate 5 canned ed25519 key pairs into `target/test-fixtures/canned_keys/<n>/`.
/// Called at most once via OnceLock; safe under parallel test execution.
fn generate_canned_keys() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let canned_dir = PathBuf::from(&manifest).join("target/test-fixtures/canned_keys");

    std::fs::create_dir_all(&canned_dir).expect("create canned_keys dir");

    for i in 0..5 {
        let dir = canned_dir.join(format!("{}", i));
        std::fs::create_dir_all(&dir).expect("create canned key subdir");
        let key_path = dir.join("key");
        if key_path.exists() && dir.join("key.pub").exists() {
            continue;
        }
        // Remove any partial key from a previous failed run
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(dir.join("key.pub"));

        // Generate with empty passphrase (-N "") — fully non-interactive
        let status = Command::new("ssh-keygen")
            .args([
                "-t", "ed25519",
                "-f", key_path.to_str().unwrap(),
                "-N", "",
                "-C", &format!("test-key-{}", i),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run ssh-keygen for canned keys");
        assert!(status.success(), "ssh-keygen failed for canned key {}", i);
    }

    canned_dir
}
