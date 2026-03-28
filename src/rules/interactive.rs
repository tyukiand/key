use anyhow::{bail, Result};

use crate::effects::Effects;
use crate::rules::ast::{FilePredicateAst, Proposition, SimplePath};

/// Response from an answerer to a question.
pub enum Answer {
    Text(String),
    EndOfInput,
}

/// Trait for getting answers to interactive questions.
pub trait Answerer {
    fn ask(&mut self, question: &str) -> Answer;
}

/// Production answerer that reads from the Effects trait.
struct EffectsAnswerer<'a> {
    fx: &'a dyn Effects,
}

impl<'a> Answerer for EffectsAnswerer<'a> {
    fn ask(&mut self, question: &str) -> Answer {
        match self.fx.prompt_text(question) {
            Ok(s) => Answer::Text(s),
            Err(_) => Answer::EndOfInput,
        }
    }
}

/// Test answerer that feeds pre-canned strings.
#[cfg(test)]
pub struct CannedAnswerer {
    answers: Vec<String>,
    pos: usize,
}

#[cfg(test)]
impl CannedAnswerer {
    pub fn new(answers: Vec<String>) -> Self {
        CannedAnswerer { answers, pos: 0 }
    }
}

#[cfg(test)]
impl Answerer for CannedAnswerer {
    fn ask(&mut self, _question: &str) -> Answer {
        if self.pos < self.answers.len() {
            let answer = self.answers[self.pos].clone();
            self.pos += 1;
            Answer::Text(answer)
        } else {
            Answer::EndOfInput
        }
    }
}

fn ask_select(answerer: &mut impl Answerer, prompt: &str, options: &[&str]) -> Result<usize> {
    let mut question = format!("{}:\n", prompt);
    for (i, opt) in options.iter().enumerate() {
        question.push_str(&format!("  {}: {}\n", i + 1, opt));
    }
    question.push_str("Choose");

    loop {
        match answerer.ask(&question) {
            Answer::Text(s) => {
                if let Ok(n) = s.trim().parse::<usize>() {
                    if n >= 1 && n <= options.len() {
                        return Ok(n - 1);
                    }
                }
                // retry with shorter prompt
                question = format!("Invalid choice. Enter 1-{}", options.len());
            }
            Answer::EndOfInput => bail!("Unexpected end of input"),
        }
    }
}

fn ask_string(answerer: &mut impl Answerer, prompt: &str) -> Result<String> {
    match answerer.ask(prompt) {
        Answer::Text(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                bail!("Empty input is not allowed");
            }
            Ok(trimmed)
        }
        Answer::EndOfInput => bail!("Unexpected end of input"),
    }
}

fn ask_yes_no(answerer: &mut impl Answerer, prompt: &str) -> Result<bool> {
    loop {
        match answerer.ask(prompt) {
            Answer::Text(s) => match s.trim().to_lowercase().as_str() {
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => {} // retry
            },
            Answer::EndOfInput => bail!("Unexpected end of input"),
        }
    }
}

fn ask_path(answerer: &mut impl Answerer, prompt: &str) -> Result<SimplePath> {
    let s = ask_string(answerer, prompt)?;
    SimplePath::new(&s)
}

fn ask_path_list(answerer: &mut impl Answerer) -> Result<Vec<SimplePath>> {
    let mut paths = Vec::new();
    loop {
        let p = ask_path(answerer, "File path (~/...)")?;
        paths.push(p);
        if !ask_yes_no(answerer, "Add another file? (y/n)")? {
            break;
        }
    }
    Ok(paths)
}

