//! In-memory Project ADT — see `spec/0015-interaction-coroutine-and-project-adt.txt` §1.
//!
//! Holds controls / fixtures / tests / meta as pure-functional data. The only
//! filesystem touchpoints are `load_from_dir` and `write_to_dir`. Mutators
//! return new Projects (no in-place mutation); errors are explicit.

// The bin (`key`) currently only consumes a subset of the Project surface
// (load_from_dir + all_controls); the mutation surface is exercised by the
// library + integration tests (round-trip / lower invariants per spec §5).
// Suppress unused-warnings outside of test builds rather than introducing
// fake call sites in the bin.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::rules::ast::{
    Control, ControlFile, PseudoFileFixture, RuleFailure, TestCase, TestFile, TestSuite,
};
use crate::rules::evaluate::{evaluate, evaluate_with_ctx};
use crate::rules::fixture::parse_fixture_collect_warnings;
use crate::rules::generate::{generate_control_file, generate_test_file};
use crate::rules::parse::{parse_control_file, parse_test_file};
use crate::rules::pseudo::EvalContext;

// ---------------------------------------------------------------------------
// Branded names (spec §1.1) — newtypes ensuring stable lexical/file identity.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectNameError {
    /// Empty.
    Empty,
    /// Contains characters outside [A-Za-z0-9_.-].
    InvalidChars(String),
    /// Reserved (e.g. "."/"..").
    Reserved(String),
}

impl fmt::Display for ProjectNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectNameError::Empty => write!(f, "name must not be empty"),
            ProjectNameError::InvalidChars(s) => {
                write!(f, "name contains invalid characters: {:?}", s)
            }
            ProjectNameError::Reserved(s) => write!(f, "name is reserved: {:?}", s),
        }
    }
}

fn validate_branded_name(s: &str) -> Result<(), ProjectNameError> {
    if s.is_empty() {
        return Err(ProjectNameError::Empty);
    }
    if s == "." || s == ".." {
        return Err(ProjectNameError::Reserved(s.to_string()));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(ProjectNameError::InvalidChars(s.to_string()));
    }
    Ok(())
}

/// Brand-equality key used by the BTreeMap. Stored as case-folded so two
/// names that differ only in case are detected as collisions at insert time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BrandKey(String);

/// Logical control-file name (without `.yaml` suffix).
///
/// Branded for case-insensitive collision detection (spec §1.1, §4.2).
#[derive(Debug, Clone, Eq)]
pub struct ControlFileName(String);

