//! YAML fixture loader for pseudo-file overrides
//! (spec/0009 §2.5, §3.8, §3.8.1; spec/0013 §A.1, §A.6).
//!
//! Canonical fixture shape:
//!
//! ```yaml
//! env-overrides:
//!   <KEY>: <VALUE>
//!   ...
//! executable-overrides:
//!   <NAME>:
//!     name: <NAME>
//!     found: bool
//!     executable: bool
//!     path: string|null
//!     command-full: string|null
//!     version-full: string|null
//!     version: string|null
//! ```
//!
//! Backwards compatibility: the singular keys `env-override` and
//! `executable-override` are still accepted (a deprecation warning is
//! emitted once per load). Mixing the singular and plural forms of the
//! same map in one file is a hard error.
//!
//! Each field is parsed individually; malformed fixtures fail with a
//! line-numbered error.

#![allow(dead_code)]

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use serde_yaml::Value;

use crate::rules::ast::{ExecutableSnapshot, PseudoFileFixture};

/// Required keys per `executable-overrides` entry (spec/0013 §A.3 / §A.6).
pub const EXECUTABLE_ENTRY_KEYS: &[&str] = &[
    "name",
    "found",
    "executable",
    "path",
    "command-full",
    "version-full",
    "version",
];

const VALID_TOP_LEVEL_KEYS: &[&str] = &[
    "env-overrides",
    "env-override",
    "executable-overrides",
    "executable-override",
];

/// Parse a YAML fixture string into a `PseudoFileFixture`.
/// Deprecation warnings (singular key forms) are emitted to stderr.
pub fn parse_fixture(yaml: &str) -> Result<PseudoFileFixture> {
    let (fix, warnings) = parse_fixture_collect_warnings(yaml)?;
    for w in warnings {
        eprintln!("{}", w);
    }
    Ok(fix)
}

/// Like `parse_fixture` but returns the deprecation warning lines instead of
/// emitting them to stderr. Used by tests to assert warning behavior.
pub fn parse_fixture_collect_warnings(yaml: &str) -> Result<(PseudoFileFixture, Vec<String>)> {
    let value: Value = serde_yaml::from_str(yaml)
        .map_err(|e| anyhow!("invalid YAML in pseudo-file fixture: {}", e))?;
    parse_fixture_value(&value, yaml)
}

fn parse_fixture_value(value: &Value, source: &str) -> Result<(PseudoFileFixture, Vec<String>)> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("pseudo-file fixture must be a YAML mapping at the top level"))?;

    let mut fixture = PseudoFileFixture::default();
    let mut warnings: Vec<String> = Vec::new();
    let mut seen_env: Option<&'static str> = None;
    let mut seen_executable: Option<&'static str> = None;
    let mut singular_warning_emitted = false;

    for (k, v) in m.iter() {
        let key = k.as_str().ok_or_else(|| {
            anyhow!(
                "pseudo-file fixture top-level key must be a string, got {:?}",
                k
            )
        })?;
        match key {
            "env-overrides" | "env-override" => {
                let canonical = if key == "env-overrides" {
                    "env-overrides"
                } else {
                    "env-override"
                };
                if let Some(prior) = seen_env {
                    bail!(
                        "fixture has both `{}` and `{}` (line {}); use only `env-overrides`.",
                        prior,
                        key,
                        find_line(source, key).unwrap_or(0),
                    );
                }
                seen_env = Some(canonical);
                if key == "env-override" && !singular_warning_emitted {
                    warnings.push(format!(
                        "warning: fixture key `env-override` is deprecated; \
                         rename to `env-overrides` (line {} of fixture).",
                        find_line(source, "env-override").unwrap_or(0),
                    ));
                    singular_warning_emitted = true;
                }
                fixture.env_override = Some(parse_env_block(v)?);
            }
            "executable-overrides" | "executable-override" => {
                let canonical = if key == "executable-overrides" {
                    "executable-overrides"
                } else {
                    "executable-override"
                };
                if let Some(prior) = seen_executable {
                    bail!(
                        "fixture has both `{}` and `{}` (line {}); use only `executable-overrides`.",
                        prior,
                        key,
                        find_line(source, key).unwrap_or(0),
                    );
                }
                seen_executable = Some(canonical);
                if key == "executable-override" && !singular_warning_emitted {
                    warnings.push(format!(
                        "warning: fixture key `executable-override` is deprecated; \
                         rename to `executable-overrides` (line {} of fixture).",
                        find_line(source, "executable-override").unwrap_or(0),
                    ));
                    singular_warning_emitted = true;
                }
                fixture.executable_override = Some(parse_executables_block(v, source)?);
            }
            other => bail!(
                "unknown top-level key in pseudo-file fixture: {:?} (line {}). \
                 Valid keys: {}.",
                other,
                find_line(source, other).unwrap_or(0),
                VALID_TOP_LEVEL_KEYS
                    .iter()
                    .map(|k| format!("`{}`", k))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        }
    }

    Ok((fixture, warnings))
}

