use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Schema for validating JSON/YAML data structures.
#[derive(Debug, Clone, PartialEq)]
pub enum DataSchema {
    /// Matches any value (keyword: `anything`).
    Anything,
    /// Value must be a string.
    IsString,
    /// Value must be a string matching a regex.
    IsStringMatching(String),
    /// Value must be a number.
    IsNumber,
    /// Value must be a boolean.
    IsBool,
    /// Spec/0013 §A.7B — value must be the JSON boolean `true` (strict).
    IsTrue,
    /// Spec/0013 §A.7B — value must be the JSON boolean `false` (strict).
    IsFalse,
    /// Value must be null.
    IsNull,
    /// Value must be an object with (at least) these keys satisfying sub-schemas.
    IsObject(Vec<(String, DataSchema)>),
    /// Value must be an array satisfying the given constraints.
    IsArray(DataArrayCheck),
}

/// Constraints on array elements.
#[derive(Debug, Clone, PartialEq)]
pub struct DataArrayCheck {
    /// Every element must match this schema.
    pub forall: Option<Box<DataSchema>>,
    /// At least one element must match this schema.
    pub exists: Option<Box<DataSchema>>,
    /// Specific indices must match their schemas.
    pub at: Vec<(u32, DataSchema)>,
}

// ---------------------------------------------------------------------------
// SimplePath — concrete or pseudo-file
// (spec/0009 §1.1: pseudo-file identifiers `<env>` and `<executable:NAME>`)
// ---------------------------------------------------------------------------

/// A pseudo-file identifier (spec/0009 §1.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PseudoFile {
    /// `<env>` — current shell-style environment (spec/0009 §2)
    Env,
    /// `<executable:NAME>` — introspected executable on PATH (spec/0009 §3)
    Executable(String),
}

impl PseudoFile {
    /// Render as the canonical `<...>` token.
    pub fn as_token(&self) -> String {
        match self {
            PseudoFile::Env => "<env>".to_string(),
            PseudoFile::Executable(name) => format!("<executable:{}>", name),
        }
    }
}

/// A validated path: either concrete `~/...` (no `.`, `..`, `//` segments)
/// or a pseudo-file identifier `<...>` (spec/0009 §1.1).
///
/// Stores the original textual form so `as_str()` is cheap for either kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimplePath {
    raw: String,
    kind: SimplePathKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SimplePathKind {
    Concrete,
    Pseudo(PseudoFile),
}

impl SimplePath {
    pub fn new(s: &str) -> Result<Self> {
        if let Some(stripped) = s.strip_prefix('<') {
            if !stripped.ends_with('>') {
                bail!("pseudo-file identifier must end with '>': got {:?}", s);
            }
            let inner = &stripped[..stripped.len() - 1];
            if inner.is_empty() {
                bail!("pseudo-file identifier must not be empty: got {:?}", s);
            }
            let (keyword, arg) = match inner.find(':') {
                Some(idx) => (&inner[..idx], Some(&inner[idx + 1..])),
                None => (inner, None),
            };
            let pseudo = match (keyword, arg) {
                ("env", None) => PseudoFile::Env,
                ("env", Some(_)) => bail!(
                    "unknown pseudo-file: {:?}. `<env:...>` forms are not yet defined; \
                     only `<env>` is supported in this increment.",
                    s
                ),
                ("executable", Some(name)) => {
                    if name.is_empty() {
                        bail!(
                            "pseudo-file `<executable:NAME>` requires a non-empty NAME: got {:?}",
                            s
                        );
                    }
                    if name.starts_with('/') {
                        bail!(
                            "pseudo-file `<executable:NAME>` does not accept absolute paths; \
                             NAME must be a bare command name (use a concrete path with \
                             `is-executable` for absolute paths). got {:?}",
                            s
                        );
                    }
                    if !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
                    {
                        bail!(
                            "pseudo-file `<executable:NAME>` requires NAME to match \
                             [A-Za-z0-9_.+-]+; got {:?}",
                            s
                        );
                    }
                    PseudoFile::Executable(name.to_string())
                }
                ("executable", None) => bail!(
                    "pseudo-file `<executable>` requires a NAME: use `<executable:NAME>`. \
                     got {:?}",
                    s
                ),
                _ => bail!(
                    "unknown pseudo-file keyword: {:?} in {:?}. \
                     Valid pseudo-files: `<env>`, `<executable:NAME>`.",
                    keyword,
                    s
                ),
            };
            Ok(SimplePath {
                raw: s.to_string(),
                kind: SimplePathKind::Pseudo(pseudo),
            })
        } else if s == "~" || s.starts_with("~/") {
            if s.contains("/./") || s.contains("/../") || s.contains("//") {
                bail!("SimplePath must not contain /./ or /../ or //: got {:?}", s);
            }
            Ok(SimplePath {
                raw: s.to_string(),
                kind: SimplePathKind::Concrete,
            })
        } else {
            bail!(
                "SimplePath must start with '~/' (concrete) or '<' (pseudo-file): got {:?}",
                s
            );
        }
    }

