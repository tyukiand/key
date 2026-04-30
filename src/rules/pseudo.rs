//! Pseudo-file resolution and materialization.
//!
//! Implements spec/0009-pseudo-files.txt:
//!  - §1.4 + §3.7: lazy resolution + per-audit-run snapshot caching.
//!  - §2.1–§2.4: `<env>` materialization (sorted `export NAME=VALUE` body)
//!    + supported / inapplicable predicate sets.
//!  - §2.5: `env_override` for hermetic tests.
//!  - §3.1–§3.3 + §3.4.1: `<executable:NAME>` PATH resolution → custom-extractor
//!    table → generic fallback loop (`--version`/`-version`/`-V`/`--help`),
//!    5s timeout, 64KiB cap, semver-ish regex extraction.
//!  - §3.4 + §3.4.1: static custom-extractor table (groups A–H).
//!  - §3.8: `executable_override` for hermetic tests.
//!  - §3.5 / §3.6 + §4.1: per-pseudo-file inapplicable-predicate listings and
//!    error-message format.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;

use crate::rules::ast::{ExecutableSnapshot, PseudoFile, PseudoFileFixture};

// ---------------------------------------------------------------------------
// EvalContext: per-audit-run state, including pseudo-file cache (spec §1.4, §3.7)
// ---------------------------------------------------------------------------

/// Per-audit-run context. Holds overrides plus a cache of materialized
/// pseudo-files so each `<env>` / `<executable:NAME>` is resolved at most once
/// per run (spec §1.4, §3.7).
pub struct EvalContext {
    pub home_dir: PathBuf,
    pub fixture: PseudoFileFixture,
    cache: RefCell<BTreeMap<PseudoFile, PseudoSnapshot>>,
}

impl EvalContext {
    pub fn new(home_dir: PathBuf) -> Self {
        EvalContext {
            home_dir,
            fixture: PseudoFileFixture::default(),
            cache: RefCell::new(BTreeMap::new()),
        }
    }

    #[allow(dead_code)]
    pub fn with_fixture(home_dir: PathBuf, fixture: PseudoFileFixture) -> Self {
        EvalContext {
            home_dir,
            fixture,
            cache: RefCell::new(BTreeMap::new()),
        }
    }

    /// Resolve a pseudo-file (cached for the lifetime of this context).
    pub fn resolve(&self, pseudo: &PseudoFile) -> PseudoSnapshot {
        if let Some(s) = self.cache.borrow().get(pseudo) {
            return s.clone();
        }
        let snap = match pseudo {
            PseudoFile::Env => materialize_env(self.fixture.env_override.as_ref()),
            PseudoFile::Executable(name) => {
                materialize_executable(name, self.fixture.executable_override.as_ref())
            }
        };
        self.cache.borrow_mut().insert(pseudo.clone(), snap.clone());
        snap
    }
}

// ---------------------------------------------------------------------------
// Materialized snapshot of a pseudo-file
// ---------------------------------------------------------------------------

/// Materialized pseudo-file: a body (text), plus pseudo-kind-specific state.
#[derive(Debug, Clone)]
pub struct PseudoSnapshot {
    pub body: String,
    #[allow(dead_code)]
    pub kind: PseudoKind,
}

#[derive(Debug, Clone)]
pub enum PseudoKind {
    Env {
        #[allow(dead_code)]
        env_map: BTreeMap<String, String>,
    },
    Executable {
        #[allow(dead_code)]
        snapshot: ExecutableSnapshot,
    },
}

// ---------------------------------------------------------------------------
// <env> materialization (spec §2.1–§2.2)
// ---------------------------------------------------------------------------

/// Build the `<env>` snapshot from either an override map or `std::env::vars()`.
pub fn materialize_env(override_map: Option<&BTreeMap<String, String>>) -> PseudoSnapshot {
    let env_map: BTreeMap<String, String> = match override_map {
        Some(m) => m.clone(),
        None => std::env::vars().collect(),
    };

    // Sorted ASCII-by-name `export NAME=VALUE` body, escaped per §2.2.
    let mut body = String::new();
    for (name, value) in &env_map {
        let escaped = escape_env_value(value);
        body.push_str("export ");
        body.push_str(name);
        body.push('=');
        body.push_str(&escaped);
        body.push('\n');
    }

    PseudoSnapshot {
        body,
        kind: PseudoKind::Env { env_map },
    }
}

