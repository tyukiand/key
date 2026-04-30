//! Safe-exec kernel — the *only* place in `key` that invokes
//! `std::process::Command`. See `spec/0014-safe-exec-kernel.txt`.
//!
//! Public surface:
//!   - `AllowedCommand`: exhaustive enum of every permitted external invocation.
//!   - `safe_exec(cmd) -> SafeExecResult`: the single execution funnel.
//!   - Brand types (`AllowedExecutableName`, `AllowedKeyPath`, `AllowedComment`,
//!     `AllowedKeyType`, `AllowedVersionFlag`, `AllowedSelfArgs`) — newtype
//!     wrappers with private inner state and fallible sanitizing constructors.
//!
//! No code outside this module file may construct a `std::process::Command`.
//! That invariant is enforced by `tests/no_direct_exec.rs`.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Brand-error type
// ---------------------------------------------------------------------------

// Some brand-error variants are only constructed in tests or in code paths
// reachable through the lib but not the bin. The full enum is part of the
// stable security API, so keep them all even if individual variants are
// otherwise unreferenced from the binary.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrandError {
    InvalidExeName(String),
    InvalidKeyPath(String),
    InvalidComment(String),
    InvalidKeyType(String),
    InvalidVersionFlag(String),
}

impl fmt::Display for BrandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrandError::InvalidExeName(s) => write!(f, "invalid executable name: {:?}", s),
            BrandError::InvalidKeyPath(s) => write!(f, "invalid key path: {:?}", s),
            BrandError::InvalidComment(s) => write!(f, "invalid comment: {:?}", s),
            BrandError::InvalidKeyType(s) => write!(f, "invalid key type: {:?}", s),
            BrandError::InvalidVersionFlag(s) => write!(f, "invalid version flag: {:?}", s),
        }
    }
}

impl std::error::Error for BrandError {}

// ---------------------------------------------------------------------------
// AllowedExecutableName (spec §3.1)
// ---------------------------------------------------------------------------

/// A program name acceptable to `safe_exec`. Constructor enforces:
///   - non-empty; length ≤ 64;
///   - characters limited to ASCII alphanumerics plus `_ - . +`.
/// No path separators, no whitespace, no shell metacharacters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AllowedExecutableName(String);

