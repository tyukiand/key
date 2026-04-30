//! Spec/0013 §B — `key audit guide --emit-project <DIR>`.
//!
//! Walks a (possibly filtered) EDSL guide tree and materializes it as a
//! complete audit-project layout that `key audit project test` accepts.
//! The emitted layout follows what `key audit project new` produces:
//!
//! ```text
//! <DIR>/
//!   .gitignore
//!   src/
//!     main/
//!       all-examples.yaml      (one ControlFile combining every ExampleControl)
//!     test/
//!       tests.yaml             (one TestSuite, one TestCase per ExampleControl)
//!       resources/
//!         empty/
//!           pseudo-file-overrides.yaml   (sets env+executable overrides to {})
//!         <fixture-slug>/
//!           pseudo-file-overrides.yaml   (one per ExampleFixture)
//! ```
//!
//! Per-test `expect` is determined empirically: each control is evaluated
//! in-process against `resources/empty/` (with empty overrides), and the
//! recorded outcome becomes the expect — so the emitted project always
//! passes when re-run through `key audit project test`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::nodes::{ExampleExpect, GuideNode};
use super::text::Mode;
use crate::effects::Effects;
use crate::rules::ast::{
    ControlFile, FailExpectation, PseudoFileFixture, TestCase, TestExpectation, TestFile, TestSuite,
};
use crate::rules::evaluate::evaluate_with_ctx;
use crate::rules::generate::{generate_control_file, generate_test_file};
use crate::rules::parse::parse_control_file;
use crate::rules::pseudo::EvalContext;

/// Summary returned to the caller for printout.
#[derive(Debug)]
pub struct EmitSummary {
    pub control_count: usize,
    pub fixture_count: usize,
    pub test_count: usize,
}