    /// Render as the original textual form (`~/...` or `<...>`).
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    #[allow(dead_code)]
    pub fn is_pseudo(&self) -> bool {
        matches!(self.kind, SimplePathKind::Pseudo(_))
    }

    pub fn pseudo(&self) -> Option<&PseudoFile> {
        match &self.kind {
            SimplePathKind::Pseudo(p) => Some(p),
            _ => None,
        }
    }

    /// Resolve a concrete simple-path to a real file path under the given home.
    /// Panics if called on a pseudo-file (callers should branch on `is_pseudo`).
    pub fn resolve(&self, home_dir: &Path) -> PathBuf {
        match &self.kind {
            SimplePathKind::Concrete if self.raw == "~" => home_dir.to_path_buf(),
            SimplePathKind::Concrete => home_dir.join(&self.raw[2..]),
            SimplePathKind::Pseudo(p) => panic!(
                "SimplePath::resolve called on pseudo-file {:?}",
                p.as_token()
            ),
        }
    }
}

impl std::fmt::Display for SimplePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilePredicateAst {
    FileExists,
    TextMatchesRegex(String),
    TextContains(String),
    TextHasLines {
        min: Option<u32>,
        max: Option<u32>,
    },
    ShellExports(String),
    ShellExportsValueMatches {
        name: String,
        value_regex: String,
    },
    ShellDefinesVariable(String),
    ShellDefinesVariableValueMatches {
        name: String,
        value_regex: String,
    },
    ShellAddsToPath(String),
    PropertiesDefinesKey(String),
    XmlMatchesPath(String),
    JsonMatches(DataSchema),
    YamlMatches(DataSchema),
    All(Vec<FilePredicateAst>),
    Any {
        hint: String,
        checks: Vec<FilePredicateAst>,
    },
    Not(Box<FilePredicateAst>),
    Conditionally {
        condition: Box<FilePredicateAst>,
        then: Box<FilePredicateAst>,
    },
}

