use anyhow::{bail, Result};
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

/// A validated path that starts with `~/` and contains no `.`, `..`, or `//` segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimplePath(String);

impl SimplePath {
    pub fn new(s: &str) -> Result<Self> {
        if !s.starts_with("~/") && s != "~" {
            bail!("SimplePath must start with '~/': got {:?}", s);
        }
        if s.contains("/./") || s.contains("/../") || s.contains("//") {
            bail!("SimplePath must not contain /./ or /../ or //: got {:?}", s);
        }
        Ok(SimplePath(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn resolve(&self, home_dir: &Path) -> PathBuf {
        if self.0 == "~" {
            home_dir.to_path_buf()
        } else {
            home_dir.join(&self.0[2..])
        }
    }
}

impl std::fmt::Display for SimplePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilePredicateAst {
    FileExists,
    TextMatchesRegex(String),
    TextHasLines {
        min: Option<u32>,
        max: Option<u32>,
    },
    ShellExports(String),
    ShellDefinesVariable(String),
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
}

#[cfg(test)]
impl FilePredicateAst {
    pub fn yaml_key(&self) -> &'static str {
        match self {
            FilePredicateAst::FileExists => "file-exists",
            FilePredicateAst::TextMatchesRegex(_) => "text-matches",
            FilePredicateAst::TextHasLines { .. } => "text-has-lines",
            FilePredicateAst::ShellExports(_) => "shell-exports",
            FilePredicateAst::ShellDefinesVariable(_) => "shell-defines",
            FilePredicateAst::ShellAddsToPath(_) => "shell-adds-to-path",
            FilePredicateAst::PropertiesDefinesKey(_) => "properties-defines-key",
            FilePredicateAst::XmlMatchesPath(_) => "xml-matches",
            FilePredicateAst::JsonMatches(_) => "json-matches",
            FilePredicateAst::YamlMatches(_) => "yaml-matches",
            FilePredicateAst::All(_) => "all",
            FilePredicateAst::Any { .. } => "any",
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
        }
    }
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
    "text-has-lines",
    "shell-exports",
    "shell-defines",
    "shell-adds-to-path",
    "properties-defines-key",
    "xml-matches",
    "json-matches",
    "yaml-matches",
    "all",
    "any",
];

#[cfg(test)]
pub const PROPOSITION_YAML_KEYS: &[&str] = &["file", "forall", "exists", "all", "any"];

#[cfg(test)]
pub fn all_predicate_variants() -> Vec<FilePredicateAst> {
    vec![
        FilePredicateAst::FileExists,
        FilePredicateAst::TextMatchesRegex("^test.*".into()),
        FilePredicateAst::TextHasLines {
            min: Some(1),
            max: Some(100),
        },
        FilePredicateAst::ShellExports("MY_VAR".into()),
        FilePredicateAst::ShellDefinesVariable("MY_VAR".into()),
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
    ]
}
