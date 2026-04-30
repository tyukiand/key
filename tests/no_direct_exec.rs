//! Lint test (spec/0014 §4.1): no direct-process invocation outside the
//! security kernel.
//!
//! Walks `src/**/*.rs`, greps for `std::process::Command`, `process::Command`,
//! `Command::new`, `tokio::process`, `subprocess::`, and asserts ZERO matches
//! outside `src/security/`. The only place these tokens are allowed is
//! `src/security/exec.rs`.

use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

fn is_allowlisted(rel: &Path) -> bool {
    // The kernel module file is the one place `std::process::Command` may
    // appear. Allow the whole `src/security/` subtree for forward-compat
    // (e.g. submodules added later for seccomp etc.).
    let s = rel.to_string_lossy().replace('\\', "/");
    s.starts_with("src/security/")
}

#[test]
fn no_direct_exec_outside_security_kernel() {
    let root = manifest_dir().join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);

    // Patterns that indicate a direct subprocess invocation. The lint is
    // intentionally textual — it's a tripwire, not a parser.
    let needles: &[&str] = &[
        "std::process::Command",
        "process::Command",
        "Command::new",
        "tokio::process",
        "subprocess::",
    ];

    let manifest = manifest_dir();
    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        let rel = file.strip_prefix(&manifest).unwrap_or(file);
        if is_allowlisted(rel) {
            continue;
        }
        let body = match std::fs::read_to_string(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for (lineno, line) in body.lines().enumerate() {
            for needle in needles {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{}: direct exec token `{}` — must go through \
                         key::security::exec::safe_exec",
                        rel.display(),
                        lineno + 1,
                        needle
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found {} direct-exec violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
