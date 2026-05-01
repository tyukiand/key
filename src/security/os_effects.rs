//! `OsEffects` capability handle — see
//! `spec/0016-os-effects-and-project-only.txt` §A and
//! `spec/0017-env-redaction-and-os-effects.txt` §A.0/§B.7.
//!
//! Single point through which every filesystem / process / env / clock
//! side-effect must flow. The `OsEffectsRo` / `OsEffectsRw` split is enforced
//! at the type level: a function that asks for `&dyn OsEffectsRo` cannot mutate
//! the host (cannot call `write_file`, `create_dir_all`, `safe_exec`, …).
//!
//! Production binaries inject `RealOsEffects` from `main.rs`. Mocks live in
//! `crate::test_support::mock_os_effects` (cfg(feature = "testing")).
//!
//! Redaction (spec/0017 §B.7): every byte that crosses the OsEffects boundary
//! is filtered through `crate::security::redact::redact_value`. The
//! length-preserving REDACTED42-loop is the ONLY redaction strategy.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::security::exec::{safe_exec_impl, AllowedCommand, SafeExecResult};
use crate::security::redact::{redact_file_content, redact_value, RedactionCtx};
use crate::security::unredacted::UnredactedMatcher;

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
    pub path: PathBuf,
    pub cleanup: Option<Box<dyn FnOnce(&Path) + Send>>,
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
// Sealed marker — the Ro/Rw split is closed-world by design.
// `pub(crate)` so test-support backends in `crate::test_support` can
// implement the trait without escaping the crate.
// ---------------------------------------------------------------------------

pub mod sealed {
    pub trait Sealed {}
    impl Sealed for super::RealOsEffects {}
    #[cfg(feature = "testing")]
    impl Sealed for crate::test_support::mock_os_effects::MockOsEffects {}
}

// ---------------------------------------------------------------------------
// OsEffectsRo — read-only capability surface
// ---------------------------------------------------------------------------

/// Read-only OS capability. Cannot mutate disk, process the host, or shell out.
#[allow(dead_code)] // some methods are exposed for tests / future migration only
pub trait OsEffectsRo: sealed::Sealed {
    fn read_file(&self, p: &Path) -> Result<Vec<u8>>;
    fn read_to_string(&self, p: &Path) -> Result<String>;
    fn metadata(&self, p: &Path) -> Result<MetadataInfo>;
    fn read_dir(&self, p: &Path) -> Result<Vec<DirEntryKind>>;
    fn path_exists(&self, p: &Path) -> bool;

    fn env_var(&self, name: &str) -> Option<OsString>;
    /// Spec/0017 §A.1 — read every variable in the host environment, sorted
    /// ASCII-by-name. Each VALUE is filtered through `redact_value` with the
    /// variable's NAME as the `name_hint`, so secrets never leak.
    fn env_vars(&self) -> Vec<(String, String)>;
    fn now(&self) -> SystemTime;
    /// Directory containing the running binary (`std::env::current_exe`'s
    /// parent). Mocks return a fixed synthetic path.
    fn current_exe_dir(&self) -> Result<PathBuf>;
    /// The redaction context held by this backend (spec/0017 §C.5). Used by
    /// callers (e.g. predicate evaluation) that need the same allowlist that
    /// the OS-boundary redactor uses.
    fn redaction_ctx(&self) -> &RedactionCtx;
}

// ---------------------------------------------------------------------------
// OsEffectsRw — read-write capability surface (super-trait of Ro)
// ---------------------------------------------------------------------------

/// Read-write OS capability. Adds mutators and the `safe_exec` funnel.
pub trait OsEffectsRw: OsEffectsRo {
    fn write_file(&self, p: &Path, bytes: &[u8]) -> Result<()>;
    fn create_dir_all(&self, p: &Path) -> Result<()>;
    fn remove_dir_all(&self, p: &Path) -> Result<()>;
    fn copy_file(&self, src: &Path, dst: &Path) -> Result<u64>;
    fn set_permissions(&self, p: &Path, mode: u32) -> Result<()>;
    fn make_tempdir(&self) -> Result<TempDirHandle>;
    fn safe_exec(&self, cmd: AllowedCommand) -> SafeExecResult;
}

// ---------------------------------------------------------------------------
// Convenience union trait — most call sites take `&dyn OsEffects` because they
// need read-AND-write. Code that genuinely is read-only takes `&dyn OsEffectsRo`.
// ---------------------------------------------------------------------------

#[allow(dead_code)] // re-exported convenience union; used by lib consumers
pub trait OsEffects: OsEffectsRw {}
impl<T: OsEffectsRw + ?Sized> OsEffects for T {}

