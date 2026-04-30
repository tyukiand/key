use std::path::PathBuf;

use regex::Regex;

use crate::rules::ast::{FilePredicateAst, PseudoFile};
use crate::rules::pseudo::{inapplicable_predicate_message, PseudoSnapshot};
use crate::rules::queries;

fn strip_shell_quotes(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

fn evaluate_shell_value_matches(
    subject: &Subject<'_>,
    name: &str,
    value_regex: &str,
    require_export: bool,
) -> Result<(), String> {
    let content = read_subject_text(subject)?;
    let line_pattern = if require_export {
        format!(r"^\s*export\s+{}=(.*)", regex::escape(name))
    } else {
        format!(r"^\s*(?:export\s+)?{}=(.*)", regex::escape(name))
    };
    let line_re = Regex::new(&line_pattern)
        .map_err(|e| format!("invalid regex {:?}: {}", line_pattern, e))?;
    let value_re =
        Regex::new(value_regex).map_err(|e| format!("invalid regex {:?}: {}", value_regex, e))?;
    let mut last_rhs: Option<String> = None;
    for line in content.lines() {
        if let Some(caps) = line_re.captures(line) {
            let raw_rhs = caps.get(1).unwrap().as_str();
            let rhs = strip_shell_quotes(raw_rhs);
            if value_re.is_match(rhs) {
                return Ok(());
            }
            last_rhs = Some(rhs.to_string());
        }
    }
    let key = if require_export {
        "shell-exports"
    } else {
        "shell-defines"
    };
    match last_rhs {
        Some(rhs) => Err(format!(
            "{}: variable {:?} found but rhs {:?} does not match regex {:?} in {}",
            key,
            name,
            rhs,
            value_regex,
            subject.path_for_display()
        )),
        None => Err(format!(
            "{}: no `{} {}=...` line found in {}",
            key,
            if require_export {
                "export"
            } else {
                "(export?)"
            },
            name,
            subject.path_for_display()
        )),
    }
}

/// A predicate's evaluation subject: either a concrete file on disk or a
/// pseudo-file snapshot (already resolved + cached by the EvalContext).
#[derive(Clone)]
pub enum Subject<'a> {
    Concrete(PathBuf),
    Pseudo(&'a PseudoFile, &'a PseudoSnapshot),
}

impl<'a> Subject<'a> {
    fn path_for_display(&self) -> String {
        match self {
            Subject::Concrete(p) => p.display().to_string(),
            Subject::Pseudo(p, _) => p.as_token(),
        }
    }
}