fn parse_env_block(value: &Value) -> Result<BTreeMap<String, String>> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("`env-overrides:` must be a mapping of NAME: VALUE entries"))?;
    let mut out = BTreeMap::new();
    for (k, v) in m.iter() {
        let key = k
            .as_str()
            .ok_or_else(|| anyhow!("env key must be a string, got {:?}", k))?;
        let val = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            other => bail!(
                "env value for {:?} must be a scalar (string/number/bool/null), got {:?}",
                key,
                other
            ),
        };
        out.insert(key.to_string(), val);
    }
    Ok(out)
}

fn parse_executables_block(
    value: &Value,
    source: &str,
) -> Result<BTreeMap<String, ExecutableSnapshot>> {
    let m = value.as_mapping().ok_or_else(|| {
        anyhow!("`executable-overrides:` must be a mapping of NAME: snapshot entries")
    })?;
    let mut out = BTreeMap::new();
    for (k, v) in m.iter() {
        let name = k
            .as_str()
            .ok_or_else(|| anyhow!("executable key must be a string, got {:?}", k))?;
        let snap = parse_executable_snapshot(name, v, source)?;
        out.insert(name.to_string(), snap);
    }
    Ok(out)
}

fn parse_executable_snapshot(
    name: &str,
    value: &Value,
    source: &str,
) -> Result<ExecutableSnapshot> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("snapshot for {:?} must be a mapping", name))?;

    // Spec/0013 §A.6 — strict-key enforcement: reject unknown keys with
    // a line-numbered error naming the entry and the offending key.
    for (k, _) in m.iter() {
        let key = k.as_str().unwrap_or("");
        if !EXECUTABLE_ENTRY_KEYS.contains(&key) {
            bail!(
                "unknown key {:?} in executable-overrides[{:?}] (line {}). \
                 Valid keys: {}.",
                key,
                name,
                find_line(source, &format!("    {}:", key)).unwrap_or(0),
                EXECUTABLE_ENTRY_KEYS.join(", "),
            );
        }
    }

    // Spec/0013 §A.3 / §A.6 — every required key MUST be present.
    let mut missing: Vec<&'static str> = Vec::new();
    for required in EXECUTABLE_ENTRY_KEYS {
        if m.get(Value::String((*required).to_string())).is_none() {
            missing.push(*required);
        }
    }
    if !missing.is_empty() {
        bail!(
            "executable-overrides[{:?}] is missing required keys: {} (line {} of fixture).",
            name,
            missing.join(", "),
            find_line(source, &format!("  {}:", name)).unwrap_or(0),
        );
    }

    // Type-checked extraction of each field.
    let entry_name = require_string(m, "name", name)?;
    if entry_name != name {
        bail!(
            "executable-overrides[{:?}] has mismatched `name:` field {:?} \
             (the entry's map key and its `name:` value MUST match).",
            name,
            entry_name,
        );
    }
    let found = require_bool(m, "found", name)?;
    let executable = require_bool(m, "executable", name)?;
    let path = nullable_string(m, "path", name)?;
    let command_full = nullable_string(m, "command-full", name)?;
    let version_full = nullable_string(m, "version-full", name)?;
    let version = nullable_string(m, "version", name)?;

    Ok(ExecutableSnapshot {
        name: name.to_string(),
        found,
        executable,
        path,
        command_full,
        version_full,
        version,
    })
}

