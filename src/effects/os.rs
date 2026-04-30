//! `OsEffects` capability handle — see
//! `spec/0016-os-effects-and-project-only.txt` §A.
//!
//! Single point through which every filesystem / process / env / clock
//! side-effect must flow. The `OsEffectsRo` / `OsEffectsRw` split is enforced
//! at the type level: a function that asks for `&dyn OsEffectsRo` cannot mutate
//! the host (cannot call `write_file`, `create_dir_all`, `safe_exec`, …).
//!
//! Production binaries inject `RealOsEffects` from `main.rs`; tests inject
//! `MockOsEffects` (in-memory FS, programmable command results, frozen clock,
//! fake env). NO global / NO thread-local / NO static — the handle is a
//! parameter, end-to-end.

// MockOsEffects + several methods are exercised primarily by the test suite
// and the `key audit project edit` end-to-end test (spec/0016 §D).
#![allow(dead_code)]

#[cfg(feature = "testing")]
use anyhow::bail;
use anyhow::{Context, Result};
#[cfg(feature = "testing")]
use std::cell::RefCell;
#[cfg(feature = "testing")]
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::security::exec::{safe_exec_impl, AllowedCommand, SafeExecResult};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Lightweight `Metadata` projection — enough to support the call sites
/// `Project::load_from_dir` / fixture walking / `is_executable_file` need
/// without leaking `std::fs::Metadata` (which carries platform-specific noise
/// the mock can't reproduce verbatim). `unix_mode` is `Some(mode_bits)` on
/// unix platforms only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataInfo {
    pub is_dir: bool,
    pub is_file: bool,
    pub len: u64,
    pub unix_mode: Option<u32>,
}

/// Lightweight `DirEntry` projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryKind {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_file: bool,
}

/// A scoped tempdir handle. Drop semantics depend on the backend:
///   - `RealOsEffects::make_tempdir` returns a real path under
///     `std::env::temp_dir()` and removes it at Drop.
///   - `MockOsEffects::make_tempdir` returns a synthetic path inside the
///     in-memory FS and removes that subtree at Drop.
pub struct TempDirHandle {
    pub(crate) path: PathBuf,
    pub(crate) cleanup: Option<Box<dyn FnOnce(&Path) + Send>>,
}

impl TempDirHandle {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirHandle {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup(&self.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Sealed marker — the Ro/Rw split is closed-world by design
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::RealOsEffects {}
    #[cfg(feature = "testing")]
    impl Sealed for super::MockOsEffects {}
}

// ---------------------------------------------------------------------------
// OsEffectsRo — read-only capability surface
// ---------------------------------------------------------------------------

/// Read-only OS capability. Cannot mutate disk, process the host, or shell out.
pub trait OsEffectsRo: sealed::Sealed {
    fn read_file(&self, p: &Path) -> Result<Vec<u8>>;
    fn read_to_string(&self, p: &Path) -> Result<String>;
    fn metadata(&self, p: &Path) -> Result<MetadataInfo>;
    fn read_dir(&self, p: &Path) -> Result<Vec<DirEntryKind>>;
    fn path_exists(&self, p: &Path) -> bool;

    fn env_var(&self, name: &str) -> Option<OsString>;
    fn now(&self) -> SystemTime;
}

// ---------------------------------------------------------------------------
// OsEffectsRw — read-write capability surface (super-trait of Ro)
// ---------------------------------------------------------------------------

/// Read-write OS capability. Adds mutators and the `safe_exec` funnel.
pub trait OsEffectsRw: OsEffectsRo {
    fn write_file(&self, p: &Path, bytes: &[u8]) -> Result<()>;
    fn create_dir_all(&self, p: &Path) -> Result<()>;
    fn remove_dir_all(&self, p: &Path) -> Result<()>;
    fn make_tempdir(&self) -> Result<TempDirHandle>;
    fn safe_exec(&self, cmd: AllowedCommand) -> SafeExecResult;
}

// ---------------------------------------------------------------------------
// Convenience union trait — most call sites take `&dyn OsEffects` because they
// need read-AND-write. Code that genuinely is read-only takes `&dyn OsEffectsRo`.
// ---------------------------------------------------------------------------

pub trait OsEffects: OsEffectsRw {}
impl<T: OsEffectsRw + ?Sized> OsEffects for T {}

// ---------------------------------------------------------------------------
// RealOsEffects — production backend (std::fs + safe_exec)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct RealOsEffects;

impl OsEffectsRo for RealOsEffects {
    fn read_file(&self, p: &Path) -> Result<Vec<u8>> {
        std::fs::read(p).with_context(|| format!("reading {}", p.display()))
    }