/// Build a FilePredicateAst interactively.
pub fn build_predicate(answerer: &mut impl Answerer) -> Result<FilePredicateAst> {
    let variants = predicate_menu_items();
    let idx = ask_select(answerer, "What kind of check?", &variants)?;

    match idx {
        0 => Ok(FilePredicateAst::FileExists),
        1 => {
            let re = ask_string(answerer, "Regex pattern")?;
            Ok(FilePredicateAst::TextMatchesRegex(re))
        }
        2 => {
            let min_s = ask_string(answerer, "Min lines (or 'none')")?;
            let min = if min_s == "none" {
                None
            } else {
                Some(min_s.parse::<u32>().map_err(|e| anyhow::anyhow!("{}", e))?)
            };
            let max_s = ask_string(answerer, "Max lines (or 'none')")?;
            let max = if max_s == "none" {
                None
            } else {
                Some(max_s.parse::<u32>().map_err(|e| anyhow::anyhow!("{}", e))?)
            };
            Ok(FilePredicateAst::TextHasLines { min, max })
        }
        3 => {
            let var = ask_string(answerer, "Variable name")?;
            Ok(FilePredicateAst::ShellExports(var))
        }
        4 => {
            let var = ask_string(answerer, "Variable name")?;
            Ok(FilePredicateAst::ShellDefinesVariable(var))
        }
        5 => {
            let var = ask_string(answerer, "Variable name")?;
            Ok(FilePredicateAst::ShellAddsToPath(var))
        }
        6 => {
            let key = ask_string(answerer, "Property key")?;
            Ok(FilePredicateAst::PropertiesDefinesKey(key))
        }
        7 => {
            let path = ask_string(answerer, "XML element path (e.g. settings/servers/server)")?;
            Ok(FilePredicateAst::XmlMatchesPath(path))
        }
        8 => {
            let q = ask_string(answerer, "JSON query (e.g. .user.name)")?;
            Ok(FilePredicateAst::JsonMatchesQuery(q))
        }
        9 => {
            let q = ask_string(answerer, "YAML query (e.g. .config.key)")?;
            Ok(FilePredicateAst::YamlMatchesQuery(q))
        }
        10 => {
            let mut preds = Vec::new();
            loop {
                preds.push(build_predicate(answerer)?);
                if !ask_yes_no(answerer, "Add another check? (y/n)")? {
                    break;
                }
            }
            Ok(FilePredicateAst::All(preds))
        }
        11 => {
            let hint = ask_string(answerer, "Hint for user when all alternatives fail")?;
            let mut checks = Vec::new();
            loop {
                checks.push(build_predicate(answerer)?);
                if !ask_yes_no(answerer, "Add another alternative? (y/n)")? {
                    break;
                }
            }
            Ok(FilePredicateAst::Any { hint, checks })
        }
        _ => bail!("Invalid selection"),
    }
}

/// Build a Proposition interactively.
pub fn build_proposition(answerer: &mut impl Answerer) -> Result<Proposition> {
    let variants = proposition_menu_items();
    let idx = ask_select(answerer, "What kind of rule?", &variants)?;

    match idx {
        0 => {
            let path = ask_path(answerer, "File path (~/...)")?;
            let check = build_predicate(answerer)?;
            Ok(Proposition::FileSatisfies { path, check })
        }
        1 => {
            let files = ask_path_list(answerer)?;
            let check = build_predicate(answerer)?;
            Ok(Proposition::Forall { files, check })
        }
        2 => {
            let files = ask_path_list(answerer)?;
            let check = build_predicate(answerer)?;
            Ok(Proposition::Exists { files, check })
        }
        3 => {
            let mut props = Vec::new();
            loop {
                props.push(build_proposition(answerer)?);
                if !ask_yes_no(answerer, "Add another rule? (y/n)")? {
                    break;
                }
            }
            Ok(Proposition::All(props))
        }
        4 => {
            let mut props = Vec::new();
            loop {
                props.push(build_proposition(answerer)?);
                if !ask_yes_no(answerer, "Add another alternative? (y/n)")? {
                    break;
                }
            }
            Ok(Proposition::Any(props))
        }
        _ => bail!("Invalid selection"),
    }
}

/// Menu items derived from all predicate variants.
fn predicate_menu_items() -> Vec<&'static str> {
    // This list must stay in sync with all_predicate_variants() — the
    // roundtrip test (predicate_to_answers → build_predicate) will fail
    // if they diverge.
    vec![
        "file-exists",
        "text-matches (regex)",
        "text-has-lines (min/max)",
        "shell-exports (variable)",
        "shell-defines (variable)",
        "shell-adds-to-path (variable)",
        "properties-defines-key",
        "xml-matches (element path)",
        "json-matches (query)",
        "yaml-matches (query)",
        "all (multiple checks)",
        "any (alternatives with hint)",
    ]
}

/// Menu items derived from all proposition variants.
fn proposition_menu_items() -> Vec<&'static str> {
    vec![
        "file (single file check)",
        "forall (all files must satisfy)",
        "exists (at least one file satisfies)",
        "all (multiple rules)",
        "any (any rule suffices)",
    ]
}

/// Convert a predicate into the sequence of answers needed to reconstruct it.
#[cfg(test)]
pub fn predicate_to_answers(pred: &FilePredicateAst) -> Vec<String> {
    let mut answers = Vec::new();
    predicate_to_answers_inner(pred, &mut answers);
    answers
}