fn require_bool(m: &serde_yaml::Mapping, key: &str, entry: &str) -> Result<bool> {
    let v = m
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("entry {:?} is missing required field {:?}", entry, key))?;
    v.as_bool().ok_or_else(|| {
        anyhow!(
            "entry {:?} field {:?} must be a bool, got {:?}",
            entry,
            key,
            v
        )
    })
}

fn require_string(m: &serde_yaml::Mapping, key: &str, entry: &str) -> Result<String> {
    let v = m
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("entry {:?} is missing required field {:?}", entry, key))?;
    match v {
        Value::String(s) => Ok(s.clone()),
        other => bail!(
            "entry {:?} field {:?} must be a string, got {:?}",
            entry,
            key,
            other
        ),
    }
}

fn nullable_string(m: &serde_yaml::Mapping, key: &str, entry: &str) -> Result<Option<String>> {
    let v = m
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("entry {:?} is missing required field {:?}", entry, key))?;
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        other => bail!(
            "entry {:?} field {:?} must be a string or null, got {:?}",
            entry,
            key,
            other
        ),
    }
}

/// Best-effort line-number lookup for error messages: search for the first
/// line containing the literal needle.
fn find_line(source: &str, needle: &str) -> Option<usize> {
    source
        .lines()
        .enumerate()
        .find_map(|(i, line)| line.contains(needle).then_some(i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_executable_entry(name: &str) -> String {
        format!(
            "  {n}:\n    name: {n}\n    found: true\n    executable: true\n    \
             path: /usr/bin/{n}\n    command-full: \"{n} --version\"\n    \
             version-full: \"{n} 1.0\"\n    version: \"1.0\"\n",
            n = name
        )
    }

    #[test]
    fn parse_canonical_plural_no_warning() {
        let yaml = format!(
            "env-overrides:\n  PATH: /usr/bin\n  HOME: /home/u\n  RUSTUP_HOME: /home/u/.rustup\n\
             executable-overrides:\n{}{}",
            full_executable_entry("docker"),
            full_executable_entry("git"),
        );
        let (fx, warns) = parse_fixture_collect_warnings(&yaml).unwrap();
        assert!(warns.is_empty(), "expected no warnings; got {:?}", warns);
        assert_eq!(fx.env_override.as_ref().unwrap().len(), 3);
        assert_eq!(fx.executable_override.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn parse_singular_emits_one_deprecation_warning() {
        let yaml = format!(
            "env-override:\n  FOO: bar\nexecutable-override:\n{}",
            full_executable_entry("docker"),
        );
        let (fx, warns) = parse_fixture_collect_warnings(&yaml).unwrap();
        // Spec/0013 §A.1 — emit ONLY ONCE per fixture load.
        assert_eq!(
            warns.len(),
            1,
            "expected exactly one warning; got {:?}",
            warns
        );
        assert!(
            warns[0].contains("env-override") || warns[0].contains("executable-override"),
            "warning should name the deprecated key; got {:?}",
            warns
        );
        assert!(fx.env_override.is_some());
        assert!(fx.executable_override.is_some());
    }

    #[test]
    fn parse_singular_only_env() {
        let yaml = "env-override:\n  FOO: bar\n";
        let (_, warns) = parse_fixture_collect_warnings(yaml).unwrap();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("env-override"));
    }

    #[test]
    fn parse_mixed_singular_and_plural_is_error() {
        let yaml = "env-override:\n  A: 1\nenv-overrides:\n  B: 2\n";
        let err = parse_fixture_collect_warnings(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("env-override"), "got {}", msg);
        assert!(msg.contains("env-overrides"), "got {}", msg);
    }

    #[test]
    fn parse_mixed_executable_singular_and_plural_is_error() {
        let yaml = format!(
            "executable-override:\n{}executable-overrides:\n{}",
            full_executable_entry("a"),
            full_executable_entry("b"),
        );
        let err = parse_fixture_collect_warnings(&yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("executable-override"));
        assert!(msg.contains("executable-overrides"));
    }

    #[test]
    fn parse_full_fixture_via_legacy_api() {
        let yaml = format!(
            "env-overrides:\n  TEST_FIXTURE_OK: \"1\"\nexecutable-overrides:\n{}",
            full_executable_entry("docker"),
        );
        let fx = parse_fixture(&yaml).unwrap();
        let exes = fx.executable_override.unwrap();
        let d = exes.get("docker").unwrap();
        assert_eq!(d.version.as_deref(), Some("1.0"));
    }

    #[test]
    fn malformed_fixture_missing_one_required_key() {
        // Omit `version` from the seven required keys.
        let yaml = "executable-overrides:\n  bad:\n    name: bad\n    found: true\n    \
                    executable: true\n    path: /x\n    command-full: x\n    version-full: x\n";
        let err = parse_fixture_collect_warnings(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("missing required keys"), "got {}", msg);
        assert!(msg.contains("version"), "got {}", msg);
        assert!(msg.contains("\"bad\""), "got {}", msg);
    }

    #[test]
    fn malformed_fixture_missing_each_required_key() {
        // Spec/0013 §A.6 — omitting any of the 7 keys produces an error
        // that names the missing key.
        for missing in EXECUTABLE_ENTRY_KEYS {
            let mut entry = String::from("executable-overrides:\n  bad:\n");
            for k in EXECUTABLE_ENTRY_KEYS {
                if k == missing {
                    continue;
                }
                let v = match *k {
                    "name" => "bad".to_string(),
                    "found" | "executable" => "true".to_string(),
                    _ => "\"x\"".to_string(),
                };
                entry.push_str(&format!("    {}: {}\n", k, v));
            }
            let err = parse_fixture_collect_warnings(&entry).unwrap_err();
            let msg = format!("{:#}", err);
            assert!(
                msg.contains(missing),
                "error for missing {:?} should name the key; got {}",
                missing,
                msg
            );
            assert!(
                msg.contains("\"bad\""),
                "error for missing {:?} should name the entry; got {}",
                missing,
                msg
            );
        }
    }

    #[test]
    fn malformed_fixture_unknown_top_level_key() {
        let yaml = "env-overrides:\n  FOO: bar\nbogus: 1\n";
        let err = parse_fixture_collect_warnings(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("bogus"), "got {}", msg);
        assert!(msg.contains("Valid keys"), "got {}", msg);
    }

    #[test]
    fn malformed_fixture_unknown_entry_key() {
        let yaml = "executable-overrides:\n  bad:\n    name: bad\n    found: true\n    \
                    executable: true\n    path: /x\n    command-full: x\n    version-full: x\n    \
                    version: \"1.0\"\n    versionfull: typo\n";
        let err = parse_fixture_collect_warnings(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("unknown key"), "got {}", msg);
        assert!(msg.contains("versionfull"), "got {}", msg);
        assert!(msg.contains("\"bad\""), "got {}", msg);
    }

    #[test]
    fn malformed_fixture_wrong_type() {
        let yaml = "executable-overrides:\n  bad:\n    name: bad\n    found: \"yes\"\n    \
                    executable: true\n    path: /x\n    command-full: x\n    version-full: x\n    \
                    version: \"1.0\"\n";
        let err = parse_fixture_collect_warnings(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("bool"), "got {}", msg);
    }

    #[test]
    fn malformed_fixture_name_mismatch() {
        let yaml = "executable-overrides:\n  docker:\n    name: not-docker\n    found: true\n    \
                    executable: true\n    path: /x\n    command-full: x\n    version-full: x\n    \
                    version: \"1.0\"\n";
        let err = parse_fixture_collect_warnings(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("mismatched"), "got {}", msg);
    }
}