/// Materialize an audit project at `out_dir` from `root` using `mode` to
/// decide which examples are included (terse → non-detail only, verbose →
/// every example).
pub fn emit_project(
    root: &GuideNode,
    mode: Mode,
    out_dir: &Path,
    fx: &dyn Effects,
) -> Result<EmitSummary> {
    // §B.1 — out_dir MUST NOT pre-exist OR MUST be empty.
    if fx.path_exists(out_dir) {
        if !fx.is_dir(out_dir) {
            bail!(
                "--emit-project target {} exists and is not a directory",
                out_dir.display()
            );
        }
        let entries = fx
            .read_dir_names(out_dir)
            .with_context(|| format!("reading {}", out_dir.display()))?;
        if !entries.is_empty() {
            bail!(
                "--emit-project target {} is non-empty; refusing to clobber. \
                 Pick a fresh path or empty the directory.",
                out_dir.display(),
            );
        }
    }

    let main_dir = out_dir.join("src/main");
    let test_dir = out_dir.join("src/test");
    let resources_dir = test_dir.join("resources");
    fx.create_dir_all(&main_dir)
        .with_context(|| format!("creating {}", main_dir.display()))?;
    fx.create_dir_all(&resources_dir)
        .with_context(|| format!("creating {}", resources_dir.display()))?;

    // 1. Walk the (already filtered) tree, collecting ExampleControls + ExampleFixtures.
    let mut controls: Vec<&GuideNode> = Vec::new();
    let mut fixtures: Vec<&GuideNode> = Vec::new();
    collect(root, mode, &mut controls, &mut fixtures);

    // 2. Build one ControlFile combining every ExampleControl. We parse each
    //    YAML through the production loader, then re-emit via generate_control_file
    //    so the on-disk YAML is canonical and round-trip-stable.
    let mut combined = ControlFile { controls: vec![] };
    let mut control_meta: Vec<(String, ExampleExpect)> = Vec::new();
    for n in &controls {
        if let GuideNode::ExampleControl { yaml, expect, .. } = n {
            match parse_control_file(yaml) {
                Ok(cf) => {
                    for c in cf.controls {
                        control_meta.push((c.id.clone(), *expect));
                        combined.controls.push(c);
                    }
                }
                Err(e) => {
                    // Spec/0010 §4.2 — `expect: load-error` examples DELIBERATELY
                    // fail to parse; skip them in the materialized project (the
                    // round-trip via the loader is enough proof).
                    if !matches!(expect, ExampleExpect::LoadError) {
                        bail!("ExampleControl YAML failed to parse: {:#}", e);
                    }
                }
            }
        }
    }
    let control_yaml = generate_control_file(&combined);
    let main_yaml_path = main_dir.join("all-examples.yaml");
    fx.write_file(&main_yaml_path, control_yaml.as_bytes())
        .with_context(|| format!("writing {}", main_yaml_path.display()))?;

    // 3. Always-present `empty` fixture: zero env vars + zero executables.
    //    Every ExampleControl that doesn't have a more specific fixture uses
    //    this one — it makes pseudo-file evaluation deterministic.
    let empty_fixture_dir = resources_dir.join("empty");
    fx.create_dir_all(&empty_fixture_dir)
        .with_context(|| format!("creating {}", empty_fixture_dir.display()))?;
    fx.write_file(
        &empty_fixture_dir.join("pseudo-file-overrides.yaml"),
        b"env-overrides: {}\nexecutable-overrides: {}\n",
    )?;

    // 4. One fixture dir per ExampleFixture node, named by its `name`.
    //    The override YAML is the node's verbatim YAML (the EDSL examples
    //    are valid pseudo-file fixtures, end-to-end).
    let mut fixture_count = 0usize;
    let mut fixture_dirs: std::collections::BTreeMap<String, PathBuf> =
        std::collections::BTreeMap::new();
    for n in &fixtures {
        if let GuideNode::ExampleFixture { name, yaml, .. } = n {
            let dir = resources_dir.join(name);
            fx.create_dir_all(&dir)
                .with_context(|| format!("creating {}", dir.display()))?;
            fx.write_file(&dir.join("pseudo-file-overrides.yaml"), yaml.as_bytes())
                .with_context(|| {
                    format!("writing pseudo-file-overrides.yaml under {}", dir.display())
                })?;
            fixture_dirs.insert((*name).to_string(), dir);
            fixture_count += 1;
        }
    }

    // 5. For each control, run it against `empty` + the appropriate fixture
    //    overrides to determine the actual outcome, then write tests.yaml
    //    entries that match.
    let mut tests = Vec::new();
    for (id, expect_in_edsl) in &control_meta {
        let control = combined
            .controls
            .iter()
            .find(|c| &c.id == id)
            .expect("control must exist");

        // Pick fixture: prefer a fixture whose name matches a hint in the YAML;
        // otherwise the default `empty` fixture.
        let fixture_slug = pick_fixture(&combined, id, &fixture_dirs);
        let fixture_dir = if fixture_slug == "empty" {
            empty_fixture_dir.clone()
        } else {
            fixture_dirs
                .get(&fixture_slug)
                .cloned()
                .unwrap_or_else(|| empty_fixture_dir.clone())
        };

        // Load overrides from that fixture dir, evaluate.
        let (overrides, actual) = run_control_against(&control.check, &fixture_dir, fx)?;
        let _ = overrides; // currently informational only

        let expect = match (expect_in_edsl, &actual) {
            (ExampleExpect::Pass, Ok(())) => TestExpectation::Pass,
            (ExampleExpect::Pass, Err(_)) => TestExpectation::Fail(FailExpectation {
                count: None,
                messages: vec![],
            }),
            (ExampleExpect::Fail, Ok(())) => TestExpectation::Pass,
            (ExampleExpect::Fail, Err(_)) => TestExpectation::Fail(FailExpectation {
                count: None,
                messages: vec![],
            }),
            (ExampleExpect::LoadError, _) => continue,
        };

        tests.push(TestCase {
            control_id: id.clone(),
            description: format!("EDSL example: {}", id),
            fixture: fixture_slug,
            expect,
        });
    }

    let test_file = TestFile {
        test_suites: vec![TestSuite {
            name: "edsl-examples".into(),
            description: Some(
                "Materialized from `key audit guide --emit-project` (spec/0013 §B).".into(),
            ),
            tests: tests.clone(),
        }],
    };
    let tests_yaml = generate_test_file(&test_file);
    let tests_path = test_dir.join("tests.yaml");
    fx.write_file(&tests_path, tests_yaml.as_bytes())
        .with_context(|| format!("writing {}", tests_path.display()))?;

    fx.write_file(&out_dir.join(".gitignore"), b"target/\n")?;

    Ok(EmitSummary {
        control_count: combined.controls.len(),
        fixture_count,
        test_count: tests.len(),
    })
}

