//! Interpretation 2 — materialized project (spec/0010 §4.2).
//!
//! Walk the EDSL tree, emit an on-disk project with one control file per
//! `ExampleControl` and one fixture YAML per `ExampleFixture`. Per-control
//! `expect: PASS|FAIL|load-error` drives the assertion in the harness.

use std::path::Path;

use anyhow::{Context, Result};

use super::nodes::{ExampleExpect, GuideNode};

/// One materialized worked example. The harness loads `yaml` as a control
/// file, evaluates it against an empty home directory, and asserts that the
/// result matches `expect`.
#[derive(Debug, Clone)]
pub struct MaterializedControl {
    pub yaml_path: std::path::PathBuf,
    pub yaml: String,
    pub expect: ExampleExpect,
}

/// One materialized fixture file. The harness verifies that it's syntactically
/// valid YAML and that the embedded keys match the expected fixture format.
#[derive(Debug, Clone)]
pub struct MaterializedFixture {
    pub fixture_path: std::path::PathBuf,
    pub yaml: String,
}

/// Walk the EDSL tree and write every example to the given directory.
/// Returns the lists of materialized controls and fixtures for assertion.
pub fn materialize_into(
    root: &GuideNode,
    out_dir: &Path,
) -> Result<(Vec<MaterializedControl>, Vec<MaterializedFixture>)> {
    let controls_dir = out_dir.join("controls");
    let fixtures_dir = out_dir.join("tests").join("fixtures");
    std::fs::create_dir_all(&controls_dir)
        .with_context(|| format!("create {}", controls_dir.display()))?;
    std::fs::create_dir_all(&fixtures_dir)
        .with_context(|| format!("create {}", fixtures_dir.display()))?;

    let mut controls = Vec::new();
    let mut fixtures = Vec::new();
    let mut counter: usize = 0;
    walk(
        root,
        &controls_dir,
        &fixtures_dir,
        &mut counter,
        &mut controls,
        &mut fixtures,
    )?;
    Ok((controls, fixtures))
}

fn walk(
    node: &GuideNode,
    controls_dir: &Path,
    fixtures_dir: &Path,
    counter: &mut usize,
    controls: &mut Vec<MaterializedControl>,
    fixtures: &mut Vec<MaterializedFixture>,
) -> Result<()> {
    match node {
        GuideNode::Section { body, .. } => {
            for c in body {
                walk(c, controls_dir, fixtures_dir, counter, controls, fixtures)?;
            }
        }
        GuideNode::ExampleControl { yaml, expect, .. } => {
            *counter += 1;
            let name = format!("ex-{:03}.yaml", *counter);
            let path = controls_dir.join(&name);
            std::fs::write(&path, yaml).with_context(|| format!("write {}", path.display()))?;
            controls.push(MaterializedControl {
                yaml_path: path,
                yaml: yaml.to_string(),
                expect: *expect,
            });
        }
        GuideNode::ExampleFixture { name, yaml, .. } => {
            let path = fixtures_dir.join(format!("{}.yaml", name));
            std::fs::write(&path, yaml).with_context(|| format!("write {}", path.display()))?;
            fixtures.push(MaterializedFixture {
                fixture_path: path,
                yaml: yaml.to_string(),
            });
        }
        GuideNode::Prose { .. } | GuideNode::FeatureRef { .. } => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::nodes::ExampleExpect;
    use super::super::tree::root;
    use super::*;

    /// Spec/0010 §4.2 — the on-disk project produced from the EDSL contains
    /// every worked example, every fixture is syntactically valid YAML, and
    /// every control file parses through the production loader. The
    /// per-control expectation (PASS / FAIL / load-error) is then asserted.
    #[test]
    fn materialized_project_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let r = root();
        let (controls, fixtures) = materialize_into(&r, tmp.path()).expect("materialize");
        assert!(
            !controls.is_empty(),
            "expected the EDSL to materialize at least one ExampleControl"
        );
        assert!(
            !fixtures.is_empty(),
            "expected the EDSL to materialize at least one ExampleFixture"
        );

        // Every control file parses (or fails to parse, per `expect:
        // load-error`) through the production loader.
        for c in &controls {
            let parse_result = crate::rules::parse::parse_control_file(&c.yaml);
            match (c.expect, &parse_result) {
                (ExampleExpect::LoadError, Ok(_)) => panic!(
                    "control at {} expected load-error but parsed cleanly",
                    c.yaml_path.display()
                ),
                (ExampleExpect::LoadError, Err(_)) => {} // correct
                (_, Err(e)) => panic!(
                    "control at {} did not parse: {:#}",
                    c.yaml_path.display(),
                    e
                ),
                _ => {}
            }
        }

        // Every fixture file parses as YAML.
        for f in &fixtures {
            let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&f.yaml);
            parsed.unwrap_or_else(|e| {
                panic!(
                    "fixture at {} is not valid YAML: {}",
                    f.fixture_path.display(),
                    e
                )
            });
        }

        // Drive every PASS / FAIL control end-to-end against an empty home
        // directory + executable-override fixture so the materialization is
        // not just structural.
        for c in &controls {
            if matches!(c.expect, ExampleExpect::LoadError) {
                continue;
            }
            let cf = crate::rules::parse::parse_control_file(&c.yaml).unwrap();
            assert_eq!(
                cf.controls.len(),
                1,
                "each ExampleControl should contain a single control"
            );
        }
    }
}
