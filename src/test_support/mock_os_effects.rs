//! In-memory `OsEffects` backend for tests.
//!
//! Lives at `crate::test_support::mock_os_effects` per spec/0017 §A.0
//! (test infrastructure is OUT of `src/security/`). Keeps the redaction
//! pipeline in the loop: `env_vars()` and `read_*()` route through
//! `redact_value` / `redact_file_content` so tests can assert that the
//! production path's invariants hold against a programmable backend.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};

use crate::security::exec::{AllowedCommand, SafeExecResult};
use crate::security::os_effects::{
    DirEntryKind, MetadataInfo, OsEffectsRo, OsEffectsRw, TempDirHandle,
};
use crate::security::redact::{redact_file_content, redact_value, RedactionCtx};
use crate::security::unredacted::UnredactedMatcher;

#[allow(dead_code)] // exposed to integration tests + lib consumers under feature = "testing"
pub struct MockOsEffects {
    files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    env: RefCell<BTreeMap<String, OsString>>,
    canned_cmds: RefCell<BTreeMap<String, SafeExecResult>>,
    frozen_now: RefCell<SystemTime>,
    tempdir_counter: std::cell::Cell<u64>,
    redaction_ctx: RedactionCtx,
}

impl Default for MockOsEffects {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)] // some helpers are only consumed by integration tests
impl MockOsEffects {
    pub fn new() -> Self {
        Self {
            files: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            env: RefCell::new(BTreeMap::new()),
            canned_cmds: RefCell::new(BTreeMap::new()),
            frozen_now: RefCell::new(SystemTime::UNIX_EPOCH),
            tempdir_counter: std::cell::Cell::new(0),
            redaction_ctx: RedactionCtx::empty(),
        }
    }

    /// Construct with a project-supplied unredacted-allowlist (spec/0017 §C.5).
    pub fn with_unredacted(matchers: Vec<UnredactedMatcher>) -> Self {
        Self {
            files: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            env: RefCell::new(BTreeMap::new()),
            canned_cmds: RefCell::new(BTreeMap::new()),
            frozen_now: RefCell::new(SystemTime::UNIX_EPOCH),
            tempdir_counter: std::cell::Cell::new(0),
            redaction_ctx: RedactionCtx::new(matchers),
        }
    }

    pub fn seed_file(&self, p: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let p = p.as_ref().to_path_buf();
        if let Some(parent) = p.parent() {
            self.seed_dir(parent);
        }
        self.files.borrow_mut().insert(p, bytes.as_ref().to_vec());
    }

    pub fn seed_dir(&self, p: impl AsRef<Path>) {
        let mut dirs = self.dirs.borrow_mut();
        let mut cur = p.as_ref().to_path_buf();
        loop {
            dirs.insert(cur.clone());
            match cur.parent() {
                Some(parent) if parent != cur.as_path() => cur = parent.to_path_buf(),
                _ => break,
            }
        }
    }

    pub fn set_env(&self, name: &str, value: impl Into<OsString>) {
        self.env.borrow_mut().insert(name.to_string(), value.into());
    }

    /// Program a canned `SafeExecResult` to be returned the next time
    /// `safe_exec` is invoked with a matching command variant.
    pub fn set_command_result(&self, variant: &str, result: SafeExecResult) {
        self.canned_cmds
            .borrow_mut()
            .insert(variant.to_string(), result);
    }

    pub fn with_frozen_now(self, t: SystemTime) -> Self {
        *self.frozen_now.borrow_mut() = t;
        self
    }

    /// Declarative seeding from the spec/0009 fixture YAML — accepts a top-level
    /// mapping with `files:` (path → string content) and `env:` (name → value).
    pub fn seed_from_yaml(&self, yaml: &str) -> Result<()> {
        use serde_yaml::Value;
        let val: Value = serde_yaml::from_str(yaml).context("parsing seed yaml")?;
        let map = val
            .as_mapping()
            .ok_or_else(|| anyhow::anyhow!("top-level must be a mapping"))?;
        if let Some(files) = map.get(Value::String("files".into())) {
            let m = files
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("`files:` must be a mapping"))?;
            for (k, v) in m {
                let path = k
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("file path key must be a string"))?;
                let body = v
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("file body for {:?} must be a string", path))?;
                self.seed_file(PathBuf::from(path), body.as_bytes());
            }
        }
        if let Some(env) = map.get(Value::String("env".into())) {
            let m = env
                .as_mapping()
                .ok_or_else(|| anyhow::anyhow!("`env:` must be a mapping"))?;
            for (k, v) in m {
                let name = k
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("env name key must be a string"))?;
                let value = v
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("env value for {:?} must be a string", name))?;
                self.set_env(name, value);
            }
        }
        Ok(())
    }

    fn next_tempdir(&self) -> PathBuf {
        let n = self.tempdir_counter.get();
        self.tempdir_counter.set(n + 1);
        PathBuf::from(format!("/mock/tmp/{}", n))
    }
}

impl OsEffectsRo for MockOsEffects {
    fn redaction_ctx(&self) -> &RedactionCtx {
        &self.redaction_ctx
    }