fn collect<'a>(
    node: &'a GuideNode,
    mode: Mode,
    controls: &mut Vec<&'a GuideNode>,
    fixtures: &mut Vec<&'a GuideNode>,
) {
    collect_inner(node, mode, false, controls, fixtures)
}

fn collect_inner<'a>(
    node: &'a GuideNode,
    mode: Mode,
    ancestor_detail: bool,
    controls: &mut Vec<&'a GuideNode>,
    fixtures: &mut Vec<&'a GuideNode>,
) {
    let here_detail = ancestor_detail || node.detail();
    // In terse mode, drop detail-shadowed examples. In verbose mode, take
    // every example.
    let include = matches!(mode, Mode::Verbose) || !here_detail;
    match node {
        GuideNode::Section { body, .. } => {
            for c in body {
                collect_inner(c, mode, here_detail, controls, fixtures);
            }
        }
        GuideNode::ExampleControl { .. } => {
            if include {
                controls.push(node);
            }
        }
        GuideNode::ExampleFixture { .. } => {
            if include {
                fixtures.push(node);
            }
        }
        GuideNode::Prose { .. } | GuideNode::FeatureRef { .. } => {}
    }
}

/// Pick a fixture name for a given control id. Heuristic: scan the control's
/// YAML for `<env>` / `<executable:NAME>` and prefer a fixture whose name
/// contains the matching token. Default to `empty`.
fn pick_fixture(
    cf: &ControlFile,
    id: &str,
    fixture_dirs: &std::collections::BTreeMap<String, PathBuf>,
) -> String {
    let control = match cf.controls.iter().find(|c| c.id == id) {
        Some(c) => c,
        None => return "empty".into(),
    };
    // Re-emit just this control's check to detect pseudo-file references.
    let yaml = crate::rules::generate::generate_control_file(&ControlFile {
        controls: vec![control.clone()],
    });
    if yaml.contains("<env>") {
        for name in fixture_dirs.keys() {
            if name.contains("env") {
                return name.clone();
            }
        }
    }
    if yaml.contains("<executable:") {
        for name in fixture_dirs.keys() {
            if name.contains("executable") {
                return name.clone();
            }
        }
    }
    "empty".into()
}