impl ControlFileName {
    pub fn new(s: &str) -> Result<Self, ProjectNameError> {
        validate_branded_name(s)?;
        Ok(ControlFileName(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn key(&self) -> BrandKey {
        BrandKey(self.0.to_ascii_lowercase())
    }
}

impl PartialEq for ControlFileName {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl PartialOrd for ControlFileName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ControlFileName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

impl fmt::Display for ControlFileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Logical fixture-directory name.
#[derive(Debug, Clone, Eq)]
pub struct FixtureFileName(String);

impl FixtureFileName {
    pub fn new(s: &str) -> Result<Self, ProjectNameError> {
        validate_branded_name(s)?;
        Ok(FixtureFileName(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn key(&self) -> BrandKey {
        BrandKey(self.0.to_ascii_lowercase())
    }
}

impl PartialEq for FixtureFileName {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl PartialOrd for FixtureFileName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FixtureFileName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

impl fmt::Display for FixtureFileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// FixtureFile — in-memory representation of one fixture directory
// ---------------------------------------------------------------------------

/// A fixture directory's content, fully in-memory.
///
/// `pseudo_overrides` mirrors the optional `pseudo-file-overrides.yaml`.
/// `files` is a map of relative path → file body (anything else under the
/// fixture dir, used as the predicate subject when evaluating the control).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixtureFile {
    pub pseudo_overrides: Option<PseudoFileFixture>,
    pub files: BTreeMap<String, Vec<u8>>,
}

// ---------------------------------------------------------------------------
// TestsYaml — thin wrapper around TestFile for round-trip stability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TestsYaml {
    pub inner: TestFile,
}

impl TestsYaml {
    pub fn empty() -> Self {
        TestsYaml {
            inner: TestFile {
                test_suites: vec![],
            },
        }
    }
}

impl Default for TestsYaml {
    fn default() -> Self {
        TestsYaml::empty()
    }
}

// ---------------------------------------------------------------------------
// ProjectMeta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectMeta {
    /// Optional project name (None for anonymous in-memory projects).
    pub name: Option<String>,
}

// ---------------------------------------------------------------------------
// Project — top-level ADT (spec §1.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    pub controls: BTreeMap<ControlFileName, ControlFile>,
    pub fixtures: BTreeMap<FixtureFileName, FixtureFile>,
    pub tests: TestsYaml,
    pub meta: ProjectMeta,
}

// ---------------------------------------------------------------------------
// Mutation errors (spec §1.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMutationError {
    DuplicateControl(String),
    DuplicateFixture(String),
    DuplicateTestEntry { suite: String, control_id: String },
    NotFoundControl(String),
    NotFoundFixture(String),
    NotFoundTestEntry { suite: String, control_id: String },
    DanglingControl(String),
    DanglingFixture(String),
    InvalidName(ProjectNameError),
}

impl fmt::Display for ProjectMutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectMutationError::DuplicateControl(s) => {
                write!(f, "control file already exists: {:?}", s)
            }
            ProjectMutationError::DuplicateFixture(s) => {
                write!(f, "fixture already exists: {:?}", s)
            }
            ProjectMutationError::DuplicateTestEntry { suite, control_id } => write!(
                f,
                "test entry already exists in suite {:?} for control {:?}",
                suite, control_id
            ),
            ProjectMutationError::NotFoundControl(s) => {
                write!(f, "control file not found: {:?}", s)
            }
            ProjectMutationError::NotFoundFixture(s) => write!(f, "fixture not found: {:?}", s),
            ProjectMutationError::NotFoundTestEntry { suite, control_id } => write!(
                f,
                "test entry not found in suite {:?} for control {:?}",
                suite, control_id
            ),
            ProjectMutationError::DanglingControl(s) => write!(
                f,
                "test entry references control {:?} that is not in the project",
                s
            ),
            ProjectMutationError::DanglingFixture(s) => write!(
                f,
                "test entry references fixture {:?} that is not in the project",
                s
            ),
            ProjectMutationError::InvalidName(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ProjectMutationError {}

// ---------------------------------------------------------------------------
// Constructors and pure-functional mutators (spec §1.2, §1.5)
// ---------------------------------------------------------------------------

impl Project {
    pub fn empty() -> Project {
        Project::default()
    }

    /// Add a control file. Errors on duplicate name (case-insensitive).
    pub fn with_control_added(
        mut self,
        name: ControlFileName,
        cf: ControlFile,
    ) -> Result<Project, ProjectMutationError> {
        if self.controls.contains_key(&name) {
            return Err(ProjectMutationError::DuplicateControl(
                name.as_str().to_string(),
            ));
        }
        self.controls.insert(name, cf);
        Ok(self)
    }

    pub fn with_control_deleted(
        mut self,
        name: &ControlFileName,
    ) -> Result<Project, ProjectMutationError> {
        if self.controls.remove(name).is_none() {
            return Err(ProjectMutationError::NotFoundControl(
                name.as_str().to_string(),
            ));
        }
        Ok(self)
    }

    pub fn with_fixture_added(
        mut self,
        name: FixtureFileName,
        fx: FixtureFile,
    ) -> Result<Project, ProjectMutationError> {
        if self.fixtures.contains_key(&name) {
            return Err(ProjectMutationError::DuplicateFixture(
                name.as_str().to_string(),
            ));
        }
        self.fixtures.insert(name, fx);
        Ok(self)
    }

    pub fn with_fixture_deleted(
        mut self,
        name: &FixtureFileName,
    ) -> Result<Project, ProjectMutationError> {
        if self.fixtures.remove(name).is_none() {
            return Err(ProjectMutationError::NotFoundFixture(
                name.as_str().to_string(),
            ));
        }
        Ok(self)
    }

    /// Add a test entry to the named suite (creating the suite if absent).
    /// Errors if the entry duplicates an existing (suite, control_id) pair.
    pub fn with_test_entry_added(
        mut self,
        suite_name: &str,
        tc: TestCase,
    ) -> Result<Project, ProjectMutationError> {
        let suite_idx = self
            .tests
            .inner
            .test_suites
            .iter()
            .position(|s| s.name == suite_name);
        let idx = match suite_idx {
            Some(i) => i,
            None => {
                self.tests.inner.test_suites.push(TestSuite {
                    name: suite_name.to_string(),
                    description: None,
                    tests: vec![],
                });
                self.tests.inner.test_suites.len() - 1
            }
        };
        if self.tests.inner.test_suites[idx]
            .tests
            .iter()
            .any(|t| t.control_id == tc.control_id && t.fixture == tc.fixture)
        {
            return Err(ProjectMutationError::DuplicateTestEntry {
                suite: suite_name.to_string(),
                control_id: tc.control_id.clone(),
            });
        }
        self.tests.inner.test_suites[idx].tests.push(tc);
        Ok(self)
    }

    pub fn with_test_entry_deleted(
        mut self,
        suite_name: &str,
        control_id: &str,
        fixture: &str,
    ) -> Result<Project, ProjectMutationError> {
        let suite_idx = self
            .tests
            .inner
            .test_suites
            .iter()
            .position(|s| s.name == suite_name)
            .ok_or_else(|| ProjectMutationError::NotFoundTestEntry {
                suite: suite_name.to_string(),
                control_id: control_id.to_string(),
            })?;
        let entry_idx = self.tests.inner.test_suites[suite_idx]
            .tests
            .iter()
            .position(|t| t.control_id == control_id && t.fixture == fixture)
            .ok_or_else(|| ProjectMutationError::NotFoundTestEntry {
                suite: suite_name.to_string(),
                control_id: control_id.to_string(),
            })?;
        self.tests.inner.test_suites[suite_idx]
            .tests
            .remove(entry_idx);
        // Drop the suite if it became empty (keeps round-trip stable).
        if self.tests.inner.test_suites[suite_idx].tests.is_empty() {
            self.tests.inner.test_suites.remove(suite_idx);
        }
        Ok(self)
    }

    /// Aggregate: every Control across every ControlFile in the project.
    pub fn all_controls(&self) -> Vec<&Control> {
        self.controls
            .values()
            .flat_map(|cf| cf.controls.iter())
            .collect()
    }

    /// Returns the (file_name, control_id) → owning control-file-name map.
    pub fn find_control_file_for_id(&self, id: &str) -> Option<&ControlFileName> {
        for (name, cf) in &self.controls {
            if cf.controls.iter().any(|c| c.id == id) {
                return Some(name);
            }
        }
        None
    }

    /// Reject test entries whose control_id is not present in any control
    /// file, or whose fixture is not present. Used at write-time validation.
    pub fn validate_references(&self) -> Result<(), ProjectMutationError> {
        for suite in &self.tests.inner.test_suites {
            for tc in &suite.tests {
                if self.find_control_file_for_id(&tc.control_id).is_none() {
                    return Err(ProjectMutationError::DanglingControl(tc.control_id.clone()));
                }
                let fx_name = match FixtureFileName::new(&tc.fixture) {
                    Ok(n) => n,
                    Err(e) => return Err(ProjectMutationError::InvalidName(e)),
                };
                if !self.fixtures.contains_key(&fx_name) {
                    return Err(ProjectMutationError::DanglingFixture(tc.fixture.clone()));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Filesystem touchpoints (spec §1.3) — the ONLY place Project hits disk.
// ---------------------------------------------------------------------------

impl Project {
    /// Load a Project from an audit-project directory laid out under
    /// `<dir>/src/{main,test/resources}` plus `<dir>/src/test/tests.yaml`.
    pub fn load_from_dir(dir: &Path) -> Result<Project> {
        let main_dir = dir.join("src/main");
        if !main_dir.is_dir() {
            bail!(
                "not an audit project (missing src/main/): {}",
                dir.display()
            );
        }

        let mut controls: BTreeMap<ControlFileName, ControlFile> = BTreeMap::new();
        for entry in std::fs::read_dir(&main_dir)
            .with_context(|| format!("reading {}", main_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let stem = if let Some(s) = name.strip_suffix(".yaml") {
                s
            } else if let Some(s) = name.strip_suffix(".yml") {
                s
            } else {
                continue;
            };
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let cf = parse_control_file(&content)
                .with_context(|| format!("parsing {}", path.display()))?;
            let cfn = ControlFileName::new(stem).map_err(|e| anyhow!(e))?;
            if controls.contains_key(&cfn) {
                bail!("duplicate control file name (case-insensitive): {:?}", stem);
            }
            controls.insert(cfn, cf);
        }

        let tests_path = dir.join("src/test/tests.yaml");
        let tests = if tests_path.is_file() {
            let content = std::fs::read_to_string(&tests_path)
                .with_context(|| format!("reading {}", tests_path.display()))?;
            let tf = parse_test_file(&content)
                .with_context(|| format!("parsing {}", tests_path.display()))?;
            TestsYaml { inner: tf }
        } else {
            TestsYaml::empty()
        };

        let mut fixtures: BTreeMap<FixtureFileName, FixtureFile> = BTreeMap::new();
        let resources_dir = dir.join("src/test/resources");
        if resources_dir.is_dir() {
            for entry in std::fs::read_dir(&resources_dir)
                .with_context(|| format!("reading {}", resources_dir.display()))?
            {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let fxn = FixtureFileName::new(&name).map_err(|e| anyhow!(e))?;
                let ff = load_fixture_dir(&entry.path())?;
                if fixtures.contains_key(&fxn) {
                    bail!("duplicate fixture name (case-insensitive): {:?}", name);
                }
                fixtures.insert(fxn, ff);
            }
        }

        let meta = ProjectMeta {
            name: dir.file_name().map(|s| s.to_string_lossy().to_string()),
        };

        Ok(Project {
            controls,
            fixtures,
            tests,
            meta,
        })
    }

    /// Materialize the project to disk under `<dir>` using the same layout
    /// `Project::load_from_dir` accepts. Creates directories as needed.
    pub fn write_to_dir(&self, dir: &Path) -> Result<()> {
        let main_dir = dir.join("src/main");
        std::fs::create_dir_all(&main_dir)
            .with_context(|| format!("creating {}", main_dir.display()))?;
        for (name, cf) in &self.controls {
            let path = main_dir.join(format!("{}.yaml", name.as_str()));
            std::fs::write(&path, generate_control_file(cf))
                .with_context(|| format!("writing {}", path.display()))?;
        }

        let test_dir = dir.join("src/test");
        std::fs::create_dir_all(&test_dir)
            .with_context(|| format!("creating {}", test_dir.display()))?;
        let tests_path = test_dir.join("tests.yaml");
        std::fs::write(&tests_path, generate_test_file(&self.tests.inner))
            .with_context(|| format!("writing {}", tests_path.display()))?;

        let resources_dir = test_dir.join("resources");
        std::fs::create_dir_all(&resources_dir)
            .with_context(|| format!("creating {}", resources_dir.display()))?;
        for (name, ff) in &self.fixtures {
            let fdir = resources_dir.join(name.as_str());
            std::fs::create_dir_all(&fdir)
                .with_context(|| format!("creating {}", fdir.display()))?;
            if let Some(po) = &ff.pseudo_overrides {
                let yaml = serialize_pseudo_overrides(po);
                std::fs::write(fdir.join("pseudo-file-overrides.yaml"), yaml).with_context(
                    || format!("writing pseudo-file-overrides.yaml in {}", fdir.display()),
                )?;
            }
            for (rel, body) in &ff.files {
                let target = fdir.join(rel);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
                std::fs::write(&target, body)
                    .with_context(|| format!("writing {}", target.display()))?;
            }
        }
        Ok(())
    }
}

fn load_fixture_dir(dir: &Path) -> Result<FixtureFile> {
    let mut ff = FixtureFile::default();
    walk_fixture(dir, dir, &mut ff)?;
    Ok(ff)
}

fn walk_fixture(root: &Path, cur: &Path, ff: &mut FixtureFile) -> Result<()> {
    for entry in std::fs::read_dir(cur).with_context(|| format!("reading {}", cur.display()))? {
        let entry = entry?;
        let p = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk_fixture(root, &p, ff)?;
            continue;
        }
        if !ty.is_file() {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .map_err(|_| anyhow!("path-relativization failed for {}", p.display()))?
            .to_string_lossy()
            .to_string();
        if rel == "pseudo-file-overrides.yaml" {
            let yaml =
                std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            let (po, _warnings) = parse_fixture_collect_warnings(&yaml)
                .with_context(|| format!("parsing {}", p.display()))?;
            ff.pseudo_overrides = Some(po);
            continue;
        }
        let body = std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?;
        ff.files.insert(rel, body);
    }
    Ok(())
}

/// Round-trip-stable serialization of pseudo-file overrides. Matches the
/// `parse_fixture_collect_warnings` accept-set: a YAML mapping with optional
/// `env` (mapping of NAME→VALUE) and `executable` (mapping of NAME→snapshot).
fn serialize_pseudo_overrides(po: &PseudoFileFixture) -> String {
    use serde_yaml::{Mapping, Value};
    let mut top = Mapping::new();
    if let Some(env) = &po.env_override {
        let mut m = Mapping::new();
        for (k, v) in env {
            m.insert(Value::String(k.clone()), Value::String(v.clone()));
        }
        top.insert(Value::String("env".into()), Value::Mapping(m));
    }
    if let Some(execs) = &po.executable_override {
        let mut m = Mapping::new();
        for (name, snap) in execs {
            let mut s = Mapping::new();
            s.insert(Value::String("found".into()), Value::Bool(snap.found));
            s.insert(
                Value::String("executable".into()),
                Value::Bool(snap.executable),
            );
            if let Some(p) = &snap.path {
                s.insert(Value::String("path".into()), Value::String(p.clone()));
            }
            if let Some(c) = &snap.command_full {
                s.insert(
                    Value::String("command-full".into()),
                    Value::String(c.clone()),
                );
            }
            if let Some(v) = &snap.version_full {
                s.insert(
                    Value::String("version-full".into()),
                    Value::String(v.clone()),
                );
            }
            if let Some(v) = &snap.version {
                s.insert(Value::String("version".into()), Value::String(v.clone()));
            }
            m.insert(Value::String(name.clone()), Value::Mapping(s));
        }
        top.insert(Value::String("executable".into()), Value::Mapping(m));
    }
    serde_yaml::to_string(&Value::Mapping(top)).expect("serialize pseudo overrides")
}

// ---------------------------------------------------------------------------
// Evaluation (spec §1.4)
// ---------------------------------------------------------------------------

/// Result of running a project's tests in memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestsReport {
    pub passed: usize,
    pub failed: usize,
    pub failure_messages: Vec<String>,
}

/// Result of running a project's controls against the host filesystem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditReport {
    pub passed: usize,
    pub failed: usize,
    pub warned: usize,
    pub failure_messages: Vec<String>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl Project {
    /// Run every test entry against its fixture, in memory.
    ///
    /// We satisfy the "in-memory" contract by writing each fixture's files
    /// to a per-call tempdir whose lifetime is bounded by this function:
    /// every fixture is supplied by the Project ADT, no on-disk project
    /// layout is required, the host filesystem outside the tempdir is not
    /// observed. An InMemory subject path on the predicate evaluator is the
    /// long-term cleanup; for now this preserves predicate-evaluator reuse
    /// per spec §1.4 ("only the subject-resolution path differs").
    pub fn run_tests(&self) -> Result<TestsReport> {
        let mut report = TestsReport::default();
        let temp = tempdir_unique()?;
        for suite in &self.tests.inner.test_suites {
            for tc in &suite.tests {
                let control_id = tc.control_id.as_str();
                let owning = match self.find_control_file_for_id(control_id) {
                    Some(_) => self
                        .all_controls()
                        .into_iter()
                        .find(|c| c.id == control_id)
                        .expect("just-found control"),
                    None => {
                        report.failed += 1;
                        report.failure_messages.push(format!(
                            "test references unknown control id {:?}",
                            control_id
                        ));
                        continue;
                    }
                };
                let fixture_name = match FixtureFileName::new(&tc.fixture) {
                    Ok(n) => n,
                    Err(e) => {
                        report.failed += 1;
                        report
                            .failure_messages
                            .push(format!("invalid fixture name {:?}: {}", tc.fixture, e));
                        continue;
                    }
                };
                let ff = match self.fixtures.get(&fixture_name) {
                    Some(f) => f,
                    None => {
                        report.failed += 1;
                        report
                            .failure_messages
                            .push(format!("test references missing fixture {:?}", tc.fixture));
                        continue;
                    }
                };
                // Materialize the fixture under the tempdir.
                let fixture_dir = temp.join(format!("{}__{}", control_id, tc.fixture));
                std::fs::create_dir_all(&fixture_dir)?;
                for (rel, body) in &ff.files {
                    let target = fixture_dir.join(rel);
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&target, body)?;
                }
                let eval_result = match &ff.pseudo_overrides {
                    Some(po) => {
                        let ctx = EvalContext::with_fixture(fixture_dir.clone(), po.clone());
                        evaluate_with_ctx(&owning.check, &ctx)
                    }
                    None => evaluate(&owning.check, &fixture_dir),
                };
                let case_ok = compare_test(&tc.expect, &eval_result);
                if case_ok {
                    report.passed += 1;
                } else {
                    let summary = match (&tc.expect, &eval_result) {
                        (crate::rules::ast::TestExpectation::Pass, Err(failures)) => format!(
                            "{} on fixture {}: expected pass but got {} failure(s): {}",
                            control_id,
                            tc.fixture,
                            failures.len(),
                            failures
                                .iter()
                                .map(|f| f.to_string())
                                .collect::<Vec<_>>()
                                .join("; ")
                        ),
                        (crate::rules::ast::TestExpectation::Fail(_), Ok(())) => format!(
                            "{} on fixture {}: expected failure but control passed",
                            control_id, tc.fixture
                        ),
                        (crate::rules::ast::TestExpectation::Fail(fe), Err(failures)) => format!(
                            "{} on fixture {}: failure-detail mismatch (expected {:?}/{:?}, got {} failure(s))",
                            control_id, tc.fixture, fe.count, fe.messages, failures.len()
                        ),
                        _ => format!("{} on fixture {}: ?", control_id, tc.fixture),
                    };
                    report.failed += 1;
                    report.failure_messages.push(summary);
                }
            }
        }
        cleanup_tempdir(&temp);
        Ok(report)
    }

    /// Run every control against the real host filesystem rooted at `home`.
    /// Pseudo-file overrides remain test-only; the live audit uses the host.
    pub fn run_audit_against_filesystem(
        &self,
        home: &Path,
        ignore: &[String],
        warn_only: &[String],
    ) -> AuditReport {
        let mut report = AuditReport::default();
        for c in self.all_controls() {
            if ignore.contains(&c.id) {
                continue;
            }
            let is_warn = warn_only.contains(&c.id);
            match evaluate(&c.check, home) {
                Ok(()) => report.passed += 1,
                Err(failures) => {
                    if is_warn {
                        report.warned += 1;
                    } else {
                        report.failed += 1;
                    }
                    for f in &failures {
                        report
                            .failure_messages
                            .push(format!("[{}] {}: {}", c.id, f.path, f.message));
                    }
                }
            }
        }
        report
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn compare_test(
    expect: &crate::rules::ast::TestExpectation,
    actual: &Result<(), Vec<RuleFailure>>,
) -> bool {
    use crate::rules::ast::TestExpectation;
    match (expect, actual) {
        (TestExpectation::Pass, Ok(())) => true,
        (TestExpectation::Pass, Err(_)) => false,
        (TestExpectation::Fail(_), Ok(())) => false,
        (TestExpectation::Fail(fe), Err(failures)) => {
            if let Some(expected_count) = fe.count {
                if failures.len() != expected_count {
                    return false;
                }
            }
            if !fe.messages.is_empty() {
                let combined: String = failures
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                for needle in &fe.messages {
                    if !combined.contains(needle) {
                        return false;
                    }
                }
            }
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Tiny tempdir helper (avoids a hard dep on `tempfile` outside dev-deps).
// ---------------------------------------------------------------------------

#[cfg_attr(not(test), allow(dead_code))]
fn tempdir_unique() -> Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("key-project-{}-{}-{}", pid, nanos, n));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

#[cfg_attr(not(test), allow(dead_code))]
fn cleanup_tempdir(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}

// ---------------------------------------------------------------------------
// Project-mutation operations (spec §4.1)
//
// Top-level "AsmOps" at the project surface, named with the same vocabulary
// as the spec. These are SEMANTIC mutation operations (one logical add, one
// logical delete). Sub-Add* dialogs in the live UX would compile each Add*
// into a sub-Interaction over the primitive AsmOp alphabet (Select/Enter/
// Yes/No/Back); for the round-trip property what matters is that the
// semantic op set and `apply` are inverse to `compile_project`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectMutation {
    AddControl {
        file: ControlFileName,
        control: Control,
    },
    DeleteControl {
        file: ControlFileName,
    },
    AddFixture {
        name: FixtureFileName,
        fixture: FixtureFile,
    },
    DeleteFixture {
        name: FixtureFileName,
    },
    AddTestEntry {
        suite: String,
        tc: TestCase,
    },
    DeleteTestEntry {
        suite: String,
        control_id: String,
        fixture: String,
    },
    /// Commits the current Project — terminates the dialog.
    Done,
}

impl Project {
    /// Apply a single mutation, returning a new Project. Errors are explicit.
    pub fn apply_mutation(self, op: ProjectMutation) -> Result<Project, ProjectMutationError> {
        match op {
            ProjectMutation::AddControl { file, control } => {
                // If the control file already exists, append the control to it
                // instead of erroring — this lets compile_project produce a
                // single AddControl per logical control even when several share
                // a file.
                if self.controls.contains_key(&file) {
                    let mut p = self;
                    let cf = p.controls.get_mut(&file).expect("just-checked");
                    if cf.controls.iter().any(|c| c.id == control.id) {
                        return Err(ProjectMutationError::DuplicateControl(control.id));
                    }
                    cf.controls.push(control);
                    Ok(p)
                } else {
                    self.with_control_added(
                        file,
                        ControlFile {
                            controls: vec![control],
                        },
                    )
                }
            }
            ProjectMutation::DeleteControl { file } => self.with_control_deleted(&file),
            ProjectMutation::AddFixture { name, fixture } => self.with_fixture_added(name, fixture),
            ProjectMutation::DeleteFixture { name } => self.with_fixture_deleted(&name),
            ProjectMutation::AddTestEntry { suite, tc } => self.with_test_entry_added(&suite, tc),
            ProjectMutation::DeleteTestEntry {
                suite,
                control_id,
                fixture,
            } => self.with_test_entry_deleted(&suite, &control_id, &fixture),
            ProjectMutation::Done => Ok(self),
        }
    }

    /// Apply a sequence of mutations from `start`, stopping at the first
    /// `Done` or end of stream.
    pub fn apply_mutations(
        start: Project,
        ops: Vec<ProjectMutation>,
    ) -> Result<Project, ProjectMutationError> {
        let mut p = start;
        for op in ops {
            let done = matches!(op, ProjectMutation::Done);
            p = p.apply_mutation(op)?;
            if done {
                break;
            }
        }
        Ok(p)
    }
}

/// Compile a Project into the deterministic mutation sequence that, applied
/// to `Project::empty()`, reconstructs it exactly. (Spec §5.1 round-trip.)
pub fn compile_project(p: &Project) -> Vec<ProjectMutation> {
    let mut ops = Vec::new();
    // Fixtures first so test entries can reference them at apply-time.
    for (name, fixture) in &p.fixtures {
        ops.push(ProjectMutation::AddFixture {
            name: name.clone(),
            fixture: fixture.clone(),
        });
    }
    for (file, cf) in &p.controls {
        for control in &cf.controls {
            ops.push(ProjectMutation::AddControl {
                file: file.clone(),
                control: control.clone(),
            });
        }
    }
    for suite in &p.tests.inner.test_suites {
        for tc in &suite.tests {
            ops.push(ProjectMutation::AddTestEntry {
                suite: suite.name.clone(),
                tc: tc.clone(),
            });
        }
    }
    ops.push(ProjectMutation::Done);
    ops
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::{
        Control, ControlFile, FailExpectation, FilePredicateAst, Proposition, SimplePath, TestCase,
        TestExpectation,
    };

    fn sample_control(id: &str) -> Control {
        Control {
            id: id.to_string(),
            title: format!("title-{}", id),
            description: format!("desc-{}", id),
            remediation: format!("rem-{}", id),
            check: Proposition::FileSatisfies {
                path: SimplePath::new("~/x").unwrap(),
                check: FilePredicateAst::FileExists,
            },
        }
    }

    #[test]
    fn empty_project_has_no_controls_or_fixtures() {
        let p = Project::empty();
        assert!(p.controls.is_empty());
        assert!(p.fixtures.is_empty());
        assert!(p.tests.inner.test_suites.is_empty());
    }

    #[test]
    fn add_control_inserts() {
        let p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        let p = p.with_control_added(cfn.clone(), cf).unwrap();
        assert!(p.controls.contains_key(&cfn));
    }

    #[test]
    fn duplicate_control_rejected() {
        let p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        let p = p.with_control_added(cfn.clone(), cf.clone()).unwrap();
        let err = p.with_control_added(cfn, cf).unwrap_err();
        assert!(matches!(err, ProjectMutationError::DuplicateControl(_)));
    }

    #[test]
    fn duplicate_control_case_insensitive() {
        let p = Project::empty();
        let a = ControlFileName::new("Alpha").unwrap();
        let b = ControlFileName::new("ALPHA").unwrap();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        let p = p.with_control_added(a, cf.clone()).unwrap();
        let err = p.with_control_added(b, cf).unwrap_err();
        assert!(matches!(err, ProjectMutationError::DuplicateControl(_)));
    }

    #[test]
    fn delete_control_removes() {
        let p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        let p = p.with_control_added(cfn.clone(), cf).unwrap();
        let p = p.with_control_deleted(&cfn).unwrap();
        assert!(p.controls.is_empty());
    }

    #[test]
    fn delete_missing_control_errors() {
        let p = Project::empty();
        let cfn = ControlFileName::new("ghost").unwrap();
        let err = p.with_control_deleted(&cfn).unwrap_err();
        assert!(matches!(err, ProjectMutationError::NotFoundControl(_)));
    }

    #[test]
    fn add_and_delete_fixture() {
        let p = Project::empty();
        let fxn = FixtureFileName::new("fx1").unwrap();
        let p = p
            .with_fixture_added(fxn.clone(), FixtureFile::default())
            .unwrap();
        assert_eq!(p.fixtures.len(), 1);
        let p = p.with_fixture_deleted(&fxn).unwrap();
        assert_eq!(p.fixtures.len(), 0);
    }

    #[test]
    fn duplicate_fixture_rejected() {
        let p = Project::empty();
        let fxn = FixtureFileName::new("fx1").unwrap();
        let p = p
            .with_fixture_added(fxn.clone(), FixtureFile::default())
            .unwrap();
        let err = p
            .with_fixture_added(fxn, FixtureFile::default())
            .unwrap_err();
        assert!(matches!(err, ProjectMutationError::DuplicateFixture(_)));
    }

    #[test]
    fn add_test_entry_creates_suite() {
        let p = Project::empty();
        let tc = TestCase {
            control_id: "X".into(),
            description: "d".into(),
            fixture: "fx1".into(),
            expect: TestExpectation::Pass,
        };
        let p = p.with_test_entry_added("default", tc).unwrap();
        assert_eq!(p.tests.inner.test_suites.len(), 1);
        assert_eq!(p.tests.inner.test_suites[0].tests.len(), 1);
    }

    #[test]
    fn duplicate_test_entry_rejected() {
        let p = Project::empty();
        let tc = TestCase {
            control_id: "X".into(),
            description: "d".into(),
            fixture: "fx1".into(),
            expect: TestExpectation::Pass,
        };
        let p = p.with_test_entry_added("default", tc.clone()).unwrap();
        let err = p.with_test_entry_added("default", tc).unwrap_err();
        assert!(matches!(
            err,
            ProjectMutationError::DuplicateTestEntry { .. }
        ));
    }

    #[test]
    fn delete_test_entry_drops_empty_suite() {
        let p = Project::empty();
        let tc = TestCase {
            control_id: "X".into(),
            description: "d".into(),
            fixture: "fx1".into(),
            expect: TestExpectation::Pass,
        };
        let p = p.with_test_entry_added("default", tc).unwrap();
        let p = p.with_test_entry_deleted("default", "X", "fx1").unwrap();
        assert!(p.tests.inner.test_suites.is_empty());
    }

    #[test]
    fn validate_references_catches_dangling_control() {
        let p = Project::empty();
        let fxn = FixtureFileName::new("fx1").unwrap();
        let p = p.with_fixture_added(fxn, FixtureFile::default()).unwrap();
        let p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "MISSING".into(),
                    description: "d".into(),
                    fixture: "fx1".into(),
                    expect: TestExpectation::Pass,
                },
            )
            .unwrap();
        let err = p.validate_references().unwrap_err();
        assert!(matches!(err, ProjectMutationError::DanglingControl(_)));
    }

    #[test]
    fn validate_references_catches_dangling_fixture() {
        let p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        let p = p.with_control_added(cfn, cf).unwrap();
        let p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "missingfx".into(),
                    expect: TestExpectation::Pass,
                },
            )
            .unwrap();
        let err = p.validate_references().unwrap_err();
        assert!(matches!(err, ProjectMutationError::DanglingFixture(_)));
    }

    #[test]
    fn round_trip_write_and_load() {
        let mut p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        p = p.with_control_added(cfn, cf).unwrap();

        let fxn = FixtureFileName::new("fx1").unwrap();
        let mut ff = FixtureFile::default();
        ff.files.insert("x".into(), b"hello".to_vec());
        p = p.with_fixture_added(fxn, ff).unwrap();

        p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "fx1".into(),
                    expect: TestExpectation::Pass,
                },
            )
            .unwrap();

        let dir = tempdir_unique().unwrap();
        p.write_to_dir(&dir).unwrap();
        let loaded = Project::load_from_dir(&dir).unwrap();
        cleanup_tempdir(&dir);

        assert_eq!(p.controls, loaded.controls);
        assert_eq!(p.tests, loaded.tests);
        // fixtures match modulo the meta-name
        assert_eq!(p.fixtures.len(), loaded.fixtures.len());
        for (k, v) in &p.fixtures {
            let l = &loaded.fixtures[k];
            assert_eq!(v.files, l.files);
            assert_eq!(v.pseudo_overrides, l.pseudo_overrides);
        }
    }

    #[test]
    fn run_tests_passes_for_satisfied_predicate() {
        let mut p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let mut control = sample_control("X");
        control.check = Proposition::FileSatisfies {
            path: SimplePath::new("~/x").unwrap(),
            check: FilePredicateAst::FileExists,
        };
        p = p
            .with_control_added(
                cfn,
                ControlFile {
                    controls: vec![control],
                },
            )
            .unwrap();

        let fxn = FixtureFileName::new("fx1").unwrap();
        let mut ff = FixtureFile::default();
        ff.files.insert("x".into(), b"".to_vec());
        p = p.with_fixture_added(fxn, ff).unwrap();
        p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "fx1".into(),
                    expect: TestExpectation::Pass,
                },
            )
            .unwrap();

        let report = p.run_tests().unwrap();
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn run_tests_reports_unmet_expectation() {
        let mut p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let control = Control {
            id: "X".into(),
            title: "t".into(),
            description: "d".into(),
            remediation: "r".into(),
            check: Proposition::FileSatisfies {
                path: SimplePath::new("~/missing").unwrap(),
                check: FilePredicateAst::FileExists,
            },
        };
        p = p
            .with_control_added(
                cfn,
                ControlFile {
                    controls: vec![control],
                },
            )
            .unwrap();

        let fxn = FixtureFileName::new("empty").unwrap();
        p = p.with_fixture_added(fxn, FixtureFile::default()).unwrap();
        p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "empty".into(),
                    expect: TestExpectation::Pass,
                },
            )
            .unwrap();

        let report = p.run_tests().unwrap();
        assert_eq!(report.passed, 0);
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn fail_expectation_with_message_match() {
        let mut p = Project::empty();
        let cfn = ControlFileName::new("alpha").unwrap();
        let control = Control {
            id: "X".into(),
            title: "t".into(),
            description: "d".into(),
            remediation: "r".into(),
            check: Proposition::FileSatisfies {
                path: SimplePath::new("~/missing").unwrap(),
                check: FilePredicateAst::FileExists,
            },
        };
        p = p
            .with_control_added(
                cfn,
                ControlFile {
                    controls: vec![control],
                },
            )
            .unwrap();

        let fxn = FixtureFileName::new("empty").unwrap();
        p = p.with_fixture_added(fxn, FixtureFile::default()).unwrap();
        p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "empty".into(),
                    expect: TestExpectation::Fail(FailExpectation {
                        count: Some(1),
                        messages: vec!["does not exist".into()],
                    }),
                },
            )
            .unwrap();

        let report = p.run_tests().unwrap();
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn run_audit_filesystem_returns_pass_count() {
        let p = Project::empty();
        // Empty project = zero controls, zero failures.
        let report = p.run_audit_against_filesystem(Path::new("/"), &[], &[]);
        assert_eq!(report.passed, 0);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn invalid_branded_name_rejected() {
        assert!(ControlFileName::new("").is_err());
        assert!(ControlFileName::new("..").is_err());
        assert!(ControlFileName::new("has space").is_err());
        assert!(ControlFileName::new("ok-name_1.0").is_ok());
    }

    // ---------------------------------------------------------------------
    // Project-mutation round-trip (spec §4.1, §5.1)
    // ---------------------------------------------------------------------

    fn build_sample_project() -> Project {
        let mut p = Project::empty();
        p = p
            .with_fixture_added("fxA".try_into().unwrap(), FixtureFile::default())
            .unwrap();
        p = p
            .with_control_added(
                "alpha".try_into().unwrap(),
                ControlFile {
                    controls: vec![sample_control("X")],
                },
            )
            .unwrap();
        p = p
            .with_test_entry_added(
                "default",
                TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "fxA".into(),
                    expect: TestExpectation::Pass,
                },
            )
            .unwrap();
        p
    }

    #[test]
    fn compile_project_round_trip_empty() {
        let p = Project::empty();
        let ops = compile_project(&p);
        let rebuilt = Project::apply_mutations(Project::empty(), ops).unwrap();
        assert_eq!(rebuilt, p);
    }

    #[test]
    fn compile_project_round_trip_basic() {
        let p = build_sample_project();
        let ops = compile_project(&p);
        let rebuilt = Project::apply_mutations(Project::empty(), ops).unwrap();
        assert_eq!(rebuilt, p);
    }

    #[test]
    fn project_apply_done_terminates() {
        let p = Project::empty();
        let ops = vec![
            ProjectMutation::AddFixture {
                name: "fxA".try_into().unwrap(),
                fixture: FixtureFile::default(),
            },
            ProjectMutation::Done,
            // This should be ignored after Done.
            ProjectMutation::AddFixture {
                name: "fxB".try_into().unwrap(),
                fixture: FixtureFile::default(),
            },
        ];
        let result = Project::apply_mutations(p, ops).unwrap();
        assert_eq!(result.fixtures.len(), 1);
    }

    #[test]
    fn project_delete_then_add_same_id() {
        // Hand-curated edge case (spec §5.1 d): delete-then-add same id.
        let p = Project::empty();
        let ops = vec![
            ProjectMutation::AddFixture {
                name: "fx1".try_into().unwrap(),
                fixture: FixtureFile::default(),
            },
            ProjectMutation::DeleteFixture {
                name: "fx1".try_into().unwrap(),
            },
            ProjectMutation::AddFixture {
                name: "fx1".try_into().unwrap(),
                fixture: FixtureFile::default(),
            },
            ProjectMutation::Done,
        ];
        let result = Project::apply_mutations(p, ops).unwrap();
        assert_eq!(result.fixtures.len(), 1);
    }

    #[test]
    fn project_only_delete_on_empty_errors() {
        // Hand-curated edge case (spec §5.1 d): delete on empty project.
        let p = Project::empty();
        let ops = vec![ProjectMutation::DeleteFixture {
            name: "missing".try_into().unwrap(),
        }];
        let err = Project::apply_mutations(p, ops).unwrap_err();
        assert!(matches!(err, ProjectMutationError::NotFoundFixture(_)));
    }

    #[test]
    fn brand_collision_detected() {
        // Hand-curated edge case (spec §5.1 d): two controls whose ids brand-
        // collide (case-insensitive).
        let p = Project::empty();
        let cf = ControlFile {
            controls: vec![sample_control("X")],
        };
        let p = p
            .with_control_added("Alpha".try_into().unwrap(), cf.clone())
            .unwrap();
        let err = p
            .with_control_added("ALPHA".try_into().unwrap(), cf)
            .unwrap_err();
        assert!(matches!(err, ProjectMutationError::DuplicateControl(_)));
    }

    #[test]
    fn add_test_before_control_then_validate_dangling() {
        // Hand-curated edge case (spec §5.1 d): test entry added before its
        // control / fixture exist — apply succeeds; validate_references catches
        // the dangling refs.
        let p = Project::empty();
        let ops = vec![
            ProjectMutation::AddTestEntry {
                suite: "default".into(),
                tc: TestCase {
                    control_id: "X".into(),
                    description: "d".into(),
                    fixture: "fx1".into(),
                    expect: TestExpectation::Pass,
                },
            },
            ProjectMutation::Done,
        ];
        let result = Project::apply_mutations(p, ops).unwrap();
        assert!(result.validate_references().is_err());
    }

    #[test]
    fn round_trip_multi_control_file() {
        // Multiple controls in the same file should compile to a single file
        // and round-trip cleanly.
        let mut p = Project::empty();
        p = p
            .with_control_added(
                "shared".try_into().unwrap(),
                ControlFile {
                    controls: vec![sample_control("A"), sample_control("B")],
                },
            )
            .unwrap();
        let ops = compile_project(&p);
        let rebuilt = Project::apply_mutations(Project::empty(), ops).unwrap();
        assert_eq!(rebuilt, p);
    }
}

// Tiny TryFrom helpers used by the test corpus. They give a concise way to
// construct branded names from string literals in tests.
impl TryFrom<&str> for ControlFileName {
    type Error = ProjectNameError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        ControlFileName::new(s)
    }
}

impl TryFrom<&str> for FixtureFileName {
    type Error = ProjectNameError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        FixtureFileName::new(s)
    }
}