/// Escape an env value so it occupies exactly one line (spec §2.2):
/// `\` → `\\`, then `\n` → literal `\n` two-char sequence.
fn escape_env_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// <executable:NAME> materialization (spec §3.1–§3.3, §3.4.1)
// ---------------------------------------------------------------------------

const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_CAP: usize = 64 * 1024;
const MAX_VERSION_LINES: usize = 16;

/// Resolve `<executable:NAME>` per spec §3.3 / §3.4.1:
///   1. PATH lookup (or override).
///   2. Custom-extractor probes.
///   3. Generic fallback loop.
fn materialize_executable(
    name: &str,
    override_map: Option<&BTreeMap<String, ExecutableSnapshot>>,
) -> PseudoSnapshot {
    // Override path is total: missing NAMEs ⇒ found=false. (§3.8)
    if let Some(map) = override_map {
        let snap = map
            .get(name)
            .cloned()
            .unwrap_or_else(|| ExecutableSnapshot::not_found(name));
        let body = render_executable_json(&snap);
        return PseudoSnapshot {
            body,
            kind: PseudoKind::Executable { snapshot: snap },
        };
    }

    // PATH resolution
    let path = which_on_path(name);
    let path = match path {
        None => {
            let snap = ExecutableSnapshot::not_found(name);
            let body = render_executable_json(&snap);
            return PseudoSnapshot {
                body,
                kind: PseudoKind::Executable { snapshot: snap },
            };
        }
        Some(p) => p,
    };

    // is_executable (mode bits)
    let executable = is_executable_file(&path);
    if !executable {
        let snap = ExecutableSnapshot {
            name: name.to_string(),
            found: true,
            executable: false,
            path: Some(path.to_string_lossy().into_owned()),
            command_full: None,
            version_full: None,
            version: None,
        };
        let body = render_executable_json(&snap);
        return PseudoSnapshot {
            body,
            kind: PseudoKind::Executable { snapshot: snap },
        };
    }

    let path_str = path.to_string_lossy().into_owned();
    let mut snap = ExecutableSnapshot {
        name: name.to_string(),
        found: true,
        executable: true,
        path: Some(path_str.clone()),
        command_full: None,
        version_full: None,
        version: None,
    };

    // Custom-extractor probes (§3.3 c, §3.4.1).
    if let Some(entry) = lookup_custom_extractor(name) {
        for probe in entry.probes {
            if let Some(out) = run_probe(&path_str, probe.flag) {
                let trimmed = trim_to_lines(&out, MAX_VERSION_LINES);
                let captured = capture_version(&trimmed, probe.version_regex);
                if let Some(v) = captured {
                    snap.command_full = Some(format!("{} {}", name, probe.flag));
                    snap.version_full = Some(trimmed);
                    snap.version = Some(v);
                    let body = render_executable_json(&snap);
                    return PseudoSnapshot {
                        body,
                        kind: PseudoKind::Executable { snapshot: snap },
                    };
                }
                // Probe produced output but no version match: keep falling through.
                // Track best-so-far for the snapshot's command-full/version-full.
                if snap.command_full.is_none() {
                    snap.command_full = Some(format!("{} {}", name, probe.flag));
                    snap.version_full = Some(trimmed);
                }
            }
        }
    }

    // Generic fallback loop (§3.3 d, §3.4.1).
    for flag in &["--version", "-version", "-V", "--help"] {
        if let Some(out) = run_probe(&path_str, flag) {
            if out.is_empty() {
                continue;
            }
            let trimmed = trim_to_lines(&out, MAX_VERSION_LINES);
            let extracted = extract_semver_ish(&trimmed);
            if snap.command_full.is_none() {
                snap.command_full = Some(format!("{} {}", name, flag));
                snap.version_full = Some(trimmed.clone());
            } else {
                // We already had a partial probe result; if the fallback succeeds in
                // extraction, prefer it for command-full/version-full.
                if extracted.is_some() {
                    snap.command_full = Some(format!("{} {}", name, flag));
                    snap.version_full = Some(trimmed.clone());
                }
            }
            if let Some(v) = extracted {
                snap.version = Some(v);
                break;
            }
        }
    }

    let body = render_executable_json(&snap);
    PseudoSnapshot {
        body,
        kind: PseudoKind::Executable { snapshot: snap },
    }
}