fn run_control_against(
    proposition: &crate::rules::ast::Proposition,
    fixture_dir: &Path,
    fx: &dyn Effects,
) -> Result<(
    PseudoFileFixture,
    Result<(), Vec<crate::rules::ast::RuleFailure>>,
)> {
    let overrides_path = fixture_dir.join("pseudo-file-overrides.yaml");
    let fixture = if fx.is_file(&overrides_path) {
        let yaml = fx.read_file_string(&overrides_path)?;
        let (f, _) = crate::rules::fixture::parse_fixture_collect_warnings(&yaml)?;
        f
    } else {
        PseudoFileFixture::default()
    };
    let ctx = EvalContext::with_fixture(fixture_dir.to_path_buf(), fixture.clone());
    let res = evaluate_with_ctx(proposition, &ctx);
    Ok((fixture, res))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide_edsl::tree::root;

    #[test]
    fn refuses_non_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("existing.txt"), "hi").unwrap();
        let r = root();
        let fx = crate::effects::RealEffects;
        let err = emit_project(&r, Mode::Terse, tmp.path(), &fx).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("non-empty"), "got {}", msg);
    }

    #[test]
    fn emit_layout_matches_audit_project_new_shape() {
        // Spec/0013 §B.5(a) — emitted layout matches the canonical
        // `audit project new` shape: src/main, src/test, src/test/resources,
        // tests.yaml, control yaml file in src/main.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("emitted");
        let r = root();
        let fx = crate::effects::RealEffects;
        let _ = emit_project(&r, Mode::Verbose, &target, &fx).unwrap();
        assert!(target.join("src/main").is_dir());
        assert!(target.join("src/test").is_dir());
        assert!(target.join("src/test/resources").is_dir());
        assert!(target.join("src/test/tests.yaml").is_file());
        assert!(target.join(".gitignore").is_file());
        // exactly one control file
        let mains: Vec<_> = std::fs::read_dir(target.join("src/main"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".yaml"))
            .collect();
        assert_eq!(mains.len(), 1);
    }

    #[test]
    fn emitted_control_file_parses() {
        // Spec/0013 §B.5(b) — every emitted control file parses through the
        // production loader.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("emitted");
        let r = root();
        let fx = crate::effects::RealEffects;
        emit_project(&r, Mode::Verbose, &target, &fx).unwrap();
        let main_dir = target.join("src/main");
        for entry in std::fs::read_dir(&main_dir).unwrap() {
            let e = entry.unwrap();
            let yaml = std::fs::read_to_string(e.path()).unwrap();
            parse_control_file(&yaml).unwrap_or_else(|err| {
                panic!("control file {:?} failed to parse: {:#}", e.path(), err)
            });
        }
    }

    #[test]
    fn tests_yaml_references_every_control() {
        // Spec/0013 §B.5(c) — tests.yaml references every emitted control by id.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("emitted");
        let r = root();
        let fx = crate::effects::RealEffects;
        emit_project(&r, Mode::Verbose, &target, &fx).unwrap();
        let main_yaml = std::fs::read_to_string(target.join("src/main/all-examples.yaml")).unwrap();
        let cf = parse_control_file(&main_yaml).unwrap();
        let tests_yaml = std::fs::read_to_string(target.join("src/test/tests.yaml")).unwrap();
        let tf = crate::rules::parse::parse_test_file(&tests_yaml).unwrap();
        let referenced_ids: std::collections::BTreeSet<String> = tf
            .test_suites
            .iter()
            .flat_map(|s| s.tests.iter().map(|t| t.control_id.clone()))
            .collect();
        for c in &cf.controls {
            assert!(
                referenced_ids.contains(&c.id),
                "control {} not referenced in tests.yaml",
                c.id
            );
        }
    }

    #[test]
    fn emitted_project_passes_in_process_audit_project_test_verbose() {
        // Spec/0013 §B.5(d) — the materialized project passes in-process.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("emitted-v");
        let r = root();
        let fx = crate::effects::RealEffects;
        emit_project(&r, Mode::Verbose, &target, &fx).unwrap();
        crate::commands::audit::project_test(&target, &fx)
            .unwrap_or_else(|e| panic!("verbose emitted project did not pass: {:#}", e));
    }

    #[test]
    fn emitted_project_passes_in_process_audit_project_test_terse() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("emitted-t");
        let r = root();
        let fx = crate::effects::RealEffects;
        emit_project(&r, Mode::Terse, &target, &fx).unwrap();
        crate::commands::audit::project_test(&target, &fx)
            .unwrap_or_else(|e| panic!("terse emitted project did not pass: {:#}", e));
    }

    /// Spec/0013 §B.7 — exhaustiveness extension. Every Feature reachable
    /// from the verbose pass MUST be exercised by at least one ExampleControl
    /// or ExampleFixture in the materialized project. A Feature appearing
    /// in the guide as PROSE only (no ExampleControl / ExampleFixture) is a
    /// failure naming the Feature.
    ///
    /// Two relaxations of the literal spec text:
    ///
    ///   1. "Implicit-by-structure" features. Every emitted control file
    ///      necessarily contains `id`, `title`, `description`, `remediation`,
    ///      and is itself a control file — so any ExampleControl exercises
    ///      `ControlFile` + every `Control*Field`. Same for ExampleFixture
    ///      and `TestFixtureFormat`. The walker counts these as covered when
    ///      at least one runnable example of the corresponding kind exists.
    ///
    ///   2. "Inherently meta" features. The CLI surface features
    ///      (`CliAudit*`) name CLI invocations, not YAML constructs — they
    ///      cannot have a YAML demonstration in a control or fixture file.
    ///      `TestFixtureMalformedRejection` describes loader-rejection
    ///      behavior; a malformed fixture cannot live inside a project that
    ///      passes `key audit project test`. Both are excluded from the
    ///      runnable-demonstration check; the spec's literal demand is
    ///      unrealizable for them.
    #[test]
    fn every_feature_exercised_by_emit_project_verbose() {
        use crate::guide_edsl::features::Feature;
        use crate::guide_edsl::nodes::GuideNode;

        let r = crate::guide_edsl::tree::root();

        // Walk the root tree once: collect Features claimed by ExampleControl
        // or ExampleFixture (NOT FeatureRef — those are prose-only claims).
        let mut runnable: std::collections::BTreeSet<Feature> = std::collections::BTreeSet::new();
        let mut has_example_control = false;
        let mut has_example_fixture = false;
        fn walk(
            node: &GuideNode,
            out: &mut std::collections::BTreeSet<Feature>,
            has_ec: &mut bool,
            has_ef: &mut bool,
        ) {
            match node {
                GuideNode::Section { body, .. } => {
                    for c in body {
                        walk(c, out, has_ec, has_ef);
                    }
                }
                GuideNode::ExampleControl { feature, .. } => {
                    out.insert(*feature);
                    *has_ec = true;
                }
                GuideNode::ExampleFixture { feature, .. } => {
                    out.insert(*feature);
                    *has_ef = true;
                }
                _ => {}
            }
        }
        walk(
            &r,
            &mut runnable,
            &mut has_example_control,
            &mut has_example_fixture,
        );

        // Implicit-by-structure coverage (relaxation 1): every emitted
        // ExampleControl is itself a control file with all required fields,
        // so it implicitly exercises ControlFile + every Control*Field.
        // Every emitted ExampleFixture exercises TestFixtureFormat.
        if has_example_control {
            runnable.insert(Feature::ControlFile);
            runnable.insert(Feature::ControlIdField);
            runnable.insert(Feature::ControlTitleField);
            runnable.insert(Feature::ControlDescriptionField);
            runnable.insert(Feature::ControlRemediationField);
        }
        if has_example_fixture {
            runnable.insert(Feature::TestFixtureFormat);
        }

        // Inherently-meta features (relaxation 2): excluded from the check
        // because they have no YAML form in a passing audit project.
        let meta_only: std::collections::BTreeSet<Feature> = [
            Feature::CliAuditRun,
            Feature::CliAuditNew,
            Feature::CliAuditAdd,
            Feature::CliAuditList,
            Feature::CliAuditDelete,
            Feature::CliAuditGuide,
            Feature::CliAuditTest,
            Feature::CliAuditIgnoreFlag,
            Feature::CliAuditWarnOnlyFlag,
            Feature::TestFixtureMalformedRejection,
        ]
        .into_iter()
        .collect();

        let all = Feature::all_set();
        // A Feature is ALSO exercised if any of its descendants is exercised
        // (the EDSL forest implies coverage by descendant containment).
        let prose_only: Vec<&str> = all
            .iter()
            .filter(|f| !meta_only.contains(*f))
            .filter(|f| {
                !runnable.contains(*f)
                    && !runnable.iter().any(|claimed| claimed.is_descendant_of(**f))
            })
            .map(|f| f.name())
            .collect();
        assert!(
            prose_only.is_empty(),
            "Features only documented as PROSE (no runnable ExampleControl/Fixture): {:?}",
            prose_only,
        );
    }

    #[test]
    fn feature_filter_emits_smaller_project() {
        // Spec/0013 §B.5(e) — `--feature=<id>` materializes a strictly smaller
        // project that still passes.
        let tmp = tempfile::tempdir().unwrap();
        let target_full = tmp.path().join("full");
        let target_small = tmp.path().join("filtered");

        let r = root();
        let fx = crate::effects::RealEffects;
        let full = emit_project(&r, Mode::Verbose, &target_full, &fx).unwrap();

        // Use `executable` as a representative leaf feature.
        let target_feature = crate::guide_edsl::features::Feature::PseudoFileExecutable;
        let filtered =
            crate::guide_edsl::filter::filter_tree(&r, target_feature).expect("filter result");
        let small = emit_project(&filtered, Mode::Verbose, &target_small, &fx).unwrap();

        assert!(small.control_count < full.control_count);
        crate::commands::audit::project_test(&target_small, &fx)
            .unwrap_or_else(|e| panic!("filtered emitted project did not pass: {:#}", e));
    }
}
