use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

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
    JsonMatchesQuery(String),
    YamlMatchesQuery(String),
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
            FilePredicateAst::JsonMatchesQuery(_) => "json-matches",
            FilePredicateAst::YamlMatchesQuery(_) => "yaml-matches",
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
        FilePredicateAst::JsonMatchesQuery(".key.sub".into()),
        FilePredicateAst::YamlMatchesQuery(".key.sub".into()),
        FilePredicateAst::All(vec![FilePredicateAst::FileExists]),
        FilePredicateAst::Any {
            hint: "try this".into(),
            checks: vec![FilePredicateAst::FileExists],
        },
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