    fn read_file(&self, p: &Path) -> Result<Vec<u8>> {
        let raw = self
            .files
            .borrow()
            .get(p)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: file not found: {}", p.display()))?;
        match std::str::from_utf8(&raw) {
            Ok(s) => Ok(redact_file_content(s, &self.redaction_ctx).into_bytes()),
            Err(_) => Ok(raw),
        }
    }

    fn read_to_string(&self, p: &Path) -> Result<String> {
        let bytes = self
            .files
            .borrow()
            .get(p)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: file not found: {}", p.display()))?;
        let s = String::from_utf8(bytes).context("mock: file is not UTF-8")?;
        Ok(redact_file_content(&s, &self.redaction_ctx))
    }

    fn metadata(&self, p: &Path) -> Result<MetadataInfo> {
        if let Some(bytes) = self.files.borrow().get(p) {
            return Ok(MetadataInfo {
                is_dir: false,
                is_file: true,
                len: bytes.len() as u64,
                unix_mode: Some(0o644),
            });
        }
        if self.dirs.borrow().contains(p) {
            return Ok(MetadataInfo {
                is_dir: true,
                is_file: false,
                len: 0,
                unix_mode: Some(0o755),
            });
        }
        bail!("mock: path not found: {}", p.display())
    }

    fn read_dir(&self, p: &Path) -> Result<Vec<DirEntryKind>> {
        if !self.dirs.borrow().contains(p) {
            bail!("mock: not a directory: {}", p.display());
        }
        let mut entries: BTreeMap<String, DirEntryKind> = BTreeMap::new();
        for fp in self.files.borrow().keys() {
            if fp.parent() == Some(p) {
                if let Some(name) = fp.file_name().and_then(|n| n.to_str()) {
                    entries.insert(
                        name.to_string(),
                        DirEntryKind {
                            name: name.to_string(),
                            path: fp.clone(),
                            is_dir: false,
                            is_file: true,
                        },
                    );
                }
            }
        }
        for dp in self.dirs.borrow().iter() {
            if dp.parent() == Some(p) {
                if let Some(name) = dp.file_name().and_then(|n| n.to_str()) {
                    entries.insert(
                        name.to_string(),
                        DirEntryKind {
                            name: name.to_string(),
                            path: dp.clone(),
                            is_dir: true,
                            is_file: false,
                        },
                    );
                }
            }
        }
        Ok(entries.into_values().collect())
    }

    fn path_exists(&self, p: &Path) -> bool {
        self.files.borrow().contains_key(p) || self.dirs.borrow().contains(p)
    }

    fn env_var(&self, name: &str) -> Option<OsString> {
        self.env.borrow().get(name).cloned().map(|v| {
            let s = v.to_string_lossy();
            let red = redact_value(&s, &self.redaction_ctx, Some(name)).into_string();
            OsString::from(red)
        })
    }

    fn env_vars(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self
            .env
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned()))
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs
            .into_iter()
            .map(|(k, v)| {
                let red = redact_value(&v, &self.redaction_ctx, Some(&k)).into_string();
                (k, red)
            })
            .collect()
    }

    fn now(&self) -> SystemTime {
        *self.frozen_now.borrow()
    }

    fn current_exe_dir(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/mock/bin"))
    }
}

