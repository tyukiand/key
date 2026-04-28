use std::path::PathBuf;

use regex::Regex;

use crate::rules::ast::{FilePredicateAst, PseudoFile};
use crate::rules::pseudo::{inapplicable_predicate_message, PseudoKind, PseudoSnapshot};
use crate::rules::queries;

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
        // Special case (spec §2.3): on `<env>`, `shell-adds-to-path: SEG`
        // checks whether $PATH contains SEG as a colon-separated segment, NOT
        // a regex match against export lines.
        if let FilePredicateAst::ShellAddsToPath(seg) = pred {
            if let Subject::Pseudo(pseudo, snap) = subject {
                if matches!(pseudo, PseudoFile::Env) {
                    return env_path_contains_segment(snap, seg);
                }
            }
        }
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
            Subject::Pseudo(pseudo, snap) => match (pseudo, &snap.kind) {
                // <env> is always present (§2.3)
                (PseudoFile::Env, _) => Ok(()),
                // <executable:NAME>: file-exists ↔ found=true (§3.5)
                (PseudoFile::Executable(_), PseudoKind::Executable { snapshot }) => {
                    if snapshot.found {
                        Ok(())
                    } else {
                        Err(format!("executable {:?} not found on PATH", snapshot.name))
                    }
                }
                _ => Ok(()),
            },
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
    }
}

fn read_subject_text(subject: &Subject<'_>) -> Result<String, String> {
    match subject {
        Subject::Concrete(p) => {
            if !p.exists() {
                return Err(format!("file does not exist: {}", p.display()));
            }
            std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {}", p.display(), e))
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
        (PseudoFile::Executable(_), FilePredicateAst::ShellExports(_)) => {
            Some(inapplicable_predicate_message("shell-exports", pseudo))
        }
        (PseudoFile::Executable(_), FilePredicateAst::ShellDefinesVariable(_)) => {
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

/// `<env>` shell-adds-to-path: split $PATH on ':' and check membership.
fn env_path_contains_segment(snap: &PseudoSnapshot, segment: &str) -> Result<(), String> {
    if let PseudoKind::Env { env_map } = &snap.kind {
        if let Some(path_val) = env_map.get("PATH") {
            for seg in path_val.split(':') {
                if seg == segment {
                    return Ok(());
                }
            }
        }
        Err(format!(
            "PATH does not contain segment {:?} in <env>",
            segment
        ))
    } else {
        // Should not happen given caller guard.
        Err("internal error: env_path_contains_segment called on non-env pseudo".into())
    }
}