/// Render the §3.2 JSON shape with all keys present and stable ordering.
pub fn render_executable_json(snap: &ExecutableSnapshot) -> String {
    let value = serde_json::json!({
        "name": snap.name,
        "found": snap.found,
        "executable": snap.executable,
        "path": snap.path,
        "command-full": snap.command_full,
        "version-full": snap.version_full,
        "version": snap.version,
    });
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.is_file() && (meta.mode() & 0o111) != 0;
        }
        false
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Run NAME with FLAG; combine first 16 lines of stdout then stderr; cap output.
/// Times out at 5s, returning None on timeout/spawn failure.
fn run_probe(path: &str, flag: &str) -> Option<String> {
    let child = Command::new(path)
        .arg(flag)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    wait_with_timeout(child, SUBPROCESS_TIMEOUT)
}

fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> Option<String> {
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let start = Instant::now();
    loop {
        if let Some(out) = child.stdout.as_mut() {
            let mut tmp = [0u8; 4096];
            if let Ok(n) = out.read(&mut tmp) {
                if n > 0 && stdout_buf.len() < OUTPUT_CAP {
                    let take = (OUTPUT_CAP - stdout_buf.len()).min(n);
                    stdout_buf.extend_from_slice(&tmp[..take]);
                }
            }
        }
        if let Some(err) = child.stderr.as_mut() {
            let mut tmp = [0u8; 4096];
            if let Ok(n) = err.read(&mut tmp) {
                if n > 0 && stderr_buf.len() < OUTPUT_CAP {
                    let take = (OUTPUT_CAP - stderr_buf.len()).min(n);
                    stderr_buf.extend_from_slice(&tmp[..take]);
                }
            }
        }
        match child.try_wait() {
            Ok(Some(_status)) => {
                // Drain remaining bytes
                if let Some(out) = child.stdout.as_mut() {
                    let mut rest = Vec::new();
                    let _ = out.take(OUTPUT_CAP as u64).read_to_end(&mut rest);
                    let avail = OUTPUT_CAP.saturating_sub(stdout_buf.len());
                    let take = avail.min(rest.len());
                    stdout_buf.extend_from_slice(&rest[..take]);
                }
                if let Some(err) = child.stderr.as_mut() {
                    let mut rest = Vec::new();
                    let _ = err.take(OUTPUT_CAP as u64).read_to_end(&mut rest);
                    let avail = OUTPUT_CAP.saturating_sub(stderr_buf.len());
                    let take = avail.min(rest.len());
                    stderr_buf.extend_from_slice(&rest[..take]);
                }
                break;
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
    }

    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&stdout_buf));
    if !combined.is_empty() && !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(&String::from_utf8_lossy(&stderr_buf));
    Some(combined)
}