#[cfg(test)]
impl FilePredicateAst {
    pub fn yaml_key(&self) -> &'static str {
        match self {
            FilePredicateAst::FileExists => "file-exists",
            FilePredicateAst::TextMatchesRegex(_) => "text-matches",
            FilePredicateAst::TextContains(_) => "text-contains",
            FilePredicateAst::TextHasLines { .. } => "text-has-lines",
            FilePredicateAst::ShellExports(_) => "shell-exports",
            FilePredicateAst::ShellExportsValueMatches { .. } => "shell-exports-value-matches",
            FilePredicateAst::ShellDefinesVariable(_) => "shell-defines",
            FilePredicateAst::ShellDefinesVariableValueMatches { .. } => {
                "shell-defines-value-matches"
            }
            FilePredicateAst::ShellAddsToPath(_) => "shell-adds-to-path",
            FilePredicateAst::PropertiesDefinesKey(_) => "properties-defines-key",
            FilePredicateAst::XmlMatchesPath(_) => "xml-matches",
            FilePredicateAst::JsonMatches(_) => "json-matches",
            FilePredicateAst::YamlMatches(_) => "yaml-matches",
            FilePredicateAst::All(_) => "all",
            FilePredicateAst::Any { .. } => "any",
            FilePredicateAst::Not(_) => "not",
            FilePredicateAst::Conditionally { .. } => "conditionally",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Proposition {
    FileSatisfies {
        path: SimplePath,
        check: FilePredicateAst,
    },
    Forall {
        files: Vec<SimplePath>,
        check: FilePredicateAst,
    },
    Exists {
        files: Vec<SimplePath>,
        check: FilePredicateAst,
    },
    All(Vec<Proposition>),
    Any(Vec<Proposition>),
    Not(Box<Proposition>),
    Conditionally {
        condition: Box<Proposition>,
        then: Box<Proposition>,
    },
}

#[cfg(test)]
impl Proposition {
    pub fn yaml_key(&self) -> &'static str {
        match self {
            Proposition::FileSatisfies { .. } => "file",
            Proposition::Forall { .. } => "forall",
            Proposition::Exists { .. } => "exists",
            Proposition::All(_) => "all",
            Proposition::Any(_) => "any",
            Proposition::Not(_) => "not",
            Proposition::Conditionally { .. } => "conditionally",
        }
    }
}

// ---------------------------------------------------------------------------
// Pseudo-file snapshots (spec/0009 §3.2 ExecutableSnapshot, §2.5 env_override)
// ---------------------------------------------------------------------------

/// JSON snapshot of an introspected executable (spec/0009 §3.2).
/// All fields are always present in the materialized JSON; missing data is
/// encoded as `null` / `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableSnapshot {
    pub name: String,
    pub found: bool,
    pub executable: bool,
    pub path: Option<String>,
    pub command_full: Option<String>,
    pub version_full: Option<String>,
    pub version: Option<String>,
}

impl ExecutableSnapshot {
    /// A `found=false` snapshot for a name not on PATH.
    pub fn not_found(name: &str) -> Self {
        ExecutableSnapshot {
            name: name.to_string(),
            found: false,
            executable: false,
            path: None,
            command_full: None,
            version_full: None,
            version: None,
        }
    }
}

/// Test-fixture overrides for pseudo-file evaluation (spec/0009 §2.5, §3.8).
///
/// `env_override`: when `Some`, `<env>` materializes from this map exclusively
/// (no `std::env::vars()` access). Override is total — keys absent are not set.
///
/// `executable_override`: when `Some`, `<executable:NAME>` lookups bypass PATH
/// and subprocess entirely. Override is total — NAMEs absent ⇒ `found=false`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PseudoFileFixture {
    pub env_override: Option<BTreeMap<String, String>>,
    pub executable_override: Option<BTreeMap<String, ExecutableSnapshot>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub id: String,
    pub title: String,
    pub description: String,
    pub remediation: String,
    pub check: Proposition,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlFile {
    pub controls: Vec<Control>,
}

