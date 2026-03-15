mod common;
use common::{haiku_judge::HaikuJudge, TestEnv};

// ---------------------------------------------------------------------------
// User management — fully deterministic
// ---------------------------------------------------------------------------

#[test]
fn user_list_empty() {
    let env = TestEnv::new();
    env.run(&["user", "list"])
        .assert_success()
        .assert_stdout_contains("No users configured.");
}

#[test]
fn user_add_and_list() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@test"])
        .assert_success()
        .assert_stdout_contains("Added user: alice@test");

    let r = env.run(&["user", "list"]);
    r.assert_success().assert_stdout_contains("alice@test");
}

#[test]
fn user_add_duplicate_is_error() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@test"]).assert_success();
    env.run(&["user", "add", "alice@test"]).assert_failure();
}

#[test]
fn user_delete_with_retype() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@test"]).assert_success();

    // Pipe the retype confirmation via stdin
    env.run_with_stdin(&["user", "delete", "alice@test"], b"alice@test\n")
        .assert_success()
        .assert_stdout_contains("Deleted user: alice@test");

    // Confirm gone
    env.run(&["user", "list"])
        .assert_success()
        .assert_stdout_contains("No users configured.");
}

#[test]
fn user_delete_wrong_retype_fails() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@test"]).assert_success();

    // Provide wrong name then close stdin — the loop exits with EOF error
    let r = env.run_with_stdin(&["user", "delete", "alice@test"], b"wrong\n");
    // Should not succeed (loop never breaks, EOF causes an error)
    assert!(
        !r.success() || !r.stdout().contains("Deleted"),
        "Should not have deleted with wrong retype"
    );
}

// ---------------------------------------------------------------------------
// Read-only flag
// ---------------------------------------------------------------------------

#[test]
fn read_only_blocks_user_add() {
    let env = TestEnv::new();
    env.run(&["--read-only", "user", "add", "alice@test"])
        .assert_failure()
        .assert_stderr_contains("read-only");
}

#[test]
fn read_only_blocks_key_add() {
    let env = TestEnv::new();
    env.run(&["--read-only", "add", "mykey"])
        .assert_failure()
        .assert_stderr_contains("read-only");
}

#[test]
fn read_only_allows_list() {
    let env = TestEnv::new();
    env.run(&["--read-only", "list"]).assert_success();
}

#[test]
fn read_only_allows_status() {
    let env = TestEnv::new();
    env.run(&["--read-only", "status"]).assert_success();
}

// ---------------------------------------------------------------------------
// Key list — deterministic
// ---------------------------------------------------------------------------

#[test]
fn key_list_empty() {
    let env = TestEnv::new();
    env.run(&["list"])
        .assert_success()
        .assert_stdout_contains("No keys found.");
}

// ---------------------------------------------------------------------------
// Key add + list + delete — deterministic (uses canned keys)
// ---------------------------------------------------------------------------

#[test]
fn key_add_creates_dir_and_info() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@test"]).assert_success();

    let r = env.run(&[
        "add",
        "mykey",
        "--test-only-user",
        "alice@test",
        "--test-only-password-storage",
        "1Password vault",
        "--test-only-comment",
        "",
    ]);
    r.assert_success()
        .assert_stdout_contains("Key created: mykey_");

    // key.pub and info.json must exist somewhere under the key dir
    let keys_dir = env.key_dir().join("keys");
    let subdirs: Vec<_> = std::fs::read_dir(&keys_dir)
        .expect("read keys dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(subdirs.len(), 1, "exactly one key subdir");

    let subdir = subdirs[0].path();
    assert!(subdir.join("key.pub").exists(), "key.pub exists");
    assert!(subdir.join("info.json").exists(), "info.json exists");

    let info_raw = std::fs::read_to_string(subdir.join("info.json")).unwrap();
    let info: serde_json::Value = serde_json::from_str(&info_raw).unwrap();
    assert_eq!(info["password_storage"], "1Password vault");
    assert!(info["creation_date"].as_str().unwrap().starts_with("20"));
}

#[test]
fn key_list_shows_added_key() {
    let env = TestEnv::new();
    env.run(&[
        "add",
        "github-work",
        "--test-only-user",
        "alice@test",
        "--test-only-password-storage",
        "bitwarden",
        "--test-only-comment",
        "",
    ])
    .assert_success();

    env.run(&["list"])
        .assert_success()
        .assert_stdout_contains("github-work");
}

#[test]
fn key_list_verbose_shows_metadata() {
    let env = TestEnv::new();
    env.run(&[
        "add",
        "verbose-key",
        "--test-only-user",
        "alice@test",
        "--test-only-password-storage",
        "1pass-vault",
        "--test-only-comment",
        "my comment",
    ])
    .assert_success();

    let r = env.run(&["list", "-v"]);
    r.assert_success()
        .assert_stdout_contains("1pass-vault")
        .assert_stdout_contains("my comment");
}

