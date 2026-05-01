//! Lint test (spec/0017 §A.3): no direct env reads outside the security
//! kernel.
//!
//! Walks `src/**/*.rs`, greps for `std::env::vars` and `std::env::var`,
//! and asserts ZERO matches outside `src/security/*` and `src/main.rs`.
//! All other code must call `OsEffects::env_vars()` / `OsEffects::env_var()`
//! so the redaction filter (spec/0017 §B.7) can run before the value
//! leaves the kernel.
//!
//! The macros `env!` and `option_env!` are compile-time, not runtime, and
//! are never matched by these patterns; they remain freely usable.

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
    let s = rel.to_string_lossy().replace('\\', "/");
    // Spec/0017 §A.0: the security kernel is the SOLE production-code home
    // for raw env reads. `src/main.rs` is allowed for one-time entry-point
    // bootstrap.
    s.starts_with("src/security/") || s == "src/main.rs"
}

/// Mark the line ranges that belong to `#[cfg(test)]` (or `#[cfg(any(test, …))]`)
/// modules, plus any `#[test]`-attributed function. These are unit tests
/// embedded in the source file; the lint focuses on production paths.
fn cfg_test_line_ranges(body: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = body.lines().collect();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim_start();
        let triggers = l.starts_with("#[cfg(test)]")
            || l.starts_with("#[cfg(any(test")
            || l.starts_with("#[cfg(any(feature = \"testing\"")
            || l.starts_with("#[test]")
            || l.starts_with("#[cfg_attr(") && (l.contains("test") || l.contains("\"testing\""))
            || l.contains("#[cfg(any(test, feature = \"testing\"))]");
        if triggers {
            let mut j = i;
            let mut found_open = false;
            let mut depth: i32 = 0;
            while j < lines.len() {
                for ch in lines[j].chars() {
                    if ch == '{' {
                        depth += 1;
                        found_open = true;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }
                if found_open && depth == 0 {
                    ranges.push((i, j));
                    break;
                }
                j += 1;
            }
            if found_open {
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    ranges
}

fn line_in_ranges(line_idx: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|(s, e)| line_idx >= *s && line_idx <= *e)
}

#[test]
fn no_direct_env_outside_security_kernel() {
    let root = manifest_dir().join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);

    // Patterns that indicate a direct env read at runtime. `env!` /
    // `option_env!` are compile-time and never match these tokens.
    let needles: &[&str] = &["std::env::vars", "std::env::var"];

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
        let test_ranges = cfg_test_line_ranges(&body);
        for (lineno, line) in body.lines().enumerate() {
            if line_in_ranges(lineno, &test_ranges) {
                continue;
            }
            for needle in needles {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{}: direct-env token `{}` — must go through \
                         key::security::os_effects::OsEffects::env_vars / env_var",
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
        "Found {} direct-env violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