#[cfg(test)]
fn predicate_to_answers_inner(pred: &FilePredicateAst, answers: &mut Vec<String>) {
    match pred {
        FilePredicateAst::FileExists => {
            answers.push("1".into()); // menu index
        }
        FilePredicateAst::TextMatchesRegex(re) => {
            answers.push("2".into());
            answers.push(re.clone());
        }
        FilePredicateAst::TextHasLines { min, max } => {
            answers.push("3".into());
            answers.push(min.map(|n| n.to_string()).unwrap_or_else(|| "none".into()));
            answers.push(max.map(|n| n.to_string()).unwrap_or_else(|| "none".into()));
        }
        FilePredicateAst::ShellExports(var) => {
            answers.push("4".into());
            answers.push(var.clone());
        }
        FilePredicateAst::ShellDefinesVariable(var) => {
            answers.push("5".into());
            answers.push(var.clone());
        }
        FilePredicateAst::ShellAddsToPath(var) => {
            answers.push("6".into());
            answers.push(var.clone());
        }
        FilePredicateAst::PropertiesDefinesKey(key) => {
            answers.push("7".into());
            answers.push(key.clone());
        }
        FilePredicateAst::XmlMatchesPath(path) => {
            answers.push("8".into());
            answers.push(path.clone());
        }
        FilePredicateAst::JsonMatchesQuery(q) => {
            answers.push("9".into());
            answers.push(q.clone());
        }
        FilePredicateAst::YamlMatchesQuery(q) => {
            answers.push("10".into());
            answers.push(q.clone());
        }
        FilePredicateAst::All(preds) => {
            answers.push("11".into());
            for (i, p) in preds.iter().enumerate() {
                predicate_to_answers_inner(p, answers);
                if i < preds.len() - 1 {
                    answers.push("y".into()); // add another
                } else {
                    answers.push("n".into()); // done
                }
            }
        }
        FilePredicateAst::Any { hint, checks } => {
            answers.push("12".into());
            answers.push(hint.clone());
            for (i, c) in checks.iter().enumerate() {
                predicate_to_answers_inner(c, answers);
                if i < checks.len() - 1 {
                    answers.push("y".into());
                } else {
                    answers.push("n".into());
                }
            }
        }
    }
}

/// Convert a proposition into the sequence of answers needed to reconstruct it.
#[cfg(test)]
pub fn proposition_to_answers(prop: &Proposition) -> Vec<String> {
    let mut answers = Vec::new();
    proposition_to_answers_inner(prop, &mut answers);
    answers
}

#[cfg(test)]
fn proposition_to_answers_inner(prop: &Proposition, answers: &mut Vec<String>) {
    match prop {
        Proposition::FileSatisfies { path, check } => {
            answers.push("1".into());
            answers.push(path.as_str().into());
            predicate_to_answers_inner(check, answers);
        }
        Proposition::Forall { files, check } => {
            answers.push("2".into());
            for (i, f) in files.iter().enumerate() {
                answers.push(f.as_str().into());
                if i < files.len() - 1 {
                    answers.push("y".into());
                } else {
                    answers.push("n".into());
                }
            }
            predicate_to_answers_inner(check, answers);
        }
        Proposition::Exists { files, check } => {
            answers.push("3".into());
            for (i, f) in files.iter().enumerate() {
                answers.push(f.as_str().into());
                if i < files.len() - 1 {
                    answers.push("y".into());
                } else {
                    answers.push("n".into());
                }
            }
            predicate_to_answers_inner(check, answers);
        }
        Proposition::All(props) => {
            answers.push("4".into());
            for (i, p) in props.iter().enumerate() {
                proposition_to_answers_inner(p, answers);
                if i < props.len() - 1 {
                    answers.push("y".into());
                } else {
                    answers.push("n".into());
                }
            }
        }
        Proposition::Any(props) => {
            answers.push("5".into());
            for (i, p) in props.iter().enumerate() {
                proposition_to_answers_inner(p, answers);
                if i < props.len() - 1 {
                    answers.push("y".into());
                } else {
                    answers.push("n".into());
                }
            }
        }
    }
}

/// Entry point for the `key rules add` command.
pub fn run_interactive_add(fx: &dyn Effects) -> Result<Proposition> {
    let mut answerer = EffectsAnswerer { fx };
    build_proposition(&mut answerer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::{all_predicate_variants, all_proposition_variants};

    #[test]
    fn roundtrip_all_predicates() {
        for pred in all_predicate_variants() {
            let answers = predicate_to_answers(&pred);
            let mut answerer = CannedAnswerer::new(answers.clone());
            let rebuilt = build_predicate(&mut answerer).unwrap_or_else(|e| {
                panic!(
                    "Failed to rebuild {:?} from answers {:?}: {}",
                    pred.yaml_key(),
                    answers,
                    e
                )
            });
            assert_eq!(rebuilt, pred, "Roundtrip failed for {:?}", pred.yaml_key());
        }
    }

    #[test]
    fn roundtrip_all_propositions() {
        for prop in all_proposition_variants() {
            let answers = proposition_to_answers(&prop);
            let mut answerer = CannedAnswerer::new(answers.clone());
            let rebuilt = build_proposition(&mut answerer).unwrap_or_else(|e| {
                panic!(
                    "Failed to rebuild {:?} from answers {:?}: {}",
                    prop.yaml_key(),
                    answers,
                    e
                )
            });
            assert_eq!(rebuilt, prop, "Roundtrip failed for {:?}", prop.yaml_key());
        }
    }

    #[test]
    fn menu_item_count_matches_variants() {
        assert_eq!(predicate_menu_items().len(), all_predicate_variants().len());
        assert_eq!(
            proposition_menu_items().len(),
            all_proposition_variants().len()
        );
    }
}