/// Desugar a predicate into a more primitive form for evaluation.
/// Returns `None` if the predicate is already primitive.
pub fn desugar(pred: &FilePredicateAst) -> Option<FilePredicateAst> {
    match pred {
        FilePredicateAst::ShellExports(var) => {
            let pattern = format!(r"^\s*export\s+{}=.*", regex::escape(var));
            Some(FilePredicateAst::TextMatchesRegex(pattern))
        }
        FilePredicateAst::ShellDefinesVariable(var) => {
            let pattern = format!(r"^\s*(export\s+)?{}=.*", regex::escape(var));
            Some(FilePredicateAst::TextMatchesRegex(pattern))
        }
        FilePredicateAst::ShellAddsToPath(var) => {
            let pattern = format!(r#"^\s*export\s+PATH="?\${}:\$PATH"?"#, regex::escape(var));
            Some(FilePredicateAst::TextMatchesRegex(pattern))
        }
        FilePredicateAst::ShellExportsValueMatches { .. } => None,
        FilePredicateAst::ShellDefinesVariableValueMatches { .. } => None,
        FilePredicateAst::FileExists => None,
        FilePredicateAst::TextMatchesRegex(_) => None,
        FilePredicateAst::TextContains(_) => None,
        FilePredicateAst::TextHasLines { .. } => None,
        FilePredicateAst::PropertiesDefinesKey(_) => None,
        FilePredicateAst::XmlMatchesPath(_) => None,
        FilePredicateAst::JsonMatches(_) => None,
        FilePredicateAst::YamlMatches(_) => None,
        FilePredicateAst::All(_) => None,
        FilePredicateAst::Any { .. } => None,
        FilePredicateAst::Not(_) => None,
        FilePredicateAst::Conditionally { .. } => None,
    }
}

/// Evaluate a predicate against a subject (concrete file or pseudo-file snapshot).
pub fn evaluate_predicate_subject(
    pred: &FilePredicateAst,
    subject: &Subject<'_>,
) -> Result<(), String> {
    // Pseudo-file inapplicability checks (spec §2.4 / §3.6 + §4.1) come BEFORE
    // desugaring: e.g. shell-* desugars to text-matches, but on `<executable>`
    // shell-* must error explicitly.
    if let Subject::Pseudo(pseudo, _) = subject {
        if let Some(msg) = pseudo_inapplicable_check(pred, pseudo) {
            return Err(msg);
        }
    }

    if let Some(desugared) = desugar(pred) {
        return evaluate_predicate_subject(&desugared, subject);
    }

    match pred {
        FilePredicateAst::FileExists => match subject {
            Subject::Concrete(p) => {
                if p.exists() {
                    Ok(())
                } else {
                    Err(format!("file does not exist: {}", p.display()))
                }
            }
            Subject::Pseudo(_, _) => {
                // Spec/0013 §A.7 — `file-exists` on every pseudo-file is constant
                // TRUE. The pseudo-file's virtual content is always materialized
                // (env reads `std::env::vars()`; executable builds a snapshot
                // whose `.found` field reflects PATH lookup but the snapshot
                // itself always exists). To test whether a program is on PATH,
                // use `json-matches: { path: $.found, equals: true }` or
                // `is-true` on the `.found` field.
                Ok(())
            }
        },
        FilePredicateAst::TextMatchesRegex(pattern) => {
            let content = read_subject_text(subject)?;
            let re =
                Regex::new(pattern).map_err(|e| format!("invalid regex {:?}: {}", pattern, e))?;
            for line in content.lines() {
                if re.is_match(line) {
                    return Ok(());
                }
            }
            Err(format!(
                "no line matches regex {:?} in {}",
                pattern,
                subject.path_for_display()
            ))
        }
        FilePredicateAst::TextContains(needle) => {
            let content = read_subject_text(subject)?;
            for line in content.lines() {
                if line.contains(needle.as_str()) {
                    return Ok(());
                }
            }
            Err(format!(
                "no line contains {:?} in {}",
                needle,
                subject.path_for_display()
            ))
        }
        FilePredicateAst::TextHasLines { min, max } => {
            let content = read_subject_text(subject)?;
            let count = content.lines().count() as u32;
            if let Some(lo) = min {
                if count < *lo {
                    return Err(format!(
                        "file has {} lines, expected at least {} in {}",
                        count,
                        lo,
                        subject.path_for_display()
                    ));
                }
            }
            if let Some(hi) = max {
                if count > *hi {
                    return Err(format!(
                        "file has {} lines, expected at most {} in {}",
                        count,
                        hi,
                        subject.path_for_display()
                    ));
                }
            }
            Ok(())
        }
        FilePredicateAst::PropertiesDefinesKey(key) => {
            let content = read_subject_text(subject)?;
            let prefix = format!("{}=", key);
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with(&prefix) {
                    return Ok(());
                }
            }
            Err(format!(
                "no line defines key {:?} in {}",
                key,
                subject.path_for_display()
            ))
        }
        FilePredicateAst::XmlMatchesPath(path) => {
            let content = read_subject_text(subject)?;
            match queries::xml_has_path(&content, path) {
                Ok(true) => Ok(()),
                Ok(false) => Err(format!(
                    "XML path {:?} not found in {}",
                    path,
                    subject.path_for_display()
                )),
                Err(e) => Err(format!(
                    "XML parse error in {}: {}",
                    subject.path_for_display(),
                    e
                )),
            }
        }
        FilePredicateAst::JsonMatches(schema) => {
            let content = read_subject_text(subject)?;
            queries::evaluate_data_schema_json_str(schema, &content).map_err(|e| {
                format!(
                    "JSON schema mismatch in {}: {}",
                    subject.path_for_display(),
                    e
                )
            })
        }
        FilePredicateAst::YamlMatches(schema) => {
            let content = read_subject_text(subject)?;
            queries::evaluate_data_schema_yaml_str(schema, &content).map_err(|e| {
                format!(
                    "YAML schema mismatch in {}: {}",
                    subject.path_for_display(),
                    e
                )
            })
        }
        FilePredicateAst::All(preds) => {
            for p in preds {
                evaluate_predicate_subject(p, subject)?;
            }
            Ok(())
        }
        FilePredicateAst::Any { hint, checks } => {
            let mut errors = Vec::new();
            for c in checks {
                match evaluate_predicate_subject(c, subject) {
                    Ok(()) => return Ok(()),
                    Err(e) => errors.push(e),
                }
            }
            Err(format!(
                "none of the alternatives matched (hint: {}): [{}]",
                hint,
                errors.join("; ")
            ))
        }
        FilePredicateAst::Not(inner) => match evaluate_predicate_subject(inner, subject) {
            Ok(()) => Err(format!(
                "expected check to fail but it passed in {}",
                subject.path_for_display()
            )),
            Err(_) => Ok(()),
        },
        FilePredicateAst::Conditionally { condition, then } => {
            match evaluate_predicate_subject(condition, subject) {
                Err(_) => Ok(()), // condition not met → vacuously true
                Ok(()) => evaluate_predicate_subject(then, subject),
            }
        }
        FilePredicateAst::ShellExports(_)
        | FilePredicateAst::ShellDefinesVariable(_)
        | FilePredicateAst::ShellAddsToPath(_) => {
            unreachable!("should have been desugared")
        }
        FilePredicateAst::ShellExportsValueMatches { name, value_regex } => {
            evaluate_shell_value_matches(subject, name, value_regex, true)
        }
        FilePredicateAst::ShellDefinesVariableValueMatches { name, value_regex } => {
            evaluate_shell_value_matches(subject, name, value_regex, false)
        }
    }
}

