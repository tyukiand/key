use crate::rules::ast::FilePredicateAst;
use crate::rules::queries;
use regex::Regex;
use std::path::Path;

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
        FilePredicateAst::TextHasLines { .. } => None,
        FilePredicateAst::PropertiesDefinesKey(_) => None,
        FilePredicateAst::XmlMatchesPath(_) => None,
        FilePredicateAst::JsonMatchesQuery(_) => None,
        FilePredicateAst::YamlMatchesQuery(_) => None,
        FilePredicateAst::All(_) => None,
        FilePredicateAst::Any { .. } => None,
    }
}

/// Evaluate a file predicate against a concrete file path.
/// Returns `Ok(())` on success, `Err(message)` on failure.
pub fn evaluate_predicate(pred: &FilePredicateAst, file_path: &Path) -> Result<(), String> {
    // If it desugars, evaluate the desugared form instead
    if let Some(desugared) = desugar(pred) {
        return evaluate_predicate(&desugared, file_path);
    }

    match pred {
        FilePredicateAst::FileExists => {
            if file_path.exists() {
                Ok(())
            } else {
                Err(format!("file does not exist: {}", file_path.display()))
            }
        }
        FilePredicateAst::TextMatchesRegex(pattern) => {
            let content = read_file_text(file_path)?;
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
                file_path.display()
            ))
        }
        FilePredicateAst::TextHasLines { min, max } => {
            let content = read_file_text(file_path)?;
            let count = content.lines().count() as u32;
            if let Some(lo) = min {
                if count < *lo {
                    return Err(format!(
                        "file has {} lines, expected at least {} in {}",
                        count,
                        lo,
                        file_path.display()
                    ));
                }
            }
            if let Some(hi) = max {
                if count > *hi {
                    return Err(format!(
                        "file has {} lines, expected at most {} in {}",
                        count,
                        hi,
                        file_path.display()
                    ));
                }
            }
            Ok(())
        }
        FilePredicateAst::PropertiesDefinesKey(key) => {
            let content = read_file_text(file_path)?;
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
                file_path.display()
            ))
        }
        FilePredicateAst::XmlMatchesPath(path) => {
            let content = read_file_text(file_path)?;
            match queries::xml_has_path(&content, path) {
                Ok(true) => Ok(()),
                Ok(false) => Err(format!(
                    "XML path {:?} not found in {}",
                    path,
                    file_path.display()
                )),
                Err(e) => Err(format!("XML parse error in {}: {}", file_path.display(), e)),
            }
        }
        FilePredicateAst::JsonMatchesQuery(query) => {
            let content = read_file_text(file_path)?;
            match queries::json_has_query(&content, query) {
                Ok(true) => Ok(()),
                Ok(false) => Err(format!(
                    "JSON query {:?} not found in {}",
                    query,
                    file_path.display()
                )),
                Err(e) => Err(format!(
                    "JSON parse error in {}: {}",
                    file_path.display(),
                    e
                )),
            }
        }
        FilePredicateAst::YamlMatchesQuery(query) => {
            let content = read_file_text(file_path)?;
            match queries::yaml_has_query(&content, query) {
                Ok(true) => Ok(()),
                Ok(false) => Err(format!(
                    "YAML query {:?} not found in {}",
                    query,
                    file_path.display()
                )),
                Err(e) => Err(format!(
                    "YAML parse error in {}: {}",
                    file_path.display(),
                    e
                )),
            }
        }
        FilePredicateAst::All(preds) => {
            for p in preds {
                evaluate_predicate(p, file_path)?;
            }
            Ok(())
        }
        FilePredicateAst::Any { hint, checks } => {
            let mut errors = Vec::new();
            for c in checks {
                match evaluate_predicate(c, file_path) {
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
        // Desugaring variants are handled above via desugar()
        FilePredicateAst::ShellExports(_)
        | FilePredicateAst::ShellDefinesVariable(_)
        | FilePredicateAst::ShellAddsToPath(_) => {
            unreachable!("should have been desugared")
        }
    }
}

fn read_file_text(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("file does not exist: {}", path.display()));
    }
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))
}