fn trim_to_lines(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i >= max {
            break;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.trim().to_string()
}

/// Capture group 1 of the regex, applied to `s`.
fn capture_version(s: &str, regex: &str) -> Option<String> {
    let re = Regex::new(regex).ok()?;
    let caps = re.captures(s)?;
    Some(caps.get(1)?.as_str().to_string())
}

/// Generic semver-ish extractor (§3.3 d).
fn extract_semver_ish(s: &str) -> Option<String> {
    // \b\d+\.\d+(?:\.\d+)?(?:[.+-][A-Za-z0-9.+-]+)?\b
    let re = Regex::new(r"\b(\d+\.\d+(?:\.\d+)?(?:[.+-][A-Za-z0-9.+-]+)?)\b").ok()?;
    let caps = re.captures(s)?;
    Some(caps.get(1)?.as_str().to_string())
}

// ---------------------------------------------------------------------------
// Custom-extractor table (spec §3.4)
// ---------------------------------------------------------------------------
//
// Sourcing protocol: web-search only (no installs / no subprocess execution
// of the tool itself). Each entry carries an inline `// source: <URL>` cite.
// All entries fall back to the §3.3(d) generic loop if their probes fail.
//
// Group H "ten useful programming-language toolchains" — chosen deliberately:
//   1. Scala         (scalac, scala, scala-cli, sbt)  — required by request
//   2. OCaml         (ocaml, ocamlfind, opam)         — required by request
//   3. Haskell       (ghc, cabal, stack)              — required by request
//   4. Rust          (rustc, cargo, rustup)           — group E
//   5. JavaScript/TS (node, deno, bun, tsc)           — group C
//   6. Python        (python3, pip, uv, poetry)       — group D
//   7. Kotlin        (kotlin, kotlinc)                — group B
//   8. Java          (java)                           — group A
//   9. Go            (go)                             — added below
//  10. Zig           (zig)                            — added below
// (Trimmed: lean/agda/coq/idris2 are research-language adjacent; elixir/erl are
// niche compared to Go; clang/gcc are large compiler suites that conflate
// distinct tool families; dart/swift/nim/crystal/julia/r are domain-niche.
// Go and Zig were chosen for breadth — Go covers cloud/devops infra, Zig
// covers modern systems-language tooling.)

#[allow(dead_code)]
struct CustomExtractor {
    name: &'static str,
    probes: &'static [Probe],
}

#[allow(dead_code)]
struct Probe {
    flag: &'static str,
    /// Regex with capture group 1 = version (semver-ish).
    version_regex: &'static str,
}

fn lookup_custom_extractor(name: &str) -> Option<&'static CustomExtractor> {
    CUSTOM_EXTRACTORS.iter().find(|e| e.name == name).copied()
}

// Group A — core
static EXTRACTOR_DOCKER: CustomExtractor = CustomExtractor {
    name: "docker",
    // source: https://docs.docker.com/reference/cli/docker/version/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Docker version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_GIT: CustomExtractor = CustomExtractor {
    name: "git",
    // source: https://git-scm.com/docs/git-version
    probes: &[Probe {
        flag: "--version",
        version_regex: r"git version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_JAVA: CustomExtractor = CustomExtractor {
    name: "java",
    // source: https://docs.oracle.com/en/java/javase/21/docs/specs/man/java.html#standard-options-for-java
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(?:openjdk|java) (\d+(?:\.\d+){0,2})",
    }],
};

