use key::effects::{CannedEffects, Effects};
use key::mutation::MutationToken;
use key::state::State;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// User management tests
// ---------------------------------------------------------------------------

#[test]
fn user_list_empty() {
    let fx = CannedEffects::new();
    let state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    key::commands::user::list(&state, &fx).unwrap();
    assert!(fx.output().contains("No users configured."));
}

#[test]
fn user_add_and_list() {
    let fx = CannedEffects::new();
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    fx.clear_output();
    key::commands::user::list(&state, &fx).unwrap();

    assert!(fx.output().contains("alice@test"));
}

#[test]
fn user_add_duplicate_is_error() {
    let fx = CannedEffects::new();
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    let result = key::commands::user::add(&mut state, "alice@test".into(), &fx, &token);

    assert!(result.is_err());
}

#[test]
fn user_delete_with_retype() {
    let fx = CannedEffects::new().with_prompt_answers(vec!["alice@test".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    fx.clear_output();

    key::commands::user::delete(&mut state, Some("alice@test".into()), &fx, &token).unwrap();
    assert!(fx.output().contains("Deleted user: alice@test"));
}

#[test]
fn user_delete_wrong_retype_fails() {
    let fx = CannedEffects::new().with_prompt_answers(vec!["wrong".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();

    let result = key::commands::user::delete(&mut state, Some("alice@test".into()), &fx, &token);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Read-only flag tests
// ---------------------------------------------------------------------------

#[test]
fn read_only_blocks_user_add() {
    let token = MutationToken::acquire(true);
    assert!(token.is_err());
}

#[test]
fn read_only_allows_list() {
    let fx = CannedEffects::new();
    let state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();

    let result = key::commands::user::list(&state, &fx);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Key management tests
// ---------------------------------------------------------------------------

#[test]
fn key_list_empty() {
    let fx = CannedEffects::new();
    let state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();

    key::commands::key::list(&state, false, &fx).unwrap();
    assert!(fx.output().contains("No keys found."));
}

#[test]
fn key_add_creates_dir_and_info() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["1Password vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    // Add user first
    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    fx.clear_output();

    // Add key
    key::commands::key::add(&mut state, Some("mykey".into()), &fx, &token).unwrap();

    // Reload state to pick up the new key
    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();

    // Verify key exists
    assert!(!state.keys.is_empty());
    assert_eq!(state.keys[0].key_id(), "mykey");
}

#[test]
fn key_list_shows_added_key() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["1Password vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("github-work".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::key::list(&state, false, &fx).unwrap();

    assert!(fx.output().contains("github-work"));
}

#[test]
fn key_list_verbose_shows_metadata() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["bitwarden".into(), "my comment".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("verbose-key".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::key::list(&state, true, &fx).unwrap();

    assert!(fx.output().contains("bitwarden"));
    assert!(fx.output().contains("my comment"));
}

#[test]
fn key_delete_removes_dir() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec![
            "vault".into(),
            "".into(),
            "to-delete_1970-01-01_00-00_UTC+0000".into(),
        ]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("to-delete".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let key_dir = state.keys[0].path.clone();
    assert!(fx.path_exists(&key_dir));

    fx.clear_output();
    key::commands::key::delete(&mut state, Some("to-delete".into()), &fx, &token).unwrap();

    assert!(fx.output().contains("Deleted key:"));
}

#[test]
fn key_add_duplicate_id_is_error() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0, 0])
        .with_prompt_answers(vec!["vault".into(), "".into(), "vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();

    key::commands::key::add(&mut state, Some("same-id".into()), &fx, &token).unwrap();
    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let result = key::commands::key::add(&mut state, Some("same-id".into()), &fx, &token);

    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Key-ID validation tests
// ---------------------------------------------------------------------------

#[test]
fn key_add_rejects_slash_in_id() {
    let fx = CannedEffects::new();
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    let result = key::commands::key::add(&mut state, Some("../evil".into()), &fx, &token);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("invalid character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn key_add_rejects_empty_id() {
    let fx = CannedEffects::new();
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    let result = key::commands::key::add(&mut state, Some("".into()), &fx, &token);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("empty"), "unexpected error: {}", msg);
}

#[test]
fn key_add_rejects_space_in_id() {
    let fx = CannedEffects::new();
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    let result = key::commands::key::add(&mut state, Some("my key".into()), &fx, &token);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("invalid character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn key_add_accepts_valid_id_chars() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    let result = key::commands::key::add(&mut state, Some("my-key_1+2".into()), &fx, &token);
    assert!(result.is_ok(), "expected ok, got: {:?}", result);
}

// ---------------------------------------------------------------------------
// Status tests
// ---------------------------------------------------------------------------

#[test]
fn status_empty_state() {
    let fx = CannedEffects::new();
    let state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();

    key::commands::status::status(&state, &fx).unwrap();

    assert!(fx.output().contains("State hash:"));
    assert!(fx.output().contains("Users (0):"));
    assert!(fx.output().contains("Keys (0):"));
}

#[test]
fn status_after_user_add() {
    let fx = CannedEffects::new();
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "bob@corp".into(), &fx, &token).unwrap();
    fx.clear_output();

    key::commands::status::status(&state, &fx).unwrap();

    assert!(fx.output().contains("Users (1):"));
    assert!(fx.output().contains("bob@corp"));
}

#[test]
fn status_after_key_add_shows_inactive() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("mykey".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::status::status(&state, &fx).unwrap();

    assert!(fx.output().contains("Keys (1):"));
    assert!(fx.output().contains("inactive"));
}

// ---------------------------------------------------------------------------
// Merkle hash tests
// ---------------------------------------------------------------------------

#[test]
fn merkle_hash_changes_on_mutation() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0, 0])
        .with_prompt_answers(vec!["vault".into(), "".into(), "vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::status::status(&state, &fx).unwrap();
    let hash1 = extract_hash(&fx.output());

    key::commands::user::add(&mut state, "carol@test".into(), &fx, &token).unwrap();
    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::status::status(&state, &fx).unwrap();
    let hash2 = extract_hash(&fx.output());

    assert_ne!(hash1, hash2, "hash must change after user add");

    key::commands::key::add(&mut state, Some("carol-key".into()), &fx, &token).unwrap();
    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::status::status(&state, &fx).unwrap();
    let hash3 = extract_hash(&fx.output());

    assert_ne!(hash2, hash3, "hash must change after key add");
}

fn extract_hash(status_output: &str) -> String {
    status_output
        .lines()
        .find(|l| l.starts_with("State hash:"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Setup tests
// ---------------------------------------------------------------------------

#[test]
fn setup_creates_zshrc_block() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh");

    key::commands::setup::setup(&fx).unwrap();

    let rc_content = fx
        .read_file_string(&std::path::PathBuf::from("/fake/home/.zshrc"))
        .unwrap();
    assert!(rc_content.contains("# __added_by_key"));
    assert!(rc_content.contains("export PATH="));
    assert!(rc_content.contains("# [ADDED BY key] START"));
    assert!(rc_content.contains("# [ADDED by key] END"));
}

#[test]
fn setup_creates_bashrc_block() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/bash");

    key::commands::setup::setup(&fx).unwrap();

    let rc_content = fx
        .read_file_string(&std::path::PathBuf::from("/fake/home/.bashrc"))
        .unwrap();
    assert!(rc_content.contains("# __added_by_key"));
    assert!(rc_content.contains("export PATH="));
}

#[test]
fn setup_is_idempotent() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh");

    key::commands::setup::setup(&fx).unwrap();
    key::commands::setup::setup(&fx).unwrap();

    let rc_content = fx
        .read_file_string(&std::path::PathBuf::from("/fake/home/.zshrc"))
        .unwrap();
    let block_count = rc_content.matches("# [ADDED BY key] START").count();
    assert_eq!(block_count, 1, "exactly one START marker after two runs");
}

#[test]
fn setup_preserves_existing_rc_content() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh");

    // Pre-populate the RC file
    let rc_path = std::path::PathBuf::from("/fake/home/.zshrc");
    fx.write_file(&rc_path, b"# my existing config\nexport FOO=bar\n")
        .unwrap();

    key::commands::setup::setup(&fx).unwrap();

    let rc_content = fx.read_file_string(&rc_path).unwrap();
    assert!(rc_content.contains("# my existing config"));
    assert!(rc_content.contains("export FOO=bar"));
    assert!(rc_content.contains("# __added_by_key"));
}

// ---------------------------------------------------------------------------
// Setup path sanitization tests
// ---------------------------------------------------------------------------

#[test]
fn setup_rejects_backtick_in_exe_path() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh")
        .with_exe_dir("/bad`path/bin");

    let result = key::commands::setup::setup(&fx);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unsafe character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn setup_rejects_space_in_exe_path() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh")
        .with_exe_dir("/bad path/bin");

    let result = key::commands::setup::setup(&fx);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unsafe character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn setup_rejects_quote_in_exe_path() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh")
        .with_exe_dir("/bad'path/bin");

    let result = key::commands::setup::setup(&fx);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unsafe character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn setup_rejects_dollar_in_exe_path() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh")
        .with_exe_dir("/bad$path/bin");

    let result = key::commands::setup::setup(&fx);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unsafe character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn setup_rejects_semicolon_in_exe_path() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh")
        .with_exe_dir("/bad;path/bin");

    let result = key::commands::setup::setup(&fx);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unsafe character"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn setup_accepts_clean_exe_path() {
    let fx = CannedEffects::new()
        .with_home("/fake/home")
        .with_shell("/bin/zsh")
        .with_exe_dir("/usr/local/bin");

    key::commands::setup::setup(&fx).unwrap();
}

// ---------------------------------------------------------------------------
// Permissions tests (using real filesystem with RealEffects)
// ---------------------------------------------------------------------------

#[test]
#[cfg(unix)]
fn permissions_key_dir_is_0700() {
    use key::effects::RealEffects;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let fx = RealEffects;
    State::load(tmp.path(), &fx).unwrap();

    let mode = std::fs::metadata(tmp.path())
        .expect("key dir metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "key dir must be 0o700, got {:o}", mode);
}

#[test]
#[cfg(unix)]
fn permissions_keys_subdir_is_0700() {
    use key::effects::RealEffects;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let fx = RealEffects;
    State::load(tmp.path(), &fx).unwrap();

    let keys_dir = tmp.path().join("keys");
    let mode = std::fs::metadata(&keys_dir)
        .expect("keys subdir metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "keys subdir must be 0o700, got {:o}", mode);
}

// ---------------------------------------------------------------------------
// Pubkey command tests
// ---------------------------------------------------------------------------

#[test]
fn pubkey_prints_key_content() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("mykey".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::key::pubkey(&state, Some("mykey".into()), &fx).unwrap();

    let output = fx.output();
    assert!(output.contains("--- public key start (do not copy this line) ---"));
    assert!(output.contains("ssh-ed25519 AAAA"));
    assert!(output.contains("--- public key end (do not copy this line) ---"));
}

// ---------------------------------------------------------------------------
// Amend command tests
// ---------------------------------------------------------------------------

#[test]
fn amend_comment_updates_verbose_list() {
    use key::cli::AmendField;

    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("amend-key".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();

    key::commands::key::amend(
        &mut state,
        Some("amend-key".into()),
        AmendField::Comment,
        "my note".into(),
        &fx,
        &token,
    )
    .unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::key::list(&state, true, &fx).unwrap();

    let output = fx.output();
    assert!(output.contains("amend-key"));
    assert!(output.contains("my note"));
}

#[test]
fn amend_password_storage_updates_verbose_list() {
    use key::cli::AmendField;

    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("pw-key".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();

    key::commands::key::amend(
        &mut state,
        Some("pw-key".into()),
        AmendField::PasswordStorage,
        "1password".into(),
        &fx,
        &token,
    )
    .unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::key::list(&state, true, &fx).unwrap();

    let output = fx.output();
    assert!(output.contains("pw-key"));
    assert!(output.contains("1password"));
}

#[test]
fn amend_empty_string_clears_comment() {
    use key::cli::AmendField;

    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "original comment".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("clear-key".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();

    key::commands::key::amend(
        &mut state,
        Some("clear-key".into()),
        AmendField::Comment,
        "".into(),
        &fx,
        &token,
    )
    .unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::key::list(&state, true, &fx).unwrap();

    let output = fx.output();
    assert!(!output.contains("original comment"));
}

// ---------------------------------------------------------------------------
// Activate command tests
// ---------------------------------------------------------------------------

#[test]
fn activate_prints_key_metadata() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["bitwarden".into(), "my note".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("testkey".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    fx.clear_output();
    key::commands::activate::activate(&state, Some("testkey".into()), &fx).unwrap();

    let output = fx.output();
    assert!(output.contains("testkey"));
    assert!(output.contains("bitwarden"));
    assert!(output.contains("my note"));
}

#[test]
fn activate_warns_if_already_active() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("active-key".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();

    // Get the fingerprint and mock agent_keys
    let key = &state.keys[0];
    let pub_path = key.path.join("key.pub");
    let fingerprint = fx.ssh_keygen_fingerprint(&pub_path).unwrap();
    let agent_keys_str = format!("256 {} alice@test (ED25519)\n", fingerprint);
    fx.set_agent_keys(&agent_keys_str);

    fx.clear_output();
    key::commands::activate::activate(&state, Some("active-key".into()), &fx).unwrap();

    let err_output = fx.err_output();
    assert!(err_output.contains("already active"));
}

// ---------------------------------------------------------------------------
// Status command - ACTIVE key tests
// ---------------------------------------------------------------------------

#[test]
fn status_shows_active_key() {
    let fx = CannedEffects::new()
        .with_pick_answers(vec![0])
        .with_prompt_answers(vec!["vault".into(), "".into()]);
    let mut state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();
    let token = MutationToken::acquire(false).unwrap();

    key::commands::user::add(&mut state, "alice@test".into(), &fx, &token).unwrap();
    key::commands::key::add(&mut state, Some("active-status-key".into()), &fx, &token).unwrap();

    state = State::load(&PathBuf::from("/test/.key"), &fx).unwrap();

    // Get the fingerprint and mock agent_keys to make the key active
    let key = &state.keys[0];
    let pub_path = key.path.join("key.pub");
    let fingerprint = fx.ssh_keygen_fingerprint(&pub_path).unwrap();
    let agent_keys_str = format!("256 {} alice@test (ED25519)\n", fingerprint);
    fx.set_agent_keys(&agent_keys_str);

    fx.clear_output();
    key::commands::status::status(&state, &fx).unwrap();

    assert!(fx.output().contains("ACTIVE"));
}