    fn read_to_string(&self, p: &Path) -> Result<String> {
        std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))
    }

    fn metadata(&self, p: &Path) -> Result<MetadataInfo> {
        let m = std::fs::metadata(p).with_context(|| format!("stat {}", p.display()))?;
        #[cfg(unix)]
        let unix_mode = {
            use std::os::unix::fs::MetadataExt;
            Some(m.mode())
        };
        #[cfg(not(unix))]
        let unix_mode: Option<u32> = None;
        Ok(MetadataInfo {
            is_dir: m.is_dir(),
            is_file: m.is_file(),
            len: m.len(),
            unix_mode,
        })
    }

    fn read_dir(&self, p: &Path) -> Result<Vec<DirEntryKind>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(p).with_context(|| format!("reading dir {}", p.display()))? {
            let entry = entry?;
            let ft = entry.file_type()?;
            out.push(DirEntryKind {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path(),
                is_dir: ft.is_dir(),
                is_file: ft.is_file(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn path_exists(&self, p: &Path) -> bool {
        p.exists()
    }

    fn env_var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

impl OsEffectsRw for RealOsEffects {
    fn write_file(&self, p: &Path, bytes: &[u8]) -> Result<()> {
        std::fs::write(p, bytes).with_context(|| format!("writing {}", p.display()))
    }

    fn create_dir_all(&self, p: &Path) -> Result<()> {
        std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))
    }

    fn remove_dir_all(&self, p: &Path) -> Result<()> {
        std::fs::remove_dir_all(p).with_context(|| format!("removing {}", p.display()))
    }

    fn make_tempdir(&self) -> Result<TempDirHandle> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir().join(format!("key-os-tmp-{}-{}-{}", pid, nanos, n));
        std::fs::create_dir_all(&base)
            .with_context(|| format!("creating tempdir {}", base.display()))?;
        Ok(TempDirHandle {
            path: base,
            cleanup: Some(Box::new(|p: &Path| {
                let _ = std::fs::remove_dir_all(p);
            })),
        })
    }

    fn safe_exec(&self, cmd: AllowedCommand) -> SafeExecResult {
        safe_exec_impl(cmd)
    }
}

// ---------------------------------------------------------------------------
// MockOsEffects — test-only in-memory backend
// ---------------------------------------------------------------------------

#[cfg(feature = "testing")]
pub struct MockOsEffects {
    files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    dirs: RefCell<std::collections::BTreeSet<PathBuf>>,
    env: RefCell<BTreeMap<String, OsString>>,
    /// Programmable command results: lookup by `AllowedCommand::variant_name()`.
    canned_cmds: RefCell<BTreeMap<String, SafeExecResult>>,
    /// Default frozen clock (UNIX_EPOCH); override via `with_frozen_now`.
    frozen_now: RefCell<SystemTime>,
    tempdir_counter: std::cell::Cell<u64>,
}

#[cfg(feature = "testing")]
impl Default for MockOsEffects {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "testing")]
impl MockOsEffects {
    pub fn new() -> Self {
        Self {
            files: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(std::collections::BTreeSet::new()),
            env: RefCell::new(BTreeMap::new()),
            canned_cmds: RefCell::new(BTreeMap::new()),
            frozen_now: RefCell::new(SystemTime::UNIX_EPOCH),
            tempdir_counter: std::cell::Cell::new(0),
        }
    }

    /// Seed an in-memory file (creating parent dirs as needed).
    pub fn seed_file(&self, p: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let p = p.as_ref().to_path_buf();
        if let Some(parent) = p.parent() {
            self.seed_dir(parent);
        }
        self.files.borrow_mut().insert(p, bytes.as_ref().to_vec());
    }

    /// Seed a directory (and all ancestors) in the in-memory FS.
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
    /// Strict on shape.
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

#[cfg(feature = "testing")]
impl OsEffectsRo for MockOsEffects {
    fn read_file(&self, p: &Path) -> Result<Vec<u8>> {
        self.files
            .borrow()
            .get(p)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: file not found: {}", p.display()))
    }

    fn read_to_string(&self, p: &Path) -> Result<String> {
        let bytes = self.read_file(p)?;
        Ok(String::from_utf8(bytes).context("mock: file is not UTF-8")?)
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
        self.env.borrow().get(name).cloned()
    }

    fn now(&self) -> SystemTime {
        *self.frozen_now.borrow()
    }
}

#[cfg(feature = "testing")]
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