// Group B — JVM ecosystem
static EXTRACTOR_SCALA: CustomExtractor = CustomExtractor {
    name: "scala",
    // source: https://docs.scala-lang.org/getting-started/install-scala.html
    probes: &[
        Probe {
            flag: "-version",
            version_regex: r"Scala (?:code runner |compiler )?version (\d+\.\d+\.\d+)",
        },
        Probe {
            flag: "--version",
            version_regex: r"Scala (?:code runner |compiler )?version (\d+\.\d+\.\d+)",
        },
    ],
};
static EXTRACTOR_SCALA3: CustomExtractor = CustomExtractor {
    name: "scala3",
    // source: https://docs.scala-lang.org/scala3/getting-started.html
    probes: &[Probe {
        flag: "-version",
        version_regex: r"Scala (?:code runner |compiler )?version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_SBT: CustomExtractor = CustomExtractor {
    name: "sbt",
    // source: https://www.scala-sbt.org/1.x/docs/Command-Line-Reference.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"sbt (?:script|runner) version: (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CS: CustomExtractor = CustomExtractor {
    name: "cs",
    // source: https://get-coursier.io/docs/cli-reference
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_COURSIER: CustomExtractor = CustomExtractor {
    name: "coursier",
    // source: https://get-coursier.io/docs/cli-reference (alias of cs)
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_MILL: CustomExtractor = CustomExtractor {
    name: "mill",
    // source: https://mill-build.org/mill/cli/builtin-commands.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Mill Build Tool version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_KOTLIN: CustomExtractor = CustomExtractor {
    name: "kotlin",
    // source: https://kotlinlang.org/docs/command-line.html
    probes: &[Probe {
        flag: "-version",
        version_regex: r"Kotlin version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_KOTLINC: CustomExtractor = CustomExtractor {
    name: "kotlinc",
    // source: https://kotlinlang.org/docs/command-line.html
    probes: &[Probe {
        flag: "-version",
        version_regex: r"Kotlin version (\d+\.\d+\.\d+)",
    }],
};

// Group C — JS/TS
static EXTRACTOR_NODE: CustomExtractor = CustomExtractor {
    name: "node",
    // source: https://nodejs.org/api/cli.html#-v---version
    probes: &[Probe {
        flag: "--version",
        version_regex: r"v(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_NPM: CustomExtractor = CustomExtractor {
    name: "npm",
    // source: https://docs.npmjs.com/cli/v10/commands/npm-version
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_NPX: CustomExtractor = CustomExtractor {
    name: "npx",
    // source: https://docs.npmjs.com/cli/v10/commands/npx
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_YARN: CustomExtractor = CustomExtractor {
    name: "yarn",
    // source: https://classic.yarnpkg.com/lang/en/docs/cli/version/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_PNPM: CustomExtractor = CustomExtractor {
    name: "pnpm",
    // source: https://pnpm.io/cli/help
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_DENO: CustomExtractor = CustomExtractor {
    name: "deno",
    // source: https://docs.deno.com/runtime/reference/cli/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"deno (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_BUN: CustomExtractor = CustomExtractor {
    name: "bun",
    // source: https://bun.sh/docs/cli/run
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_TSC: CustomExtractor = CustomExtractor {
    name: "tsc",
    // source: https://www.typescriptlang.org/docs/handbook/compiler-options.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_ESLINT: CustomExtractor = CustomExtractor {
    name: "eslint",
    // source: https://eslint.org/docs/latest/use/command-line-interface
    probes: &[Probe {
        flag: "--version",
        version_regex: r"v(\d+\.\d+\.\d+)",
    }],
};

// Group D — Python (omit `venv`, which is a stdlib module not a binary, per §3.4 D note)
static EXTRACTOR_PYTHON: CustomExtractor = CustomExtractor {
    name: "python",
    // source: https://docs.python.org/3/using/cmdline.html#cmdoption-V
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Python (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_PYTHON3: CustomExtractor = CustomExtractor {
    name: "python3",
    // source: https://docs.python.org/3/using/cmdline.html#cmdoption-V
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Python (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_PIP: CustomExtractor = CustomExtractor {
    name: "pip",
    // source: https://pip.pypa.io/en/stable/cli/pip/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"pip (\d+\.\d+(?:\.\d+)?)",
    }],
};
static EXTRACTOR_PIP3: CustomExtractor = CustomExtractor {
    name: "pip3",
    // source: https://pip.pypa.io/en/stable/cli/pip/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"pip (\d+\.\d+(?:\.\d+)?)",
    }],
};
static EXTRACTOR_UV: CustomExtractor = CustomExtractor {
    name: "uv",
    // source: https://docs.astral.sh/uv/reference/cli/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"uv (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_VIRTUALENV: CustomExtractor = CustomExtractor {
    name: "virtualenv",
    // source: https://virtualenv.pypa.io/en/latest/cli_interface.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"virtualenv (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_POETRY: CustomExtractor = CustomExtractor {
    name: "poetry",
    // source: https://python-poetry.org/docs/cli/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Poetry \(version (\d+\.\d+\.\d+)\)",
    }],
};
static EXTRACTOR_PYENV: CustomExtractor = CustomExtractor {
    name: "pyenv",
    // source: https://github.com/pyenv/pyenv/blob/master/COMMANDS.md
    probes: &[Probe {
        flag: "--version",
        version_regex: r"pyenv (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CONDA: CustomExtractor = CustomExtractor {
    name: "conda",
    // source: https://docs.conda.io/projects/conda/en/stable/commands/index.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"conda (\d+\.\d+\.\d+)",
    }],
};

// Group E — Rust
static EXTRACTOR_CARGO: CustomExtractor = CustomExtractor {
    name: "cargo",
    // source: https://doc.rust-lang.org/cargo/commands/cargo.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"cargo (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_RUSTC: CustomExtractor = CustomExtractor {
    name: "rustc",
    // source: https://doc.rust-lang.org/rustc/command-line-arguments.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"rustc (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_RUSTUP: CustomExtractor = CustomExtractor {
    name: "rustup",
    // source: https://rust-lang.github.io/rustup/concepts/index.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"rustup (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CLIPPY_DRIVER: CustomExtractor = CustomExtractor {
    name: "clippy-driver",
    // source: https://doc.rust-lang.org/clippy/usage.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"clippy (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CARGO_CLIPPY: CustomExtractor = CustomExtractor {
    name: "cargo-clippy",
    // source: https://doc.rust-lang.org/clippy/usage.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"clippy (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CARGO_FMT: CustomExtractor = CustomExtractor {
    name: "cargo-fmt",
    // source: https://github.com/rust-lang/rustfmt
    probes: &[Probe {
        flag: "--version",
        version_regex: r"rustfmt (\d+\.\d+\.\d+)",
    }],
};

// Group F — agentic coding tools, top 5 by web-search of currently-prominent
// CLIs (April 2026 timeframe). Seed: Claude Code; remaining 4: Aider, OpenAI
// Codex CLI, Cursor (CLI mode), Gemini CLI.
static EXTRACTOR_CLAUDE: CustomExtractor = CustomExtractor {
    name: "claude",
    // source: https://docs.claude.com/en/docs/claude-code/overview
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_AIDER: CustomExtractor = CustomExtractor {
    name: "aider",
    // source: https://aider.chat/docs/usage/commands.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"aider (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CODEX: CustomExtractor = CustomExtractor {
    name: "codex",
    // source: https://github.com/openai/codex
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_GEMINI: CustomExtractor = CustomExtractor {
    name: "gemini",
    // source: https://github.com/google-gemini/gemini-cli
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};

// Group G — IDEs / editors, top-5: VS Code, Cursor, JetBrains IDEA, Zed, Neovim.
static EXTRACTOR_CODE: CustomExtractor = CustomExtractor {
    name: "code",
    // source: https://code.visualstudio.com/docs/editor/command-line
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CURSOR: CustomExtractor = CustomExtractor {
    name: "cursor",
    // source: https://docs.cursor.com/get-started/introduction (VS Code-fork CLI)
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_IDEA: CustomExtractor = CustomExtractor {
    name: "idea",
    // source: https://www.jetbrains.com/help/idea/working-with-the-ide-features-from-command-line.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+(?:\.\d+)?)",
    }],
};
static EXTRACTOR_ZED: CustomExtractor = CustomExtractor {
    name: "zed",
    // source: https://zed.dev/docs/getting-started
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Zed (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_NVIM: CustomExtractor = CustomExtractor {
    name: "nvim",
    // source: https://neovim.io/doc/user/starting.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"NVIM v(\d+\.\d+\.\d+)",
    }],
};

// Group H — top-10 useful languages, additional toolchain entries beyond
// what groups A-E already cover (see comment block above for the chosen 10).
static EXTRACTOR_SCALAC: CustomExtractor = CustomExtractor {
    name: "scalac",
    // source: https://docs.scala-lang.org/getting-started/install-scala.html
    probes: &[Probe {
        flag: "-version",
        version_regex: r"Scala compiler version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_SCALA_CLI: CustomExtractor = CustomExtractor {
    name: "scala-cli",
    // source: https://scala-cli.virtuslab.org/docs/reference/cli-options
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Scala CLI version: (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_OCAML: CustomExtractor = CustomExtractor {
    name: "ocaml",
    // source: https://ocaml.org/manual/5.2/runtime.html
    probes: &[Probe {
        flag: "-version",
        version_regex: r"(?:The OCaml toplevel,|OCaml) version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_OCAMLFIND: CustomExtractor = CustomExtractor {
    name: "ocamlfind",
    // source: http://projects.camlcity.org/projects/dl/findlib-1.9.6/doc/ref-html/r865.html
    probes: &[Probe {
        flag: "-version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_OPAM: CustomExtractor = CustomExtractor {
    name: "opam",
    // source: https://opam.ocaml.org/doc/man/opam.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_GHC: CustomExtractor = CustomExtractor {
    name: "ghc",
    // source: https://downloads.haskell.org/ghc/latest/docs/users_guide/usage.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_CABAL: CustomExtractor = CustomExtractor {
    name: "cabal",
    // source: https://cabal.readthedocs.io/en/stable/cabal-commands.html
    probes: &[Probe {
        flag: "--version",
        version_regex: r"cabal-install version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_STACK: CustomExtractor = CustomExtractor {
    name: "stack",
    // source: https://docs.haskellstack.org/en/stable/
    probes: &[Probe {
        flag: "--version",
        version_regex: r"Version (\d+\.\d+\.\d+)",
    }],
};
static EXTRACTOR_GO: CustomExtractor = CustomExtractor {
    name: "go",
    // source: https://go.dev/doc/manage-install
    probes: &[Probe {
        flag: "version",
        version_regex: r"go version go(\d+\.\d+(?:\.\d+)?)",
    }],
};
static EXTRACTOR_ZIG: CustomExtractor = CustomExtractor {
    name: "zig",
    // source: https://ziglang.org/documentation/master/
    probes: &[Probe {
        flag: "version",
        version_regex: r"(\d+\.\d+\.\d+)",
    }],
};

static CUSTOM_EXTRACTORS: &[&CustomExtractor] = &[
    // Group A
    &EXTRACTOR_DOCKER,
    &EXTRACTOR_GIT,
    &EXTRACTOR_JAVA,
    // Group B
    &EXTRACTOR_SCALA,
    &EXTRACTOR_SCALA3,
    &EXTRACTOR_SBT,
    &EXTRACTOR_CS,
    &EXTRACTOR_COURSIER,
    &EXTRACTOR_MILL,
    &EXTRACTOR_KOTLIN,
    &EXTRACTOR_KOTLINC,
    // Group C
    &EXTRACTOR_NODE,
    &EXTRACTOR_NPM,
    &EXTRACTOR_NPX,
    &EXTRACTOR_YARN,
    &EXTRACTOR_PNPM,
    &EXTRACTOR_DENO,
    &EXTRACTOR_BUN,
    &EXTRACTOR_TSC,
    &EXTRACTOR_ESLINT,
    // Group D
    &EXTRACTOR_PYTHON,
    &EXTRACTOR_PYTHON3,
    &EXTRACTOR_PIP,
    &EXTRACTOR_PIP3,
    &EXTRACTOR_UV,
    &EXTRACTOR_VIRTUALENV,
    &EXTRACTOR_POETRY,
    &EXTRACTOR_PYENV,
    &EXTRACTOR_CONDA,
    // Group E
    &EXTRACTOR_CARGO,
    &EXTRACTOR_RUSTC,
    &EXTRACTOR_RUSTUP,
    &EXTRACTOR_CLIPPY_DRIVER,
    &EXTRACTOR_CARGO_CLIPPY,
    &EXTRACTOR_CARGO_FMT,
    // Group F
    &EXTRACTOR_CLAUDE,
    &EXTRACTOR_AIDER,
    &EXTRACTOR_CODEX,
    &EXTRACTOR_GEMINI,
    // Group G
    &EXTRACTOR_CODE,
    &EXTRACTOR_CURSOR,
    &EXTRACTOR_IDEA,
    &EXTRACTOR_ZED,
    &EXTRACTOR_NVIM,
    // Group H (additional language toolchains)
    &EXTRACTOR_SCALAC,
    &EXTRACTOR_SCALA_CLI,
    &EXTRACTOR_OCAML,
    &EXTRACTOR_OCAMLFIND,
    &EXTRACTOR_OPAM,
    &EXTRACTOR_GHC,
    &EXTRACTOR_CABAL,
    &EXTRACTOR_STACK,
    &EXTRACTOR_GO,
    &EXTRACTOR_ZIG,
];

// ---------------------------------------------------------------------------
// Helpers exposed for the predicate evaluator
// ---------------------------------------------------------------------------

/// Format the §4.1 inapplicable-predicate error.
pub fn inapplicable_predicate_message(predicate_key: &str, pseudo: &PseudoFile) -> String {
    format!(
        "predicate `{}` is not applicable to pseudo-file `{}`",
        predicate_key,
        pseudo.as_token()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_value_escaping() {
        assert_eq!(escape_env_value("plain"), "plain");
        assert_eq!(escape_env_value("a\nb"), "a\\nb");
        assert_eq!(escape_env_value("c\\d"), "c\\\\d");
        assert_eq!(escape_env_value("a\\nb"), "a\\\\nb");
    }

    #[test]
    fn materialize_env_sorted_with_override() {
        let mut m = BTreeMap::new();
        m.insert("ZED".to_string(), "1".to_string());
        m.insert("ABC".to_string(), "x\ny".to_string());
        let snap = materialize_env(Some(&m));
        // sorted ascending
        assert_eq!(snap.body, "export ABC=x\\ny\nexport ZED=1\n");
    }

    #[test]
    fn extract_semver_basic() {
        assert_eq!(extract_semver_ish("foo 1.2.3 bar"), Some("1.2.3".into()));
        assert_eq!(extract_semver_ish("dummy-tool 1.2"), Some("1.2".into()));
        // The spec-mandated regex uses `\b` boundaries; for "v1.2.3..." there is
        // no word boundary between `v` and `1`, so the leading `1` is skipped.
        // Tools that emit `vX.Y.Z` (node, eslint, nvim) get custom extractors.
        assert_eq!(
            extract_semver_ish("Docker version 20.10.7, build f0df350"),
            Some("20.10.7".into())
        );
        assert_eq!(
            extract_semver_ish("dummy-tool 1.2.3-alpha+build.5"),
            Some("1.2.3-alpha+build.5".into())
        );
        assert_eq!(extract_semver_ish("nothing here"), None);
    }

    #[test]
    fn render_executable_json_has_all_keys() {
        let snap = ExecutableSnapshot::not_found("foo");
        let body = render_executable_json(&snap);
        for key in &[
            "name",
            "found",
            "executable",
            "path",
            "command-full",
            "version-full",
            "version",
        ] {
            assert!(body.contains(key), "missing key {} in {}", key, body);
        }
    }

    #[test]
    fn custom_extractor_table_has_all_required_groups() {
        // Spot-check coverage so missing entries fail loud.
        for required in &[
            "docker",
            "git",
            "java",
            "scala",
            "scala3",
            "sbt",
            "cs",
            "coursier",
            "mill",
            "kotlin",
            "kotlinc",
            "node",
            "npm",
            "npx",
            "yarn",
            "pnpm",
            "deno",
            "bun",
            "tsc",
            "eslint",
            "python",
            "python3",
            "pip",
            "pip3",
            "uv",
            "virtualenv",
            "poetry",
            "pyenv",
            "conda",
            "cargo",
            "rustc",
            "rustup",
            "clippy-driver",
            "cargo-clippy",
            "cargo-fmt",
            "claude",
            "aider",
            "codex",
            "gemini",
            "code",
            "cursor",
            "idea",
            "zed",
            "nvim",
            "scalac",
            "scala-cli",
            "ocaml",
            "ocamlfind",
            "opam",
            "ghc",
            "cabal",
            "stack",
            "go",
            "zig",
        ] {
            assert!(
                lookup_custom_extractor(required).is_some(),
                "missing custom extractor for {:?}",
                required
            );
        }
    }
}