impl OsEffectsRw for MockOsEffects {
    fn write_file(&self, p: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = p.parent() {
            self.seed_dir(parent);
        }
        self.files
            .borrow_mut()
            .insert(p.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    fn create_dir_all(&self, p: &Path) -> Result<()> {
        self.seed_dir(p);
        Ok(())
    }

    fn remove_dir_all(&self, p: &Path) -> Result<()> {
        let mut files = self.files.borrow_mut();
        let mut dirs = self.dirs.borrow_mut();
        files.retain(|k, _| !k.starts_with(p));
        dirs.retain(|k| !k.starts_with(p));
        Ok(())
    }

    fn copy_file(&self, src: &Path, dst: &Path) -> Result<u64> {
        let bytes = self
            .files
            .borrow()
            .get(src)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: file not found: {}", src.display()))?;
        let n = bytes.len() as u64;
        self.write_file(dst, &bytes)?;
        Ok(n)
    }

    fn set_permissions(&self, _p: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }

    fn make_tempdir(&self) -> Result<TempDirHandle> {
        let path = self.next_tempdir();
        self.seed_dir(&path);
        Ok(TempDirHandle {
            path,
            cleanup: None,
        })
    }

    fn safe_exec(&self, cmd: AllowedCommand) -> SafeExecResult {
        let key = cmd.variant_name().to_string();
        if let Some(canned) = self.canned_cmds.borrow().get(&key) {
            return canned.clone();
        }
        SafeExecResult {
            exit: Some(0),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
            command_summary: key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_after_seed() {
        let fx = MockOsEffects::new();
        fx.seed_file("/x", b"hello");
        let bytes = fx.read_file(Path::new("/x")).unwrap();
        assert_eq!(bytes, b"hello");
        let md = fx.metadata(Path::new("/x")).unwrap();
        assert!(md.is_file && md.len == 5);
    }

    #[test]
    fn write_creates_parent_dir() {
        let fx = MockOsEffects::new();
        fx.write_file(Path::new("/a/b/c.txt"), b"yo").unwrap();
        assert!(fx.path_exists(Path::new("/a/b")));
        assert_eq!(fx.read_file(Path::new("/a/b/c.txt")).unwrap(), b"yo");
    }

    #[test]
    fn read_dir_lists_children() {
        let fx = MockOsEffects::new();
        fx.write_file(Path::new("/d/x"), b"").unwrap();
        fx.write_file(Path::new("/d/y"), b"").unwrap();
        fx.create_dir_all(Path::new("/d/sub")).unwrap();
        let names: Vec<String> = fx
            .read_dir(Path::new("/d"))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(
            names,
            vec!["sub".to_string(), "x".to_string(), "y".to_string()]
        );
    }

    #[test]
    fn remove_dir_all_drops_subtree() {
        let fx = MockOsEffects::new();
        fx.write_file(Path::new("/d/x"), b"").unwrap();
        fx.write_file(Path::new("/d/sub/y"), b"").unwrap();
        fx.remove_dir_all(Path::new("/d")).unwrap();
        assert!(!fx.path_exists(Path::new("/d")));
        assert!(!fx.path_exists(Path::new("/d/sub/y")));
    }

    #[test]
    fn env_var_redacts_token_by_name() {
        let fx = MockOsEffects::new();
        fx.set_env("GITHUB_TOKEN", "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB");
        let v = fx
            .env_var("GITHUB_TOKEN")
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(v.starts_with("REDACTED42"));
        assert_eq!(v.len(), "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB".len());
    }

    #[test]
    fn env_vars_sorted_and_redacted() {
        let fx = MockOsEffects::new();
        fx.set_env("Z_NORMAL", "hello");
        fx.set_env("A_TOKEN", "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB");
        fx.set_env("PATH", "/usr/bin");
        let pairs = fx.env_vars();
        assert_eq!(
            pairs.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            vec!["A_TOKEN", "PATH", "Z_NORMAL"]
        );
        assert!(pairs[0].1.starts_with("REDACTED42"));
        assert_eq!(pairs[1].1, "/usr/bin");
        assert_eq!(pairs[2].1, "hello");
    }

    #[test]
    fn allowlist_suppresses_redaction() {
        let val = "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB";
        let fx = MockOsEffects::with_unredacted(vec![UnredactedMatcher::value(val).unwrap()]);
        fx.set_env("GITHUB_TOKEN", val);
        let v = fx
            .env_var("GITHUB_TOKEN")
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(v, val);
    }

    #[test]
    fn read_file_redacts_inline_token() {
        let fx = MockOsEffects::new();
        fx.seed_file(
            "/etc/secret.conf",
            b"api_key = ghp_abcdefghijklmnopqrstuvwxyz0123456789AB\n",
        );
        let body = fx.read_to_string(Path::new("/etc/secret.conf")).unwrap();
        assert!(body.starts_with("api_key = "));
        assert!(body.contains("REDACTED42"));
    }

    #[test]
    fn now_is_frozen() {
        let fx = MockOsEffects::new();
        let a = fx.now();
        let b = fx.now();
        assert_eq!(a, b);
    }

    #[test]
    fn make_tempdir_unique() {
        let fx = MockOsEffects::new();
        let a = fx.make_tempdir().unwrap();
        let b = fx.make_tempdir().unwrap();
        assert_ne!(a.path(), b.path());
        assert!(fx.path_exists(a.path()));
        assert!(fx.path_exists(b.path()));
    }

    #[test]
    fn safe_exec_default_succeeds_with_empty_output() {
        use crate::security::exec::{AllowedCommand, AllowedExecutableName};
        let fx = MockOsEffects::new();
        let exe = AllowedExecutableName::new("ssh-keygen").unwrap();
        let r = fx.safe_exec(AllowedCommand::Which { exe });
        assert!(r.success);
    }

    #[test]
    fn safe_exec_returns_canned_result() {
        use crate::security::exec::{AllowedCommand, AllowedExecutableName};
        let fx = MockOsEffects::new();
        fx.set_command_result(
            "Which",
            SafeExecResult {
                exit: Some(1),
                success: false,
                stdout: String::new(),
                stderr: String::new(),
                command_summary: "Which".into(),
            },
        );
        let exe = AllowedExecutableName::new("nope").unwrap();
        let r = fx.safe_exec(AllowedCommand::Which { exe });
        assert!(!r.success);
    }

    #[test]
    fn seed_from_yaml_files_and_env() {
        let fx = MockOsEffects::new();
        let yaml = r#"
files:
  /etc/hello: "world"
  /home/u/.bashrc: "export X=1"
env:
  HOME: /home/u
"#;
        fx.seed_from_yaml(yaml).unwrap();
        assert_eq!(fx.read_to_string(Path::new("/etc/hello")).unwrap(), "world");
        assert_eq!(fx.env_var("HOME").unwrap().to_string_lossy(), "/home/u");
    }
}