#[test]
fn key_delete_removes_dir() {
    let env = TestEnv::new();
    env.run(&[
        "add",
        "to-delete",
        "--test-only-user",
        "alice@test",
        "--test-only-password-storage",
        "nowhere",
        "--test-only-comment",
        "",
    ])
    .assert_success();

    // Find the full dir name so we can pipe the retype
    let keys_dir = env.key_dir().join("keys");
    let dir_name = std::fs::read_dir(&keys_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .expect("key dir exists");

    let stdin = format!("{}\n", dir_name);
    env.run_with_stdin(&["delete", &dir_name], stdin.as_bytes())
        .assert_success()
        .assert_stdout_contains("Deleted key:");

    // Dir must be gone
    assert!(!keys_dir.join(&dir_name).exists(), "key dir was removed");
}

#[test]
fn key_add_duplicate_id_is_error() {
    let env = TestEnv::new();
    let add_args = &[
        "add",
        "same-id",
        "--test-only-user",
        "alice@test",
        "--test-only-password-storage",
        "vault",
        "--test-only-comment",
        "",
    ];
    env.run(add_args).assert_success();
    env.run(add_args).assert_failure();
}

// ---------------------------------------------------------------------------
// Status — deterministic structure checks
// ---------------------------------------------------------------------------

#[test]
fn status_empty_state() {
    let env = TestEnv::new();
    let r = env.run(&["status"]);
    r.assert_success()
        .assert_stdout_contains("State hash:")
        .assert_stdout_contains("Users (0):")
        .assert_stdout_contains("Keys (0):");
}

#[test]
fn status_after_user_add() {
    let env = TestEnv::new();
    env.run(&["user", "add", "bob@corp"]).assert_success();

    let r = env.run(&["status"]);
    r.assert_success()
        .assert_stdout_contains("Users (1):")
        .assert_stdout_contains("bob@corp");
}

#[test]
fn status_after_key_add_shows_inactive() {
    let env = TestEnv::new();
    env.run(&[
        "add",
        "mykey",
        "--test-only-user",
        "alice@test",
        "--test-only-password-storage",
        "vault",
        "--test-only-comment",
        "",
    ])
    .assert_success();

    let r = env.run(&["status"]);
    r.assert_success()
        .assert_stdout_contains("Keys (1):")
        .assert_stdout_contains("inactive"); // no ssh-agent in test environment
}

// ---------------------------------------------------------------------------
// Merkle hash changes when state changes
// ---------------------------------------------------------------------------

#[test]
fn merkle_hash_changes_on_mutation() {
    let env = TestEnv::new();

    let hash1 = extract_hash(&env.run(&["status"]).stdout());

    env.run(&["user", "add", "carol@test"]).assert_success();
    let hash2 = extract_hash(&env.run(&["status"]).stdout());

    assert_ne!(hash1, hash2, "hash must change after user add");

    env.run(&[
        "add",
        "carol-key",
        "--test-only-user",
        "carol@test",
        "--test-only-password-storage",
        "vault",
        "--test-only-comment",
        "",
    ])
    .assert_success();
    let hash3 = extract_hash(&env.run(&["status"]).stdout());

    assert_ne!(hash2, hash3, "hash must change after key add");
}

fn extract_hash(status_output: &str) -> String {
    status_output
        .lines()
        .find(|l| l.starts_with("State hash:"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .expect("State hash line not found")
}

// ---------------------------------------------------------------------------
// Setup — RC file modification
// ---------------------------------------------------------------------------

#[test]
fn setup_creates_zshrc_block() {
    let env = TestEnv::new();
    let fake_home = tempfile::tempdir().expect("temp home");

    let args = [
        "--test-only-home",
        fake_home.path().to_str().unwrap(),
        "setup",
    ];
    let r = env.run_with_stdin_and_env(&args, b"", &[("SHELL", "/bin/zsh")]);
    r.assert_success();

    let rc = std::fs::read_to_string(fake_home.path().join(".zshrc")).expect(".zshrc created");
    assert!(rc.contains("# __added_by_key"), "marker present");
    assert!(rc.contains("export PATH="), "PATH export present");
    assert!(rc.contains("# [ADDED BY key] START"), "START marker");
    assert!(rc.contains("# [ADDED by key] END"), "END marker");
}

#[test]
fn setup_creates_bashrc_block() {
    let env = TestEnv::new();
    let fake_home = tempfile::tempdir().expect("temp home");

    let args = [
        "--test-only-home",
        fake_home.path().to_str().unwrap(),
        "setup",
    ];
    let r = env.run_with_stdin_and_env(&args, b"", &[("SHELL", "/bin/bash")]);
    r.assert_success();

    let rc = std::fs::read_to_string(fake_home.path().join(".bashrc")).expect(".bashrc created");
    assert!(rc.contains("# __added_by_key"), "marker present");
    assert!(rc.contains("export PATH="), "PATH export present");
}

#[test]
fn setup_is_idempotent() {
    let env = TestEnv::new();
    let fake_home = tempfile::tempdir().expect("temp home");

    let args = [
        "--test-only-home",
        fake_home.path().to_str().unwrap(),
        "setup",
    ];

    // Run twice
    env.run_with_stdin_and_env(&args, b"", &[("SHELL", "/bin/zsh")])
        .assert_success();
    env.run_with_stdin_and_env(&args, b"", &[("SHELL", "/bin/zsh")])
        .assert_success();

    let rc = std::fs::read_to_string(fake_home.path().join(".zshrc")).expect(".zshrc created");
    let block_count = rc.matches("# [ADDED BY key] START").count();
    assert_eq!(block_count, 1, "exactly one START marker after two runs");
}

#[test]
fn setup_preserves_existing_rc_content() {
    let env = TestEnv::new();
    let fake_home = tempfile::tempdir().expect("temp home");

    // Write pre-existing content
    let rc_path = fake_home.path().join(".zshrc");
    std::fs::write(&rc_path, "# my existing config\nexport FOO=bar\n").unwrap();

    let args = [
        "--test-only-home",
        fake_home.path().to_str().unwrap(),
        "setup",
    ];
    env.run_with_stdin_and_env(&args, b"", &[("SHELL", "/bin/zsh")])
        .assert_success();

    let rc = std::fs::read_to_string(&rc_path).unwrap();
    assert!(
        rc.contains("# my existing config"),
        "existing content preserved"
    );
    assert!(rc.contains("export FOO=bar"), "existing export preserved");
    assert!(rc.contains("# __added_by_key"), "key block added");
}

// ---------------------------------------------------------------------------
// Haiku-judged semantic tests
// ---------------------------------------------------------------------------

#[test]
fn haiku_user_list_looks_reasonable() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@github"]).assert_success();
    env.run(&["user", "add", "bob@work"]).assert_success();

    let output = env.run(&["user", "list"]).stdout();
    let mut judge = HaikuJudge::load();
    assert!(
        judge.judge(
            "Does this output show a numbered list of users that includes alice@github and bob@work?",
            &output
        ),
        "Haiku judged user list output as FAIL:\n{}",
        output
    );
}

#[test]
fn haiku_status_looks_reasonable() {
    let env = TestEnv::new();
    env.run(&["user", "add", "alice@github"]).assert_success();
    env.run(&[
        "add",
        "mykey",
        "--test-only-user",
        "alice@github",
        "--test-only-password-storage",
        "1Password",
        "--test-only-comment",
        "main key",
    ])
    .assert_success();

    let output = env.run(&["status"]).stdout();
    let mut judge = HaikuJudge::load();
    assert!(
        judge.judge(
            "Does this output look like a valid SSH key manager status report? \
             It should have a merkle hash line, a users section, and a keys section.",
            &output
        ),
        "Haiku judged status output as FAIL:\n{}",
        output
    );
}

#[test]
fn haiku_setup_output_looks_reasonable() {
    let env = TestEnv::new();
    let fake_home = tempfile::tempdir().expect("temp home");

    let args = [
        "--test-only-home",
        fake_home.path().to_str().unwrap(),
        "setup",
    ];
    let output = env
        .run_with_stdin_and_env(&args, b"", &[("SHELL", "/bin/zsh")])
        .stdout();

    let mut judge = HaikuJudge::load();
    assert!(
        judge.judge(
            "Does this output confirm that a shell config file was updated to add \
             a directory to PATH, and tell the user to restart their shell or source the file?",
            &output
        ),
        "Haiku judged setup output as FAIL:\n{}",
        output
    );
}

#[test]
fn haiku_key_list_verbose_looks_reasonable() {
    let env = TestEnv::new();
    env.run(&[
        "add",
        "work-key",
        "--test-only-user",
        "alice@github",
        "--test-only-password-storage",
        "Bitwarden > SSH section",
        "--test-only-comment",
        "primary work key",
    ])
    .assert_success();

    let output = env.run(&["list", "-v"]).stdout();
    let mut judge = HaikuJudge::load();
    assert!(
        judge.judge(
            "Does this output show a verbose listing of SSH keys with metadata \
             including a creation date, password storage hint, and comment?",
            &output
        ),
        "Haiku judged verbose list output as FAIL:\n{}",
        output
    );
}