impl AllowedExecutableName {
    pub fn new(s: &str) -> Result<Self, BrandError> {
        if s.is_empty() || s.len() > 64 {
            return Err(BrandError::InvalidExeName(s.to_string()));
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '+')
        {
            return Err(BrandError::InvalidExeName(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AllowedExecutableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// AllowedKeyPath (spec §3.1)
// ---------------------------------------------------------------------------

/// An SSH key file path acceptable to `safe_exec`. Constructor enforces:
///   - absolute path;
///   - contains a `.key/keys/` segment (key-store layout);
///   - no `..` components;
///   - no NUL bytes.
/// Symlink-traversal guarantees beyond the subtree are best-effort: we do not
/// dereference symlinks at construction time; callers are responsible for
/// constructing the path from trusted state (key id + home dir), and the
/// constructor catches accidental misuse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AllowedKeyPath(PathBuf);

impl AllowedKeyPath {
    pub fn new(path: &Path) -> Result<Self, BrandError> {
        let s = match path.to_str() {
            Some(s) => s,
            None => return Err(BrandError::InvalidKeyPath(format!("{}", path.display()))),
        };
        if s.is_empty() || s.contains('\0') {
            return Err(BrandError::InvalidKeyPath(s.to_string()));
        }
        if !path.is_absolute() {
            return Err(BrandError::InvalidKeyPath(s.to_string()));
        }
        for comp in path.components() {
            if matches!(comp, std::path::Component::ParentDir) {
                return Err(BrandError::InvalidKeyPath(s.to_string()));
            }
        }
        // Must live under a `.key/keys/<id>/` subtree somewhere in the path.
        let mut prev_was_dotkey = false;
        let mut found = false;
        for comp in path.components() {
            let name = match comp {
                std::path::Component::Normal(os) => os.to_string_lossy().into_owned(),
                _ => {
                    prev_was_dotkey = false;
                    continue;
                }
            };
            if prev_was_dotkey && name == "keys" {
                found = true;
                break;
            }
            prev_was_dotkey = name == ".key";
        }
        if !found {
            return Err(BrandError::InvalidKeyPath(s.to_string()));
        }
        Ok(Self(path.to_path_buf()))
    }

    #[allow(dead_code)]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

impl fmt::Display for AllowedKeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

// ---------------------------------------------------------------------------
// AllowedComment (spec §3.1)
// ---------------------------------------------------------------------------

/// A `-C <comment>` argument to ssh-keygen. Constructor enforces:
///   - printable ASCII (0x20..=0x7e), no control characters;
///   - length 1..=255;
///   - excludes shell metacharacters: `` ` $ \ ; | & < > * ? ( ) { } [ ] ! " ' \n \r ``.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AllowedComment(String);

impl AllowedComment {
    pub fn new(s: &str) -> Result<Self, BrandError> {
        if s.is_empty() || s.len() > 255 {
            return Err(BrandError::InvalidComment(s.to_string()));
        }
        for ch in s.chars() {
            if !(0x20u32..=0x7e).contains(&(ch as u32)) {
                return Err(BrandError::InvalidComment(s.to_string()));
            }
            if matches!(
                ch,
                '`' | '$'
                    | '\\'
                    | ';'
                    | '|'
                    | '&'
                    | '<'
                    | '>'
                    | '*'
                    | '?'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | '!'
                    | '"'
                    | '\''
            ) {
                return Err(BrandError::InvalidComment(s.to_string()));
            }
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AllowedComment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// AllowedKeyType (spec §3.1)
// ---------------------------------------------------------------------------

/// `-t <key_type>` value for ssh-keygen. Closed set per spec. Production code
/// currently only constructs `Ed25519`; the remaining variants and the
/// fallible `new()` constructor are part of the brand's API surface.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllowedKeyType {
    Ed25519,
    Rsa4096,
    EcdsaP256,
    Ed25519Sk,
}

impl AllowedKeyType {
    #[allow(dead_code)]
    pub fn new(s: &str) -> Result<Self, BrandError> {
        match s {
            "ed25519" => Ok(Self::Ed25519),
            "rsa-4096" => Ok(Self::Rsa4096),
            "ecdsa-p256" => Ok(Self::EcdsaP256),
            "ed25519-sk" => Ok(Self::Ed25519Sk),
            other => Err(BrandError::InvalidKeyType(other.to_string())),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::Rsa4096 => "rsa-4096",
            Self::EcdsaP256 => "ecdsa-p256",
            Self::Ed25519Sk => "ed25519-sk",
        }
    }

    /// The (`-t`, [`-b`...]) argv slice ssh-keygen actually consumes.
    fn ssh_keygen_args(&self) -> &'static [&'static str] {
        match self {
            Self::Ed25519 => &["-t", "ed25519"],
            Self::Rsa4096 => &["-t", "rsa", "-b", "4096"],
            Self::EcdsaP256 => &["-t", "ecdsa", "-b", "256"],
            Self::Ed25519Sk => &["-t", "ed25519-sk"],
        }
    }
}

impl fmt::Display for AllowedKeyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// AllowedVersionFlag (spec §3.1)
// ---------------------------------------------------------------------------

/// `<exe> <flag>` flag for version probing. Closed set covering every flag the
/// in-tree version-probe extractors actually use:
///   `--version`, `-version`, `-V`, `--help`, `version`.
/// (Spec §3.1 lists the first four explicitly; `version` is added because the
/// `go`/`zig` extractors invoke `<exe> version` with no leading dash.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllowedVersionFlag {
    DoubleDashVersion,
    DashVersion,
    DashV,
    DoubleDashHelp,
    BareVersion,
}

impl AllowedVersionFlag {
    pub fn new(s: &str) -> Result<Self, BrandError> {
        match s {
            "--version" => Ok(Self::DoubleDashVersion),
            "-version" => Ok(Self::DashVersion),
            "-V" => Ok(Self::DashV),
            "--help" => Ok(Self::DoubleDashHelp),
            "version" => Ok(Self::BareVersion),
            other => Err(BrandError::InvalidVersionFlag(other.to_string())),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DoubleDashVersion => "--version",
            Self::DashVersion => "-version",
            Self::DashV => "-V",
            Self::DoubleDashHelp => "--help",
            Self::BareVersion => "version",
        }
    }
}

impl fmt::Display for AllowedVersionFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// AllowedSelfArgs (spec §3.1, §5 — test-harness shapes)
// ---------------------------------------------------------------------------

/// Structured arguments for the `AuditSelf` variant. Each variant corresponds
/// to one shape `tests/.../scenario_integ.rs` actually uses. No
/// arbitrary-string passthrough. (Only constructed in `#[cfg(test)]` code,
/// hence the dead-code allowance for the binary build.)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AllowedSelfArgs {
    /// `--test-only-home-dir <home> audit run --file <yaml>`
    AuditRunFile { home: PathBuf, yaml: PathBuf },

    /// `--test-only-home-dir <home> audit test <yaml> <home>
    ///  [--expect-failure-message <msg>]* [--expect-failures <n>]?`
    AuditTest {
        home: PathBuf,
        yaml: PathBuf,
        expect_failure_messages: Vec<String>,
        expect_num_failures: Option<usize>,
    },

    /// `audit project new <name>` run with `cwd = <work_dir>`.
    AuditProjectNew { work_dir: PathBuf, name: String },

    /// `audit project test` run with `cwd = <project_dir>`.
    AuditProjectTest { project_dir: PathBuf },

    /// `audit project build` run with `cwd = <project_dir>`.
    AuditProjectBuild { project_dir: PathBuf },

    /// `audit project clean` run with `cwd = <project_dir>`.
    AuditProjectClean { project_dir: PathBuf },

    /// `--test-only-home-dir <home> audit project run` run with `cwd = <project_dir>`.
    AuditProjectRun { home: PathBuf, project_dir: PathBuf },

    /// `--test-only-home-dir <home> audit install <yaml>`
    AuditInstall { home: PathBuf, yaml: PathBuf },
}

impl AllowedSelfArgs {
    fn build_argv(&self) -> Vec<String> {
        match self {
            Self::AuditRunFile { home, yaml } => vec![
                "--test-only-home-dir".into(),
                home.to_string_lossy().into_owned(),
                "audit".into(),
                "run".into(),
                "--file".into(),
                yaml.to_string_lossy().into_owned(),
            ],
            Self::AuditTest {
                home,
                yaml,
                expect_failure_messages,
                expect_num_failures,
            } => {
                let mut v: Vec<String> = vec![
                    "--test-only-home-dir".into(),
                    home.to_string_lossy().into_owned(),
                    "audit".into(),
                    "test".into(),
                    yaml.to_string_lossy().into_owned(),
                    home.to_string_lossy().into_owned(),
                ];
                for msg in expect_failure_messages {
                    v.push("--expect-failure-message".into());
                    v.push(msg.clone());
                }
                if let Some(n) = expect_num_failures {
                    v.push("--expect-failures".into());
                    v.push(n.to_string());
                }
                v
            }
            Self::AuditProjectNew { name, .. } => {
                vec!["audit".into(), "project".into(), "new".into(), name.clone()]
            }
            Self::AuditProjectTest { .. } => {
                vec!["audit".into(), "project".into(), "test".into()]
            }
            Self::AuditProjectBuild { .. } => {
                vec!["audit".into(), "project".into(), "build".into()]
            }
            Self::AuditProjectClean { .. } => {
                vec!["audit".into(), "project".into(), "clean".into()]
            }
            Self::AuditProjectRun { home, .. } => vec![
                "--test-only-home-dir".into(),
                home.to_string_lossy().into_owned(),
                "audit".into(),
                "project".into(),
                "run".into(),
            ],
            Self::AuditInstall { home, yaml } => vec![
                "--test-only-home-dir".into(),
                home.to_string_lossy().into_owned(),
                "audit".into(),
                "install".into(),
                yaml.to_string_lossy().into_owned(),
            ],
        }
    }

    fn cwd(&self) -> Option<&Path> {
        match self {
            Self::AuditProjectNew { work_dir, .. } => Some(work_dir),
            Self::AuditProjectTest { project_dir }
            | Self::AuditProjectBuild { project_dir }
            | Self::AuditProjectClean { project_dir } => Some(project_dir),
            Self::AuditProjectRun { project_dir, .. } => Some(project_dir),
            Self::AuditRunFile { .. } | Self::AuditTest { .. } | Self::AuditInstall { .. } => None,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            Self::AuditRunFile { .. } => "AuditRunFile",
            Self::AuditTest { .. } => "AuditTest",
            Self::AuditProjectNew { .. } => "AuditProjectNew",
            Self::AuditProjectTest { .. } => "AuditProjectTest",
            Self::AuditProjectBuild { .. } => "AuditProjectBuild",
            Self::AuditProjectClean { .. } => "AuditProjectClean",
            Self::AuditProjectRun { .. } => "AuditProjectRun",
            Self::AuditInstall { .. } => "AuditInstall",
        }
    }
}

// ---------------------------------------------------------------------------
// AllowedCommand (spec §2.1)
// ---------------------------------------------------------------------------

/// Exhaustive enumeration of every external program `key` is allowed to run.
/// Each variant maps 1:1 to a real call site in the codebase. Some variants
/// (`AuditSelf`) are only constructed in `#[cfg(test)]` integration code.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AllowedCommand {
    /// `which <exe>` — used by `check_ssh_prereqs`.
    Which { exe: AllowedExecutableName },

    /// `ssh-keygen -t <type> -f <key_path> -C <comment>`.
    SshKeygenGenerate {
        key_type: AllowedKeyType,
        comment: AllowedComment,
        key_path: AllowedKeyPath,
    },

    /// `ssh-keygen -l -E sha256 -f <pub_path>`.
    SshKeygenFingerprint { key_path: AllowedKeyPath },

    /// `ssh-add <key_path>`.
    SshAddAdd { key_path: AllowedKeyPath },

    /// `ssh-add -l`.
    SshAddList,

    /// `<exe> <flag>` — version probing for pseudo-files.
    /// `exe_path` is the resolved absolute path (post-PATH-lookup) so that we
    /// invoke the binary the caller actually intends; `exe_name` keeps the
    /// human-meaningful name for command-summary logging.
    ProbeVersionGeneric {
        #[allow(dead_code)]
        exe_name: AllowedExecutableName,
        exe_path: AllowedExecutablePath,
        flag: AllowedVersionFlag,
    },

    /// Re-invoke the test binary itself (integration scenarios). The exact
    /// argv shape is gated by `AllowedSelfArgs`.
    AuditSelf {
        binary: AllowedExecutablePath,
        args: AllowedSelfArgs,
    },
}

impl AllowedCommand {
    fn variant_name(&self) -> &'static str {
        match self {
            Self::Which { .. } => "Which",
            Self::SshKeygenGenerate { .. } => "SshKeygenGenerate",
            Self::SshKeygenFingerprint { .. } => "SshKeygenFingerprint",
            Self::SshAddAdd { .. } => "SshAddAdd",
            Self::SshAddList => "SshAddList",
            Self::ProbeVersionGeneric { .. } => "ProbeVersionGeneric",
            Self::AuditSelf { args, .. } => match args.variant_name() {
                "AuditRunFile" => "AuditSelf::AuditRunFile",
                "AuditTest" => "AuditSelf::AuditTest",
                "AuditProjectNew" => "AuditSelf::AuditProjectNew",
                "AuditProjectTest" => "AuditSelf::AuditProjectTest",
                "AuditProjectBuild" => "AuditSelf::AuditProjectBuild",
                "AuditProjectClean" => "AuditSelf::AuditProjectClean",
                "AuditProjectRun" => "AuditSelf::AuditProjectRun",
                "AuditInstall" => "AuditSelf::AuditInstall",
                _ => "AuditSelf::Unknown",
            },
        }
    }
}

// ---------------------------------------------------------------------------
// AllowedExecutablePath — narrower than AllowedKeyPath; for resolved
// program paths from PATH lookup or `current_exe`. Construction is fallible
// but lenient: must be absolute, no `..`, no NUL.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AllowedExecutablePath(PathBuf);

impl AllowedExecutablePath {
    pub fn new(path: &Path) -> Result<Self, BrandError> {
        let s = match path.to_str() {
            Some(s) => s,
            None => return Err(BrandError::InvalidExeName(format!("{}", path.display()))),
        };
        if s.is_empty() || s.contains('\0') {
            return Err(BrandError::InvalidExeName(s.to_string()));
        }
        if !path.is_absolute() {
            return Err(BrandError::InvalidExeName(s.to_string()));
        }
        for comp in path.components() {
            if matches!(comp, std::path::Component::ParentDir) {
                return Err(BrandError::InvalidExeName(s.to_string()));
            }
        }
        Ok(Self(path.to_path_buf()))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

impl fmt::Display for AllowedExecutablePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

// ---------------------------------------------------------------------------
// safe_exec
// ---------------------------------------------------------------------------

/// Per-stream capture cap for piped stdout/stderr (spec §5.1.d).
const CAPTURE_CAP: usize = 1024 * 1024;
/// Marker appended on capture-cap overflow.
const TRUNCATION_MARKER: &str = "\n[...truncated by safe_exec, output exceeded 1 MiB...]";
/// Default per-call timeout (spec §5.1.c).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct SafeExecResult {
    /// Process exit code, or `None` if the process was killed by signal.
    pub exit: Option<i32>,
    /// Whether the process exited successfully.
    pub success: bool,
    /// Captured stdout (UTF-8 lossy). Empty if the variant inherits stdio.
    pub stdout: String,
    /// Captured stderr (UTF-8 lossy). Empty if the variant inherits stdio.
    pub stderr: String,
    /// Variant name for logging / error messages. Always populated; callers
    /// may surface it in panic / error formatting (some currently don't).
    #[allow(dead_code)]
    pub command_summary: String,
}

impl SafeExecResult {
    fn timed_out(summary: String) -> Self {
        Self {
            exit: None,
            success: false,
            stdout: String::new(),
            stderr: String::new(),
            command_summary: summary,
        }
    }
}

/// Internal IO mode: `Inherit` for interactive commands (passphrase prompts),
/// `Capture` for everything else.
#[derive(Debug, Clone, Copy)]
enum IoMode {
    Inherit,
    Capture,
}

/// Resolved execution plan derived from an `AllowedCommand`.
struct ExecPlan {
    program: PathBuf,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    timeout: Duration,
    io_mode: IoMode,
}

fn plan(cmd: &AllowedCommand) -> ExecPlan {
    match cmd {
        AllowedCommand::Which { exe } => ExecPlan {
            program: PathBuf::from("which"),
            args: vec![exe.as_str().to_string()],
            cwd: None,
            timeout: DEFAULT_TIMEOUT,
            io_mode: IoMode::Capture,
        },
        AllowedCommand::SshKeygenGenerate {
            key_type,
            comment,
            key_path,
        } => {
            let mut args: Vec<String> = key_type
                .ssh_keygen_args()
                .iter()
                .map(|s| s.to_string())
                .collect();
            args.extend([
                "-f".into(),
                key_path.as_str().to_string(),
                "-C".into(),
                comment.as_str().to_string(),
            ]);
            ExecPlan {
                program: PathBuf::from("ssh-keygen"),
                args,
                cwd: None,
                timeout: DEFAULT_TIMEOUT,
                io_mode: IoMode::Inherit,
            }
        }
        AllowedCommand::SshKeygenFingerprint { key_path } => ExecPlan {
            program: PathBuf::from("ssh-keygen"),
            args: vec![
                "-l".into(),
                "-E".into(),
                "sha256".into(),
                "-f".into(),
                key_path.as_str().to_string(),
            ],
            cwd: None,
            timeout: DEFAULT_TIMEOUT,
            io_mode: IoMode::Capture,
        },
        AllowedCommand::SshAddAdd { key_path } => ExecPlan {
            program: PathBuf::from("ssh-add"),
            args: vec![key_path.as_str().to_string()],
            cwd: None,
            timeout: DEFAULT_TIMEOUT,
            io_mode: IoMode::Inherit,
        },
        AllowedCommand::SshAddList => ExecPlan {
            program: PathBuf::from("ssh-add"),
            args: vec!["-l".into()],
            cwd: None,
            timeout: DEFAULT_TIMEOUT,
            io_mode: IoMode::Capture,
        },
        AllowedCommand::ProbeVersionGeneric { exe_path, flag, .. } => ExecPlan {
            program: exe_path.as_path().to_path_buf(),
            args: vec![flag.as_str().to_string()],
            cwd: None,
            // Per existing pseudo.rs behavior: 5s subprocess timeout.
            timeout: Duration::from_secs(5),
            io_mode: IoMode::Capture,
        },
        AllowedCommand::AuditSelf { binary, args } => ExecPlan {
            program: binary.as_path().to_path_buf(),
            args: args.build_argv(),
            cwd: args.cwd().map(|p| p.to_path_buf()),
            // Tests can be slow on cold caches; allow a generous timeout.
            timeout: Duration::from_secs(120),
            io_mode: IoMode::Capture,
        },
    }
}

/// The single funnel for all external-process invocation.
///
/// This is the *only* place in the crate that may call
/// `std::process::Command`. Enforced by `tests/no_direct_exec.rs`.
pub fn safe_exec(cmd: AllowedCommand) -> SafeExecResult {
    let summary = cmd.variant_name().to_string();
    debug_log(&summary);

    let plan = plan(&cmd);
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    if let Some(cwd) = &plan.cwd {
        command.current_dir(cwd);
    }
    match plan.io_mode {
        IoMode::Inherit => {
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
        IoMode::Capture => {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }
    }

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => {
            return SafeExecResult {
                exit: None,
                success: false,
                stdout: String::new(),
                stderr: String::new(),
                command_summary: summary,
            };
        }
    };

    let (status_opt, stdout, stderr) = wait_with_timeout(&mut child, plan.timeout, plan.io_mode);
    match status_opt {
        Some(status) => SafeExecResult {
            exit: status.code(),
            success: status.success(),
            stdout,
            stderr,
            command_summary: summary,
        },
        None => SafeExecResult::timed_out(summary),
    }
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    io_mode: IoMode,
) -> (Option<std::process::ExitStatus>, String, String) {
    let mut stdout_buf: Vec<u8> = Vec::new();
    let mut stderr_buf: Vec<u8> = Vec::new();
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;

    let start = Instant::now();

    loop {
        if matches!(io_mode, IoMode::Capture) {
            if let Some(out) = child.stdout.as_mut() {
                let mut tmp = [0u8; 8192];
                if let Ok(n) = out.read(&mut tmp) {
                    if n > 0 {
                        if stdout_buf.len() < CAPTURE_CAP {
                            let take = (CAPTURE_CAP - stdout_buf.len()).min(n);
                            stdout_buf.extend_from_slice(&tmp[..take]);
                            if take < n {
                                stdout_truncated = true;
                            }
                        } else {
                            stdout_truncated = true;
                        }
                    }
                }
            }
            if let Some(err) = child.stderr.as_mut() {
                let mut tmp = [0u8; 8192];
                if let Ok(n) = err.read(&mut tmp) {
                    if n > 0 {
                        if stderr_buf.len() < CAPTURE_CAP {
                            let take = (CAPTURE_CAP - stderr_buf.len()).min(n);
                            stderr_buf.extend_from_slice(&tmp[..take]);
                            if take < n {
                                stderr_truncated = true;
                            }
                        } else {
                            stderr_truncated = true;
                        }
                    }
                }
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if matches!(io_mode, IoMode::Capture) {
                    drain(&mut child.stdout, &mut stdout_buf, &mut stdout_truncated);
                    drain(&mut child.stderr, &mut stderr_buf, &mut stderr_truncated);
                }
                let mut so = String::from_utf8_lossy(&stdout_buf).into_owned();
                if stdout_truncated {
                    so.push_str(TRUNCATION_MARKER);
                }
                let mut se = String::from_utf8_lossy(&stderr_buf).into_owned();
                if stderr_truncated {
                    se.push_str(TRUNCATION_MARKER);
                }
                return (Some(status), so, se);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (None, String::new(), String::new());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                return (None, String::new(), String::new());
            }
        }
    }
}

fn drain<R: std::io::Read>(opt: &mut Option<R>, buf: &mut Vec<u8>, truncated: &mut bool) {
    if let Some(r) = opt.as_mut() {
        let mut rest = Vec::new();
        let _ = r.take((CAPTURE_CAP as u64) + 1).read_to_end(&mut rest);
        let avail = CAPTURE_CAP.saturating_sub(buf.len());
        let take = avail.min(rest.len());
        buf.extend_from_slice(&rest[..take]);
        if rest.len() > take {
            *truncated = true;
        }
    }
}

fn debug_log(variant_name: &str) {
    if std::env::var_os("KEY_DEBUG_EXEC").is_some() {
        eprintln!("[safe_exec] {}", variant_name);
    }
}

// ---------------------------------------------------------------------------
// Tests (spec §6.1)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- AllowedExecutableName ---------------------------------------------

    #[test]
    fn exe_name_happy_path() {
        for s in &[
            "ls",
            "ssh-keygen",
            "ssh-add",
            "python3",
            "scala-cli",
            "clippy-driver",
            "go",
            "x",
            "+",
            "_",
            "-",
            ".",
            "abcDEF012._-+",
            "a".repeat(64).as_str(),
        ] {
            assert!(
                AllowedExecutableName::new(s).is_ok(),
                "unexpected reject: {:?}",
                s
            );
        }
    }

    #[test]
    fn exe_name_rejects_empty() {
        assert!(matches!(
            AllowedExecutableName::new(""),
            Err(BrandError::InvalidExeName(_))
        ));
    }

    #[test]
    fn exe_name_rejects_too_long() {
        let s = "a".repeat(65);
        assert!(matches!(
            AllowedExecutableName::new(&s),
            Err(BrandError::InvalidExeName(_))
        ));
    }

    #[test]
    fn exe_name_rejects_path_separator() {
        for s in &["/usr/bin/ls", "../bin/ls", "bin/ls", "ls/", "\\ls"] {
            assert!(
                AllowedExecutableName::new(s).is_err(),
                "should reject: {:?}",
                s
            );
        }
    }

    #[test]
    fn exe_name_rejects_metacharacters() {
        for s in &[
            "ls;rm", "ls&", "ls|cat", "ls$x", "ls`x`", "ls\"q", "ls'q", "ls space", "ls\n", "ls\t",
            "ls?", "ls*", "ls(x)", "ls{x}", "ls[x]", "ls<x", "ls>x", "ls#",
        ] {
            assert!(
                AllowedExecutableName::new(s).is_err(),
                "should reject: {:?}",
                s
            );
        }
    }

    #[test]
    fn exe_name_boundary_length() {
        assert!(AllowedExecutableName::new(&"a".repeat(64)).is_ok());
        assert!(AllowedExecutableName::new(&"a".repeat(65)).is_err());
    }

    // ---- AllowedKeyPath ----------------------------------------------------

    #[test]
    fn key_path_happy_path() {
        for s in &[
            "/home/u/.key/keys/abc_2024-01-01/key",
            "/home/u/.key/keys/abc_2024-01-01/key.pub",
            "/tmp/test/.key/keys/x/key",
            "/x/.key/keys/y/key",
        ] {
            assert!(
                AllowedKeyPath::new(Path::new(s)).is_ok(),
                "should accept: {}",
                s
            );
        }
    }

    #[test]
    fn key_path_rejects_relative() {
        assert!(AllowedKeyPath::new(Path::new(".key/keys/x/key")).is_err());
    }

    #[test]
    fn key_path_rejects_no_keystore_segment() {
        assert!(AllowedKeyPath::new(Path::new("/home/u/Documents/foo")).is_err());
        assert!(AllowedKeyPath::new(Path::new("/home/u/.key/other/key")).is_err());
    }

    #[test]
    fn key_path_rejects_parent_dir() {
        assert!(AllowedKeyPath::new(Path::new("/home/u/.key/keys/../etc/passwd")).is_err());
    }

    #[test]
    fn key_path_rejects_nul() {
        let raw = "/home/u/.key/keys/x/key\0bad";
        assert!(AllowedKeyPath::new(Path::new(raw)).is_err());
    }

    // ---- AllowedComment ----------------------------------------------------

    #[test]
    fn comment_happy_path() {
        for s in &[
            "alice@example.com",
            "alice's-key",        // would be rejected — apostrophe
            "key for production", // contains space — accepted
            "user+tag@example",
        ] {
            // Skip the apostrophe one for the assertion: it should be rejected
            if s.contains('\'') {
                assert!(AllowedComment::new(s).is_err());
            } else {
                assert!(AllowedComment::new(s).is_ok(), "should accept: {:?}", s);
            }
        }
    }

    #[test]
    fn comment_rejects_empty_and_oversize() {
        assert!(AllowedComment::new("").is_err());
        let big = "a".repeat(256);
        assert!(AllowedComment::new(&big).is_err());
    }

    #[test]
    fn comment_boundary_length() {
        let ok = "a".repeat(255);
        assert!(AllowedComment::new(&ok).is_ok());
        let bad = "a".repeat(256);
        assert!(AllowedComment::new(&bad).is_err());
    }

    #[test]
    fn comment_rejects_shell_meta_and_control() {
        for s in &[
            "back`tick",
            "dollar$x",
            "back\\slash",
            "semi;colon",
            "pipe|x",
            "amp&x",
            "lt<x",
            "gt>x",
            "star*x",
            "q?x",
            "paren(x)",
            "brace{x}",
            "brack[x]",
            "bang!x",
            "dq\"x",
            "sq'x",
            "newline\nx",
            "tab\tx",
            "del\x7fx",
            "ctl\x01x",
        ] {
            assert!(AllowedComment::new(s).is_err(), "should reject: {:?}", s);
        }
    }

    // ---- AllowedKeyType ----------------------------------------------------

    #[test]
    fn key_type_happy_path() {
        assert!(matches!(
            AllowedKeyType::new("ed25519"),
            Ok(AllowedKeyType::Ed25519)
        ));
        assert!(matches!(
            AllowedKeyType::new("rsa-4096"),
            Ok(AllowedKeyType::Rsa4096)
        ));
        assert!(matches!(
            AllowedKeyType::new("ecdsa-p256"),
            Ok(AllowedKeyType::EcdsaP256)
        ));
        assert!(matches!(
            AllowedKeyType::new("ed25519-sk"),
            Ok(AllowedKeyType::Ed25519Sk)
        ));
    }

    #[test]
    fn key_type_rejects_off_set() {
        for s in &[
            "",
            "ED25519",
            "rsa",
            "rsa-2048",
            "ecdsa",
            "ecdsa-p384",
            "junk",
        ] {
            assert!(AllowedKeyType::new(s).is_err(), "should reject: {:?}", s);
        }
    }

    // ---- AllowedVersionFlag -----------------------------------------------

    #[test]
    fn version_flag_happy_path() {
        for s in &["--version", "-version", "-V", "--help", "version"] {
            assert!(AllowedVersionFlag::new(s).is_ok(), "should accept: {}", s);
        }
    }

    #[test]
    fn version_flag_rejects_off_set() {
        for s in &["", "v", "VERSION", "-v", "--Version", "--", "-h"] {
            assert!(
                AllowedVersionFlag::new(s).is_err(),
                "should reject: {:?}",
                s
            );
        }
    }

    // ---- AllowedExecutablePath --------------------------------------------

    #[test]
    fn exe_path_happy_path() {
        assert!(AllowedExecutablePath::new(Path::new("/usr/bin/ls")).is_ok());
        assert!(AllowedExecutablePath::new(Path::new("/tmp/x/key")).is_ok());
    }

    #[test]
    fn exe_path_rejects_relative_and_parent() {
        assert!(AllowedExecutablePath::new(Path::new("usr/bin/ls")).is_err());
        assert!(AllowedExecutablePath::new(Path::new("/usr/../bin/ls")).is_err());
    }

    // ---- safe_exec — minimal smoke test (spec §6.3) -----------------------

    #[test]
    fn safe_exec_runs_which_for_known_program() {
        // sh is essentially always on PATH on any unix-y CI box; if /bin/sh
        // doesn't exist this test is moot anyway.
        let r = safe_exec(AllowedCommand::Which {
            exe: AllowedExecutableName::new("sh").unwrap(),
        });
        assert_eq!(r.command_summary, "Which");
        assert!(r.success, "which sh should succeed; got {:?}", r);
    }

    #[test]
    fn safe_exec_which_unknown_program_fails() {
        let r = safe_exec(AllowedCommand::Which {
            exe: AllowedExecutableName::new("definitely-not-a-real-program-xyz").unwrap(),
        });
        assert!(!r.success);
    }
}