/// Validate a control ID: must match `[A-Z][A-Z0-9-]*`.
pub fn validate_control_id(s: &str) -> Result<()> {
    if s.is_empty() {
        bail!("Control ID must not be empty");
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_uppercase() {
        bail!(
            "Control ID must start with an uppercase letter [A-Z], got {:?}",
            s
        );
    }
    for c in chars {
        if !c.is_ascii_uppercase() && !c.is_ascii_digit() && c != '-' {
            bail!(
                "Control ID must contain only [A-Z0-9-], got invalid character {:?} in {:?}",
                c,
                s
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// TestAst — for `key audit project test` (tests.yaml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TestFile {
    pub test_suites: Vec<TestSuite>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestSuite {
    pub name: String,
    pub description: Option<String>,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestCase {
    pub control_id: String,
    pub description: String,
    pub fixture: String,
    pub expect: TestExpectation,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestExpectation {
    Pass,
    Fail(FailExpectation),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FailExpectation {
    pub count: Option<usize>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFailure {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for RuleFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

#[cfg(test)]
pub const PREDICATE_YAML_KEYS: &[&str] = &[
    "file-exists",
    "text-matches",
    "text-contains",
    "text-has-lines",
    "shell-exports",
    "shell-exports-value-matches",
    "shell-defines",
    "shell-defines-value-matches",
    "shell-adds-to-path",
    "properties-defines-key",
    "xml-matches",
    "json-matches",
    "yaml-matches",
    "all",
    "any",
    "not",
    "conditionally",
];

#[cfg(test)]
pub const PROPOSITION_YAML_KEYS: &[&str] = &[
    "file",
    "forall",
    "exists",
    "all",
    "any",
    "not",
    "conditionally",
];

#[cfg(test)]
pub fn all_predicate_variants() -> Vec<FilePredicateAst> {
    vec![
        FilePredicateAst::FileExists,
        FilePredicateAst::TextMatchesRegex("^test.*".into()),
        FilePredicateAst::TextContains("needle".into()),
        FilePredicateAst::TextHasLines {
            min: Some(1),
            max: Some(100),
        },
        FilePredicateAst::ShellExports("MY_VAR".into()),
        FilePredicateAst::ShellExportsValueMatches {
            name: "MY_VAR".into(),
            value_regex: r"^/usr/.*".into(),
        },
        FilePredicateAst::ShellDefinesVariable("MY_VAR".into()),
        FilePredicateAst::ShellDefinesVariableValueMatches {
            name: "MY_VAR".into(),
            value_regex: r"^/opt/.*".into(),
        },
        FilePredicateAst::ShellAddsToPath("MY_DIR".into()),
        FilePredicateAst::PropertiesDefinesKey("my.key".into()),
        FilePredicateAst::XmlMatchesPath("root/child".into()),
        FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
            "key".into(),
            DataSchema::IsString,
        )])),
        FilePredicateAst::YamlMatches(DataSchema::IsObject(vec![(
            "key".into(),
            DataSchema::IsString,
        )])),
        FilePredicateAst::All(vec![FilePredicateAst::FileExists]),
        FilePredicateAst::Any {
            hint: "try this".into(),
            checks: vec![FilePredicateAst::FileExists],
        },
        FilePredicateAst::Not(Box::new(FilePredicateAst::FileExists)),
        FilePredicateAst::Conditionally {
            condition: Box::new(FilePredicateAst::FileExists),
            then: Box::new(FilePredicateAst::TextMatchesRegex("^test.*".into())),
        },
    ]
}

#[cfg(test)]
pub fn all_data_schema_variants() -> Vec<DataSchema> {
    vec![
        DataSchema::Anything,
        DataSchema::IsString,
        DataSchema::IsStringMatching("^test.*".into()),
        DataSchema::IsNumber,
        DataSchema::IsBool,
        DataSchema::IsTrue,
        DataSchema::IsFalse,
        DataSchema::IsNull,
        DataSchema::IsObject(vec![
            ("name".into(), DataSchema::IsString),
            ("count".into(), DataSchema::IsNumber),
        ]),
        DataSchema::IsArray(DataArrayCheck {
            forall: Some(Box::new(DataSchema::IsString)),
            exists: None,
            at: vec![],
        }),
        DataSchema::IsArray(DataArrayCheck {
            forall: None,
            exists: Some(Box::new(DataSchema::IsNumber)),
            at: vec![(0, DataSchema::IsString)],
        }),
    ]
}

#[cfg(test)]
pub fn all_proposition_variants() -> Vec<Proposition> {
    vec![
        Proposition::FileSatisfies {
            path: SimplePath::new("~/test").unwrap(),
            check: FilePredicateAst::FileExists,
        },
        Proposition::Forall {
            files: vec![SimplePath::new("~/a").unwrap()],
            check: FilePredicateAst::FileExists,
        },
        Proposition::Exists {
            files: vec![SimplePath::new("~/a").unwrap()],
            check: FilePredicateAst::FileExists,
        },
        Proposition::All(vec![Proposition::FileSatisfies {
            path: SimplePath::new("~/test").unwrap(),
            check: FilePredicateAst::FileExists,
        }]),
        Proposition::Any(vec![Proposition::FileSatisfies {
            path: SimplePath::new("~/test").unwrap(),
            check: FilePredicateAst::FileExists,
        }]),
        Proposition::Not(Box::new(Proposition::FileSatisfies {
            path: SimplePath::new("~/test").unwrap(),
            check: FilePredicateAst::FileExists,
        })),
        Proposition::Conditionally {
            condition: Box::new(Proposition::FileSatisfies {
                path: SimplePath::new("~/test").unwrap(),
                check: FilePredicateAst::FileExists,
            }),
            then: Box::new(Proposition::FileSatisfies {
                path: SimplePath::new("~/test").unwrap(),
                check: FilePredicateAst::TextMatchesRegex("^hello".into()),
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Pseudo-file parser tests (spec/0009 §6.1)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod pseudo_path_tests {
    use super::*;

    #[test]
    fn parse_env_pseudo() {
        let p = SimplePath::new("<env>").unwrap();
        assert!(p.is_pseudo());
        assert_eq!(p.pseudo(), Some(&PseudoFile::Env));
        assert_eq!(p.as_str(), "<env>");
    }

    #[test]
    fn parse_executable_with_name() {
        let p = SimplePath::new("<executable:docker>").unwrap();
        assert!(p.is_pseudo());
        assert!(matches!(p.pseudo(), Some(PseudoFile::Executable(n)) if n == "docker"));
        assert_eq!(p.as_str(), "<executable:docker>");
    }

    #[test]
    fn parse_executable_hyphen_name() {
        let p = SimplePath::new("<executable:git-lfs>").unwrap();
        assert!(matches!(p.pseudo(), Some(PseudoFile::Executable(n)) if n == "git-lfs"));
    }

    #[test]
    fn parse_executable_weird_name_chars() {
        let p = SimplePath::new("<executable:weird.name+1>").unwrap();
        assert!(matches!(p.pseudo(), Some(PseudoFile::Executable(n)) if n == "weird.name+1"));
    }

    #[test]
    fn reject_empty_pseudo() {
        let err = SimplePath::new("<>").unwrap_err();
        assert!(format!("{:#}", err).contains("empty"));
    }

    #[test]
    fn reject_unterminated_pseudo() {
        let err = SimplePath::new("<env").unwrap_err();
        assert!(format!("{:#}", err).contains("'>'"));
    }

    #[test]
    fn reject_env_with_arg() {
        let err = SimplePath::new("<env:json>").unwrap_err();
        assert!(format!("{:#}", err).contains("env"));
    }

    #[test]
    fn reject_executable_no_name() {
        let err = SimplePath::new("<executable:>").unwrap_err();
        assert!(format!("{:#}", err).contains("non-empty"));
    }

    #[test]
    fn reject_executable_space_in_name() {
        let err = SimplePath::new("<executable:has space>").unwrap_err();
        assert!(format!("{:#}", err).contains("[A-Za-z0-9_.+-]"));
    }

    #[test]
    fn reject_executable_absolute_path() {
        let err = SimplePath::new("<executable:/abs>").unwrap_err();
        assert!(format!("{:#}", err).contains("absolute"));
    }

    #[test]
    fn reject_unknown_pseudo_keyword() {
        let err = SimplePath::new("<process-list>").unwrap_err();
        assert!(format!("{:#}", err).contains("unknown pseudo-file keyword"));
    }

    #[test]
    fn concrete_path_still_works() {
        let p = SimplePath::new("~/.bashrc").unwrap();
        assert!(!p.is_pseudo());
        assert_eq!(p.as_str(), "~/.bashrc");
    }

    #[test]
    fn reject_invalid_concrete() {
        assert!(SimplePath::new("/etc/passwd").is_err());
        assert!(SimplePath::new("~/.././x").is_err());
    }
}

#[cfg(test)]
pub const TEST_EXPECTATION_YAML_KEYS: &[&str] = &["pass", "fail"];

#[cfg(test)]
pub fn all_test_expectation_variants() -> Vec<TestExpectation> {
    vec![
        TestExpectation::Pass,
        TestExpectation::Fail(FailExpectation {
            count: Some(2),
            messages: vec!["does not exist".into()],
        }),
    ]
}
