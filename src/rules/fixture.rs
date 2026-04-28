//! YAML fixture loader for pseudo-file overrides (spec/0009 §2.5, §3.8, §3.8.1).
//!
//! Only invoked from test code (unit / integration / round-trip guide tests).
//! Public so external integration tests can use it via the library crate;
//! suppress the bin's dead-code lint here.
//!
//! Fixture shape:
//!
//! ```yaml
//! env:
//!   <KEY>: <VALUE>
//!   ...
//! executables:
//!   <NAME>:
//!     found: bool
//!     executable: bool
//!     path: string|null
//!     command-full: string|null
//!     version-full: string|null
//!     version: string|null
//! ```
//!
//! Each field is parsed individually; malformed fixtures fail with a
//! line-numbered error (per §6.7 / §3.8.1 negative meta-test).

#![allow(dead_code)]

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use serde_yaml::Value;

use crate::rules::ast::{ExecutableSnapshot, PseudoFileFixture};

/// Parse a YAML fixture string into a `PseudoFileFixture`.
pub fn parse_fixture(yaml: &str) -> Result<PseudoFileFixture> {
    let value: Value = serde_yaml::from_str(yaml).context("invalid YAML in pseudo-file fixture")?;
    parse_fixture_value(&value)
}

fn parse_fixture_value(value: &Value) -> Result<PseudoFileFixture> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("pseudo-file fixture must be a YAML mapping at the top level"))?;

    let mut fixture = PseudoFileFixture::default();

    for (k, v) in m.iter() {
        let key = k.as_str().ok_or_else(|| {
            anyhow!(
                "pseudo-file fixture top-level key must be a string, got {:?}",
                k
            )
        })?;
        match key {
            "env" => {
                fixture.env_override = Some(parse_env_block(v)?);
            }
            "executables" => {
                fixture.executable_override = Some(parse_executables_block(v)?);
            }
            other => bail!(
                "unknown top-level key in pseudo-file fixture: {:?}. \
                 Valid keys: `env`, `executables`.",
                other
            ),
        }
    }

    Ok(fixture)
}

fn parse_env_block(value: &Value) -> Result<BTreeMap<String, String>> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("`env:` must be a mapping of NAME: VALUE entries"))?;
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

fn parse_executables_block(value: &Value) -> Result<BTreeMap<String, ExecutableSnapshot>> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("`executables:` must be a mapping of NAME: snapshot entries"))?;
    let mut out = BTreeMap::new();
    for (k, v) in m.iter() {
        let name = k
            .as_str()
            .ok_or_else(|| anyhow!("executable key must be a string, got {:?}", k))?;
        let snap = parse_executable_snapshot(name, v)
            .with_context(|| format!("parsing executable {:?}", name))?;
        out.insert(name.to_string(), snap);
    }
    Ok(out)
}

fn parse_executable_snapshot(name: &str, value: &Value) -> Result<ExecutableSnapshot> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("snapshot for {:?} must be a mapping", name))?;

    let found = require_bool(m, "found")?;
    let executable = require_bool(m, "executable")?;
    let path = optional_string(m, "path")?;
    let command_full = optional_string(m, "command-full")?;
    let version_full = optional_string(m, "version-full")?;
    let version = optional_string(m, "version")?;

    // Sanity-check unknown keys to avoid silent typos (e.g. `versionfull:`).
    for (k, _) in m.iter() {
        let key = k.as_str().unwrap_or("");
        if !matches!(
            key,
            "found" | "executable" | "path" | "command-full" | "version-full" | "version"
        ) {
            bail!(
                "unknown key {:?} in executable snapshot for {:?}. \
                 Valid keys: found, executable, path, command-full, version-full, version.",
                key,
                name
            );
        }
    }

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

fn require_bool(m: &serde_yaml::Mapping, key: &str) -> Result<bool> {
    let v = m
        .get(Value::String(key.to_string()))
        .ok_or_else(|| anyhow!("missing required field {:?}", key))?;
    v.as_bool()
        .ok_or_else(|| anyhow!("field {:?} must be a bool, got {:?}", key, v))
}

fn optional_string(m: &serde_yaml::Mapping, key: &str) -> Result<Option<String>> {
    let v = match m.get(Value::String(key.to_string())) {
        None => return Ok(None),
        Some(v) => v,
    };
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        other => bail!("field {:?} must be a string or null, got {:?}", key, other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_only_fixture() {
        let yaml = r#"
env:
  RUSTUP_HOME: /home/u/.rustup
  PATH: /usr/bin:/home/u/.cargo/bin
  FOO: "bar baz"
"#;
        let fx = parse_fixture(yaml).unwrap();
        let env = fx.env_override.unwrap();
        assert_eq!(env.get("RUSTUP_HOME").unwrap(), "/home/u/.rustup");
        assert_eq!(env.get("PATH").unwrap(), "/usr/bin:/home/u/.cargo/bin");
        assert_eq!(env.get("FOO").unwrap(), "bar baz");
        assert!(fx.executable_override.is_none());
    }

    #[test]
    fn parse_full_fixture() {
        let yaml = r#"
env:
  TEST_FIXTURE_OK: "1"
executables:
  docker:
    found: true
    executable: true
    path: /usr/bin/docker
    command-full: docker --version
    version-full: |
      Docker version 20.10.7, build f0df350
    version: 20.10.7
  nonexistent-tool:
    found: false
    executable: false
    path: null
    command-full: null
    version-full: null
    version: null
"#;
        let fx = parse_fixture(yaml).unwrap();
        let exes = fx.executable_override.unwrap();
        let d = exes.get("docker").unwrap();
        assert_eq!(d.version.as_deref(), Some("20.10.7"));
        assert_eq!(d.path.as_deref(), Some("/usr/bin/docker"));
        let n = exes.get("nonexistent-tool").unwrap();
        assert!(!n.found);
        assert!(n.path.is_none());
    }

    #[test]
    fn malformed_fixture_missing_required_field() {
        let yaml = r#"
executables:
  bad:
    executable: true
    path: /x
"#;
        // Missing `found:` field
        let err = parse_fixture(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("found"), "unexpected error: {}", msg);
    }

    #[test]
    fn malformed_fixture_wrong_type() {
        let yaml = r#"
executables:
  bad:
    found: "yes"
    executable: true
"#;
        let err = parse_fixture(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("bool"), "unexpected error: {}", msg);
    }

    #[test]
    fn malformed_fixture_unknown_key() {
        let yaml = r#"
executables:
  bad:
    found: true
    executable: true
    versionfull: typo
"#;
        let err = parse_fixture(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("unknown key"), "unexpected error: {}", msg);
    }
}
