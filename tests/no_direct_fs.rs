//! Lint test (spec/0016 §A.6): no direct filesystem invocation outside the
//! OsEffects implementation surface.
//!
//! Walks `src/**/*.rs`, greps for `std::fs::`, `tokio::fs::`, `fs::File::`,
//! and asserts ZERO matches outside `src/effects/*.rs` and `src/main.rs`. The
//! only place these tokens are allowed is inside the OsEffects backends (the
//! `RealOsEffects` impl in `src/effects/os.rs`, plus the legacy `RealEffects`
//! in `src/effects/mod.rs`). All other code must thread an OsEffects handle.

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
    // The OsEffects backends are the only place direct fs calls may appear.
    // `src/main.rs` is allowed for one-time entry-point bootstrap (e.g.
    // resolving the binary location before the OsEffects handle exists), but
    // see the spec — at present it does not need any direct fs call.
    s.starts_with("src/effects/") || s == "src/main.rs"
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
            // Find the next opening `{` (end of fn signature / mod header) and
            // track brace depth until it returns to zero.
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
fn no_direct_fs_outside_effects_module() {
    let root = manifest_dir().join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);

    // Patterns that indicate a direct filesystem call. Textual tripwire,
    // mirroring tests/no_direct_exec.rs from spec/0014.
    let needles: &[&str] = &["std::fs::", "tokio::fs::", "fs::File::"];

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
            // Skip in-source unit-test modules + #[test] fns. The OsEffects
            // discipline targets production paths; tests routinely need
            // ad-hoc real-fs scaffolding.
            if line_in_ranges(lineno, &test_ranges) {
                continue;
            }
            for needle in needles {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{}: direct-fs token `{}` — must go through \
                         key::effects::OsEffects",
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
        "Found {} direct-fs violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