    fn make_tempdir(&self) -> Result<TempDirHandle> {
        let path = self.next_tempdir();
        self.seed_dir(&path);
        // Cleanup in the mock backend is a no-op for reuse simplicity — tests
        // can always read what was written under the tempdir up to the next
        // make_tempdir call (paths are unique).
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_read_write_roundtrip() {
        let fx = RealOsEffects;
        let td = fx.make_tempdir().unwrap();
        let p = td.path().join("hello.txt");
        fx.write_file(&p, b"hi").unwrap();
        let body = fx.read_file(&p).unwrap();
        assert_eq!(body, b"hi");
        let s = fx.read_to_string(&p).unwrap();
        assert_eq!(s, "hi");
        let md = fx.metadata(&p).unwrap();
        assert!(md.is_file && !md.is_dir && md.len == 2);
        let entries = fx.read_dir(td.path()).unwrap();
        assert!(entries.iter().any(|e| e.name == "hello.txt"));
    }

    #[test]
    fn real_create_remove_dir() {
        let fx = RealOsEffects;
        let td = fx.make_tempdir().unwrap();
        let nested = td.path().join("a/b/c");
        fx.create_dir_all(&nested).unwrap();
        assert!(fx.path_exists(&nested));
        fx.remove_dir_all(&td.path().join("a")).unwrap();
        assert!(!fx.path_exists(&nested));
    }

    #[test]
    fn real_safe_exec_dispatches_via_security_kernel() {
        use crate::security::exec::{AllowedCommand, AllowedExecutableName};
        let fx = RealOsEffects;
        let exe = AllowedExecutableName::new("ls").unwrap();
        let result = fx.safe_exec(AllowedCommand::Which { exe });
        // Whether `which ls` succeeds depends on the host PATH; we just assert
        // the funnel produced *some* result (no panic, populated summary).
        assert!(!result.command_summary.is_empty());
    }

    #[test]
    fn real_env_and_now() {
        let fx = RealOsEffects;
        // PATH is virtually always set on unix
        let _ = fx.env_var("PATH");
        let _ = fx.now();
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_read_file_after_seed() {
        let fx = MockOsEffects::new();
        fx.seed_file("/x", b"hello");
        let bytes = fx.read_file(Path::new("/x")).unwrap();
        assert_eq!(bytes, b"hello");
        let md = fx.metadata(Path::new("/x")).unwrap();
        assert!(md.is_file && md.len == 5);
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_write_creates_parent_dir() {
        let fx = MockOsEffects::new();
        fx.write_file(Path::new("/a/b/c.txt"), b"yo").unwrap();
        assert!(fx.path_exists(Path::new("/a/b")));
        assert_eq!(fx.read_file(Path::new("/a/b/c.txt")).unwrap(), b"yo");
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_read_dir_lists_children() {
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

    #[cfg(feature = "testing")]
    #[test]
    fn mock_remove_dir_all_drops_subtree() {
        let fx = MockOsEffects::new();
        fx.write_file(Path::new("/d/x"), b"").unwrap();
        fx.write_file(Path::new("/d/sub/y"), b"").unwrap();
        fx.remove_dir_all(Path::new("/d")).unwrap();
        assert!(!fx.path_exists(Path::new("/d")));
        assert!(!fx.path_exists(Path::new("/d/sub/y")));
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_env_var_roundtrip() {
        let fx = MockOsEffects::new();
        fx.set_env("KEY_TEST", "hello");
        assert_eq!(fx.env_var("KEY_TEST").unwrap().to_string_lossy(), "hello");
        assert!(fx.env_var("KEY_MISSING").is_none());
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_now_is_frozen() {
        let fx = MockOsEffects::new();
        let a = fx.now();
        let b = fx.now();
        assert_eq!(a, b);
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_make_tempdir_unique() {
        let fx = MockOsEffects::new();
        let a = fx.make_tempdir().unwrap();
        let b = fx.make_tempdir().unwrap();
        assert_ne!(a.path(), b.path());
        assert!(fx.path_exists(a.path()));
        assert!(fx.path_exists(b.path()));
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_safe_exec_default_succeeds_with_empty_output() {
        use crate::security::exec::{AllowedCommand, AllowedExecutableName};
        let fx = MockOsEffects::new();
        let exe = AllowedExecutableName::new("ssh-keygen").unwrap();
        let r = fx.safe_exec(AllowedCommand::Which { exe });
        assert!(r.success);
    }

    #[cfg(feature = "testing")]
    #[test]
    fn mock_safe_exec_returns_canned_result() {
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

    #[cfg(feature = "testing")]
    #[test]
    fn mock_seed_from_yaml_files_and_env() {
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