fn read_subject_text(subject: &Subject<'_>) -> Result<String, String> {
    use crate::effects::{OsEffectsRo, RealOsEffects};
    match subject {
        Subject::Concrete(p) => {
            let os = RealOsEffects;
            if !os.path_exists(p) {
                return Err(format!("file does not exist: {}", p.display()));
            }
            os.read_to_string(p)
                .map_err(|e| format!("cannot read {}: {}", p.display(), e))
        }
        Subject::Pseudo(_, snap) => Ok(snap.body.clone()),
    }
}

/// Check whether a predicate is inapplicable to a pseudo-file (spec §2.4 / §3.6).
/// Returns `Some(message)` if so, `None` if applicable. The message format
/// matches §4.1.
fn pseudo_inapplicable_check(pred: &FilePredicateAst, pseudo: &PseudoFile) -> Option<String> {
    match (pseudo, pred) {
        // <env> §2.4
        (PseudoFile::Env, FilePredicateAst::PropertiesDefinesKey(_)) => Some(
            inapplicable_predicate_message("properties-defines-key", pseudo),
        ),
        (PseudoFile::Env, FilePredicateAst::XmlMatchesPath(_)) => {
            Some(inapplicable_predicate_message("xml-matches", pseudo))
        }
        (PseudoFile::Env, FilePredicateAst::JsonMatches(_)) => {
            Some(inapplicable_predicate_message("json-matches", pseudo))
        }
        (PseudoFile::Env, FilePredicateAst::YamlMatches(_)) => {
            Some(inapplicable_predicate_message("yaml-matches", pseudo))
        }
        // <executable:NAME> §3.6
        (PseudoFile::Executable(_), FilePredicateAst::ShellExports(_))
        | (PseudoFile::Executable(_), FilePredicateAst::ShellExportsValueMatches { .. }) => {
            Some(inapplicable_predicate_message("shell-exports", pseudo))
        }
        (PseudoFile::Executable(_), FilePredicateAst::ShellDefinesVariable(_))
        | (PseudoFile::Executable(_), FilePredicateAst::ShellDefinesVariableValueMatches { .. }) => {
            Some(inapplicable_predicate_message("shell-defines", pseudo))
        }
        (PseudoFile::Executable(_), FilePredicateAst::ShellAddsToPath(_)) => {
            Some(inapplicable_predicate_message("shell-adds-to-path", pseudo))
        }
        (PseudoFile::Executable(_), FilePredicateAst::PropertiesDefinesKey(_)) => Some(
            inapplicable_predicate_message("properties-defines-key", pseudo),
        ),
        (PseudoFile::Executable(_), FilePredicateAst::XmlMatchesPath(_)) => {
            Some(inapplicable_predicate_message("xml-matches", pseudo))
        }
        (PseudoFile::Executable(_), FilePredicateAst::YamlMatches(_)) => {
            Some(inapplicable_predicate_message("yaml-matches", pseudo))
        }
        (PseudoFile::Executable(_), FilePredicateAst::TextHasLines { .. }) => {
            Some(inapplicable_predicate_message("text-has-lines", pseudo))
        }
        _ => None,
    }
}