// ---------------------------------------------------------------------------
// RealOsEffects — production backend (std::fs + safe_exec)
// ---------------------------------------------------------------------------

/// Production backend. Holds an immutable redaction context for its lifetime
/// (spec/0017 §C.5).
#[derive(Debug)]
pub struct RealOsEffects {
    redaction_ctx: RedactionCtx,
}

impl Default for RealOsEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl RealOsEffects {
    /// Construct with an empty unredacted-allowlist (the safest default).
    pub fn new() -> Self {
        RealOsEffects {
            redaction_ctx: RedactionCtx::new(Vec::new()),
        }
    }

    /// Construct with a project-supplied unredacted-allowlist (spec/0017 §C.5).
    pub fn with_unredacted(matchers: Vec<UnredactedMatcher>) -> Self {
        RealOsEffects {
            redaction_ctx: RedactionCtx::new(matchers),
        }
    }
}

impl OsEffectsRo for RealOsEffects {
    fn redaction_ctx(&self) -> &RedactionCtx {
        &self.redaction_ctx
    }

    fn read_file(&self, p: &Path) -> Result<Vec<u8>> {
        let bytes = std::fs::read(p).with_context(|| format!("reading {}", p.display()))?;
        // Redact UTF-8 line-by-line; non-UTF-8 byte content is left as-is
        // (no name_hint; redaction is purely content-based for files).
        match std::str::from_utf8(&bytes) {
            Ok(s) => Ok(redact_file_content(s, &self.redaction_ctx).into_bytes()),
            Err(_) => Ok(bytes),
        }
    }

    fn read_to_string(&self, p: &Path) -> Result<String> {
        let s = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        Ok(redact_file_content(&s, &self.redaction_ctx))
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
        std::env::var_os(name).map(|v| {
            // Redact the value before it leaves the kernel boundary, with the
            // variable name as the `name_hint` for Layer-1 detection.
            let s = v.to_string_lossy();
            let redacted = redact_value(&s, &self.redaction_ctx, Some(name)).into_string();
            OsString::from(redacted)
        })
    }

    fn env_vars(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = std::env::vars().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs
            .into_iter()
            .map(|(k, v)| {
                let redacted = redact_value(&v, &self.redaction_ctx, Some(&k)).into_string();
                (k, redacted)
            })
            .collect()
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn current_exe_dir(&self) -> Result<PathBuf> {
        let exe = std::env::current_exe().context("Failed to determine executable path")?;
        exe.parent()
            .context("Executable has no parent directory")
            .map(|p| p.to_path_buf())
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

    fn copy_file(&self, src: &Path, dst: &Path) -> Result<u64> {
        std::fs::copy(src, dst)
            .with_context(|| format!("copying {} -> {}", src.display(), dst.display()))
    }

    fn set_permissions(&self, p: &Path, mode: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("Setting permissions on {}", p.display()))?;
        }
        #[cfg(not(unix))]
        let _ = (p, mode);
        Ok(())
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
// Tests — RealOsEffects only. MockOsEffects tests live alongside the mock in
// `crate::test_support::mock_os_effects`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_read_write_roundtrip() {
        let fx = RealOsEffects::new();
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
        let fx = RealOsEffects::new();
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
        let fx = RealOsEffects::new();
        let exe = AllowedExecutableName::new("ls").unwrap();
        let result = fx.safe_exec(AllowedCommand::Which { exe });
        assert!(!result.command_summary.is_empty());
    }

    #[test]
    fn real_env_and_now() {
        let fx = RealOsEffects::new();
        let _ = fx.env_var("PATH");
        let _ = fx.now();
    }

    #[test]
    fn real_env_vars_sorted_and_path_unredacted() {
        // `env_vars()` returns sorted pairs; PATH (whitelist) is verbatim.
        let fx = RealOsEffects::new();
        let pairs = fx.env_vars();
        for w in pairs.windows(2) {
            assert!(w[0].0 <= w[1].0, "env_vars must be sorted by name");
        }
        // PATH should be present with its real value (allow CI environments
        // where PATH is set; if missing, skip the assertion).
        if let Some((_, value)) = pairs.iter().find(|(k, _)| k == "PATH") {
            assert!(
                !value.is_empty(),
                "PATH should not be blanked by redaction (whitelist)"
            );
            assert!(!value.starts_with("REDACTED42"));
        }
    }

    #[test]
    fn real_current_exe_dir_returns_a_path() {
        let fx = RealOsEffects::new();
        let p = fx.current_exe_dir().unwrap();
        assert!(p.as_os_str().len() > 0);
    }
}
