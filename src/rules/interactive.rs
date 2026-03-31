use anyhow::{bail, Result};

use crate::effects::Effects;
use crate::rules::ast::{DataArrayCheck, DataSchema, FilePredicateAst, Proposition, SimplePath};

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

// ---------------------------------------------------------------------------
// GoBack support
// ---------------------------------------------------------------------------

/// Result of an ask function: either a value or a request to go back.
enum AskResult<T> {
    Answer(T),
    GoBack,
}

/// Result of a builder function: either a built value or a request to go back.
#[derive(Debug)]
pub enum BuildResult<T> {
    Built(T),
    GoBack,
}

fn is_back(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("back")
}

// ---------------------------------------------------------------------------
// Levenshtein distance for fuzzy name matching
// ---------------------------------------------------------------------------

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Extract the canonical name from a menu item: text before the first `(`, trimmed.
fn canonical_name(option: &str) -> &str {
    match option.find('(') {
        Some(pos) => option[..pos].trim(),
        None => option.trim(),
    }
}

/// Try to match user input against option canonical names.
/// Returns the index if there's an unambiguous best match within threshold.
fn fuzzy_match_option(input: &str, options: &[&str]) -> Option<usize> {
    let input_lower = input.to_lowercase();

    // 1. Exact match on canonical name
    for (i, opt) in options.iter().enumerate() {
        if canonical_name(opt).to_lowercase() == input_lower {
            return Some(i);
        }
    }

    // 2. Prefix match (unique)
    let prefix_matches: Vec<usize> = options
        .iter()
        .enumerate()
        .filter(|(_, opt)| canonical_name(opt).to_lowercase().starts_with(&input_lower))
        .map(|(i, _)| i)
        .collect();
    if prefix_matches.len() == 1 {
        return Some(prefix_matches[0]);
    }

    // 3. Levenshtein — find the closest match
    let mut best_dist = usize::MAX;
    let mut best_idx = 0;
    let mut ambiguous = false;
    for (i, opt) in options.iter().enumerate() {
        let name = canonical_name(opt).to_lowercase();
        let dist = levenshtein(&input_lower, &name);
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
            ambiguous = false;
        } else if dist == best_dist {
            ambiguous = true;
        }
    }

    // Accept if distance is at most max(2, name_len/3) and unambiguous
    let best_name_len = canonical_name(options[best_idx]).len();
    let threshold = 2.max(best_name_len / 3);
    if !ambiguous && best_dist <= threshold {
        Some(best_idx)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Ask helpers
// ---------------------------------------------------------------------------

fn ask_select(
    answerer: &mut impl Answerer,
    prompt: &str,
    options: &[&str],
) -> Result<AskResult<usize>> {
    let mut question = format!("{}:\n", prompt);
    for (i, opt) in options.iter().enumerate() {
        question.push_str(&format!("  {}: {}\n", i + 1, opt));
    }
    question.push_str("Choose (number or name, or 'back')");

    loop {
        match answerer.ask(&question) {
            Answer::Text(s) => {
                let trimmed = s.trim();
                if is_back(trimmed) {
                    return Ok(AskResult::GoBack);
                }
                // Try number first
                if let Ok(n) = trimmed.parse::<usize>() {
                    if n >= 1 && n <= options.len() {
                        return Ok(AskResult::Answer(n - 1));
                    }
                }
                // Try name / fuzzy match
                if let Some(idx) = fuzzy_match_option(trimmed, options) {
                    return Ok(AskResult::Answer(idx));
                }
                // retry with shorter prompt
                question = format!(
                    "Invalid choice. Enter 1-{}, a name, or 'back'",
                    options.len()
                );
            }
            Answer::EndOfInput => bail!("Unexpected end of input"),
        }
    }
}

fn ask_string(answerer: &mut impl Answerer, prompt: &str) -> Result<AskResult<String>> {
    match answerer.ask(prompt) {
        Answer::Text(s) => {
            let trimmed = s.trim().to_string();
            if is_back(&trimmed) {
                return Ok(AskResult::GoBack);
            }
            if trimmed.is_empty() {
                bail!("Empty input is not allowed");
            }
            Ok(AskResult::Answer(trimmed))
        }
        Answer::EndOfInput => bail!("Unexpected end of input"),
    }
}

fn ask_yes_no(answerer: &mut impl Answerer, prompt: &str) -> Result<AskResult<bool>> {
    loop {
        match answerer.ask(prompt) {
            Answer::Text(s) => match s.trim().to_lowercase().as_str() {
                "y" | "yes" => return Ok(AskResult::Answer(true)),
                "n" | "no" => return Ok(AskResult::Answer(false)),
                "back" => return Ok(AskResult::GoBack),
                _ => {} // retry
            },
            Answer::EndOfInput => bail!("Unexpected end of input"),
        }
    }
}

fn ask_path(answerer: &mut impl Answerer, prompt: &str) -> Result<AskResult<SimplePath>> {
    match ask_string(answerer, prompt)? {
        AskResult::Answer(s) => Ok(AskResult::Answer(SimplePath::new(&s)?)),
        AskResult::GoBack => Ok(AskResult::GoBack),
    }
}

fn ask_path_list(answerer: &mut impl Answerer) -> Result<AskResult<Vec<SimplePath>>> {
    let mut paths = Vec::new();
    loop {
        match ask_path(answerer, "File path (~/...)")? {
            AskResult::Answer(p) => paths.push(p),
            AskResult::GoBack => return Ok(AskResult::GoBack),
        }
        match ask_yes_no(answerer, "Add another file? (y/n)")? {
            AskResult::Answer(true) => continue,
            AskResult::Answer(false) => break,
            AskResult::GoBack => return Ok(AskResult::GoBack),
        }
    }
    Ok(AskResult::Answer(paths))
}

// ---------------------------------------------------------------------------
// Collect items in a loop with GoBack support
// ---------------------------------------------------------------------------

/// Collect items by repeatedly calling a builder, with GoBack = pop-last or propagate.
/// Returns the collected items, or empty vec if the user backed out completely.
fn collect_items<T>(
    answerer: &mut impl Answerer,
    build: &mut dyn FnMut(&mut dyn Answerer) -> Result<BuildResult<T>>,
    more_prompt: &str,
) -> Result<Vec<T>>
where
    T: std::fmt::Debug,
{
    let mut items: Vec<T> = Vec::new();
    loop {
        // Use a trait-object-compatible wrapper since we need &mut dyn Answerer
        match build(answerer)? {
            BuildResult::Built(item) => items.push(item),
            BuildResult::GoBack => {
                if items.is_empty() {
                    return Ok(vec![]); // signal to caller: user backed out
                }
                items.pop();
                continue;
            }
        }
        match ask_yes_no(answerer, more_prompt)? {
            AskResult::Answer(true) => continue,
            AskResult::Answer(false) => break,
            AskResult::GoBack => {
                items.pop();
                continue;
            }
        }
    }
    Ok(items)
}

// ---------------------------------------------------------------------------
// DataSchema builder
// ---------------------------------------------------------------------------

/// Build a DataSchema interactively.
pub fn build_data_schema(answerer: &mut impl Answerer) -> Result<BuildResult<DataSchema>> {
    let variants = data_schema_menu_items();
    loop {
        let idx = match ask_select(answerer, "What kind of data check?", &variants)? {
            AskResult::Answer(i) => i,
            AskResult::GoBack => return Ok(BuildResult::GoBack),
        };

        let result = match idx {
            0 => DataSchema::Anything,
            1 => DataSchema::IsString,
            2 => match ask_string(answerer, "Regex pattern for string")? {
                AskResult::Answer(re) => DataSchema::IsStringMatching(re),
                AskResult::GoBack => continue,
            },
            3 => DataSchema::IsNumber,
            4 => DataSchema::IsBool,
            5 => DataSchema::IsNull,
            6 => {
                // is-object: collect key→schema pairs
                let mut entries: Vec<(String, DataSchema)> = Vec::new();
                loop {
                    let key = match ask_string(answerer, "Object key name")? {
                        AskResult::Answer(k) => k,
                        AskResult::GoBack => {
                            if entries.is_empty() {
                                break;
                            }
                            entries.pop();
                            continue;
                        }
                    };
                    let schema = match build_data_schema(answerer)? {
                        BuildResult::Built(s) => s,
                        BuildResult::GoBack => continue, // retry key entry
                    };
                    entries.push((key, schema));
                    match ask_yes_no(answerer, "Add another key? (y/n)")? {
                        AskResult::Answer(true) => continue,
                        AskResult::Answer(false) => break,
                        AskResult::GoBack => {
                            entries.pop();
                            continue;
                        }
                    }
                }
                if entries.is_empty() {
                    continue;
                }
                DataSchema::IsObject(entries)
            }
            7 => {
                // is-array: ask for forall, exists, at constraints
                let forall = match ask_yes_no(answerer, "Add forall constraint? (y/n)")? {
                    AskResult::Answer(true) => match build_data_schema(answerer)? {
                        BuildResult::Built(s) => Some(Box::new(s)),
                        BuildResult::GoBack => continue,
                    },
                    AskResult::Answer(false) => None,
                    AskResult::GoBack => continue,
                };
                let exists = match ask_yes_no(answerer, "Add exists constraint? (y/n)")? {
                    AskResult::Answer(true) => match build_data_schema(answerer)? {
                        BuildResult::Built(s) => Some(Box::new(s)),
                        BuildResult::GoBack => continue,
                    },
                    AskResult::Answer(false) => None,
                    AskResult::GoBack => continue,
                };
                let mut at = Vec::new();
                match ask_yes_no(answerer, "Add index constraints? (y/n)")? {
                    AskResult::Answer(true) => loop {
                        let idx_s = match ask_string(answerer, "Array index")? {
                            AskResult::Answer(s) => s,
                            AskResult::GoBack => {
                                if at.is_empty() {
                                    break;
                                }
                                at.pop();
                                continue;
                            }
                        };
                        let idx = idx_s.parse::<u32>().map_err(|e| anyhow::anyhow!("{}", e))?;
                        let schema = match build_data_schema(answerer)? {
                            BuildResult::Built(s) => s,
                            BuildResult::GoBack => continue,
                        };
                        at.push((idx, schema));
                        match ask_yes_no(answerer, "Add another index? (y/n)")? {
                            AskResult::Answer(true) => continue,
                            AskResult::Answer(false) => break,
                            AskResult::GoBack => {
                                at.pop();
                                continue;
                            }
                        }
                    },
                    AskResult::Answer(false) => {}
                    AskResult::GoBack => continue,
                }
                DataSchema::IsArray(DataArrayCheck { forall, exists, at })
            }
            _ => bail!("Invalid selection"),
        };

        return Ok(BuildResult::Built(result));
    }
}

// ---------------------------------------------------------------------------
// FilePredicateAst builder
// ---------------------------------------------------------------------------

/// Build a FilePredicateAst interactively.
pub fn build_predicate(answerer: &mut impl Answerer) -> Result<BuildResult<FilePredicateAst>> {
    let variants = predicate_menu_items();
    loop {
        let idx = match ask_select(answerer, "What kind of check?", &variants)? {
            AskResult::Answer(i) => i,
            AskResult::GoBack => return Ok(BuildResult::GoBack),
        };

        let result = match idx {
            0 => FilePredicateAst::FileExists,
            1 => match ask_string(answerer, "Regex pattern")? {
                AskResult::Answer(re) => FilePredicateAst::TextMatchesRegex(re),
                AskResult::GoBack => continue,
            },
            2 => {
                let min_s = match ask_string(answerer, "Min lines (or 'none')")? {
                    AskResult::Answer(s) => s,
                    AskResult::GoBack => continue,
                };
                let min = if min_s == "none" {
                    None
                } else {
                    Some(min_s.parse::<u32>().map_err(|e| anyhow::anyhow!("{}", e))?)
                };
                let max_s = match ask_string(answerer, "Max lines (or 'none')")? {
                    AskResult::Answer(s) => s,
                    AskResult::GoBack => continue,
                };
                let max = if max_s == "none" {
                    None
                } else {
                    Some(max_s.parse::<u32>().map_err(|e| anyhow::anyhow!("{}", e))?)
                };
                FilePredicateAst::TextHasLines { min, max }
            }
            3 => match ask_string(answerer, "Variable name")? {
                AskResult::Answer(var) => FilePredicateAst::ShellExports(var),
                AskResult::GoBack => continue,
            },
            4 => match ask_string(answerer, "Variable name")? {
                AskResult::Answer(var) => FilePredicateAst::ShellDefinesVariable(var),
                AskResult::GoBack => continue,
            },
            5 => match ask_string(answerer, "Variable name")? {
                AskResult::Answer(var) => FilePredicateAst::ShellAddsToPath(var),
                AskResult::GoBack => continue,
            },
            6 => match ask_string(answerer, "Property key")? {
                AskResult::Answer(key) => FilePredicateAst::PropertiesDefinesKey(key),
                AskResult::GoBack => continue,
            },
            7 => match ask_string(answerer, "XML element path (e.g. settings/servers/server)")? {
                AskResult::Answer(path) => FilePredicateAst::XmlMatchesPath(path),
                AskResult::GoBack => continue,
            },
            8 => match build_data_schema(answerer)? {
                BuildResult::Built(schema) => FilePredicateAst::JsonMatches(schema),
                BuildResult::GoBack => continue,
            },
            9 => match build_data_schema(answerer)? {
                BuildResult::Built(schema) => FilePredicateAst::YamlMatches(schema),
                BuildResult::GoBack => continue,
            },
            10 => {
                let preds = collect_items(
                    answerer,
                    &mut |a| build_predicate_dyn(a),
                    "Add another check? (y/n)",
                )?;
                if preds.is_empty() {
                    continue;
                }
                FilePredicateAst::All(preds)
            }
            11 => {
                let hint = match ask_string(answerer, "Hint for user when all alternatives fail")? {
                    AskResult::Answer(h) => h,
                    AskResult::GoBack => continue,
                };
                let checks = collect_items(
                    answerer,
                    &mut |a| build_predicate_dyn(a),
                    "Add another alternative? (y/n)",
                )?;
                if checks.is_empty() {
                    continue;
                }
                FilePredicateAst::Any { hint, checks }
            }
            _ => bail!("Invalid selection"),
        };

        return Ok(BuildResult::Built(result));
    }
}

/// Wrapper to call build_predicate through a &mut dyn Answerer.
fn build_predicate_dyn(answerer: &mut dyn Answerer) -> Result<BuildResult<FilePredicateAst>> {
    // We need a concrete impl to call build_predicate. Use a thin wrapper.
    struct DynAnswerer<'a>(&'a mut dyn Answerer);
    impl<'a> Answerer for DynAnswerer<'a> {
        fn ask(&mut self, question: &str) -> Answer {
            self.0.ask(question)
        }
    }
    build_predicate(&mut DynAnswerer(answerer))
}

/// Wrapper to call build_proposition through a &mut dyn Answerer.
fn build_proposition_dyn(answerer: &mut dyn Answerer) -> Result<BuildResult<Proposition>> {
    struct DynAnswerer<'a>(&'a mut dyn Answerer);
    impl<'a> Answerer for DynAnswerer<'a> {
        fn ask(&mut self, question: &str) -> Answer {
            self.0.ask(question)
        }
    }
    build_proposition(&mut DynAnswerer(answerer))
}

// ---------------------------------------------------------------------------
// Proposition builder
// ---------------------------------------------------------------------------

/// Build a Proposition interactively.
pub fn build_proposition(answerer: &mut impl Answerer) -> Result<BuildResult<Proposition>> {
    let variants = proposition_menu_items();
    loop {
        let idx = match ask_select(answerer, "What kind of rule?", &variants)? {
            AskResult::Answer(i) => i,
            AskResult::GoBack => return Ok(BuildResult::GoBack),
        };

        let result = match idx {
            0 => {
                let path = match ask_path(answerer, "File path (~/...)")? {
                    AskResult::Answer(p) => p,
                    AskResult::GoBack => continue,
                };
                let check = match build_predicate(answerer)? {
                    BuildResult::Built(c) => c,
                    BuildResult::GoBack => continue,
                };
                Proposition::FileSatisfies { path, check }
            }
            1 => {
                let files = match ask_path_list(answerer)? {
                    AskResult::Answer(f) => f,
                    AskResult::GoBack => continue,
                };
                let check = match build_predicate(answerer)? {
                    BuildResult::Built(c) => c,
                    BuildResult::GoBack => continue,
                };
                Proposition::Forall { files, check }
            }
            2 => {
                let files = match ask_path_list(answerer)? {
                    AskResult::Answer(f) => f,
                    AskResult::GoBack => continue,
                };
                let check = match build_predicate(answerer)? {
                    BuildResult::Built(c) => c,
                    BuildResult::GoBack => continue,
                };
                Proposition::Exists { files, check }
            }
            3 => {
                let props = collect_items(
                    answerer,
                    &mut |a| build_proposition_dyn(a),
                    "Add another rule? (y/n)",
                )?;
                if props.is_empty() {
                    continue;
                }
                Proposition::All(props)
            }
            4 => {
                let props = collect_items(
                    answerer,
                    &mut |a| build_proposition_dyn(a),
                    "Add another alternative? (y/n)",
                )?;
                if props.is_empty() {
                    continue;
                }
                Proposition::Any(props)
            }
            _ => bail!("Invalid selection"),
        };

        return Ok(BuildResult::Built(result));
    }
}

// ---------------------------------------------------------------------------
// Menu items
// ---------------------------------------------------------------------------

fn data_schema_menu_items() -> Vec<&'static str> {
    vec![
        "anything (matches any value)",
        "is-string",
        "is-string-matching (regex)",
        "is-number",
        "is-bool",
        "is-null",
        "is-object (keys with schemas)",
        "is-array (element constraints)",
    ]
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
        "json-matches (data schema)",
        "yaml-matches (data schema)",
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

/// Entry point for the `key rules add` command.
pub fn run_interactive_add(fx: &dyn Effects) -> Result<Proposition> {
    let mut answerer = EffectsAnswerer { fx };
    loop {
        match build_proposition(&mut answerer)? {
            BuildResult::Built(prop) => return Ok(prop),
            BuildResult::GoBack => {
                fx.println("Already at top level, cannot go back further.");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test helpers: convert AST → answer sequences for roundtrip testing
// ---------------------------------------------------------------------------

/// Convert a DataSchema into the sequence of answers needed to reconstruct it.
#[cfg(test)]
pub fn data_schema_to_answers(schema: &DataSchema) -> Vec<String> {
    let mut answers = Vec::new();
    data_schema_to_answers_inner(schema, &mut answers);
    answers
}

#[cfg(test)]
fn data_schema_to_answers_inner(schema: &DataSchema, answers: &mut Vec<String>) {
    match schema {
        DataSchema::Anything => answers.push("1".into()),
        DataSchema::IsString => answers.push("2".into()),
        DataSchema::IsStringMatching(re) => {
            answers.push("3".into());
            answers.push(re.clone());
        }
        DataSchema::IsNumber => answers.push("4".into()),
        DataSchema::IsBool => answers.push("5".into()),
        DataSchema::IsNull => answers.push("6".into()),
        DataSchema::IsObject(entries) => {
            answers.push("7".into());
            for (i, (key, sub)) in entries.iter().enumerate() {
                answers.push(key.clone());
                data_schema_to_answers_inner(sub, answers);
                if i < entries.len() - 1 {
                    answers.push("y".into()); // add another key
                } else {
                    answers.push("n".into()); // done
                }
            }
        }
        DataSchema::IsArray(check) => {
            answers.push("8".into());
            // forall?
            if let Some(ref f) = check.forall {
                answers.push("y".into());
                data_schema_to_answers_inner(f, answers);
            } else {
                answers.push("n".into());
            }
            // exists?
            if let Some(ref e) = check.exists {
                answers.push("y".into());
                data_schema_to_answers_inner(e, answers);
            } else {
                answers.push("n".into());
            }
            // index constraints?
            if !check.at.is_empty() {
                answers.push("y".into());
                for (i, (idx, sub)) in check.at.iter().enumerate() {
                    answers.push(idx.to_string());
                    data_schema_to_answers_inner(sub, answers);
                    if i < check.at.len() - 1 {
                        answers.push("y".into());
                    } else {
                        answers.push("n".into());
                    }
                }
            } else {
                answers.push("n".into());
            }
        }
    }
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
        FilePredicateAst::JsonMatches(schema) => {
            answers.push("9".into());
            data_schema_to_answers_inner(schema, answers);
        }
        FilePredicateAst::YamlMatches(schema) => {
            answers.push("10".into());
            data_schema_to_answers_inner(schema, answers);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::{
        all_data_schema_variants, all_predicate_variants, all_proposition_variants,
    };

    #[test]
    fn roundtrip_all_data_schemas() {
        for schema in all_data_schema_variants() {
            let answers = data_schema_to_answers(&schema);
            let mut answerer = CannedAnswerer::new(answers.clone());
            let rebuilt = build_data_schema(&mut answerer).unwrap_or_else(|e| {
                panic!(
                    "Failed to rebuild DataSchema {:?} from answers {:?}: {}",
                    schema, answers, e
                )
            });
            match rebuilt {
                BuildResult::Built(s) => assert_eq!(s, schema, "Roundtrip failed"),
                BuildResult::GoBack => panic!("Unexpected GoBack"),
            }
        }
    }

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
            match rebuilt {
                BuildResult::Built(p) => {
                    assert_eq!(p, pred, "Roundtrip failed for {:?}", pred.yaml_key())
                }
                BuildResult::GoBack => {
                    panic!("Unexpected GoBack for {:?}", pred.yaml_key())
                }
            }
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
            match rebuilt {
                BuildResult::Built(p) => {
                    assert_eq!(p, prop, "Roundtrip failed for {:?}", prop.yaml_key())
                }
                BuildResult::GoBack => {
                    panic!("Unexpected GoBack for {:?}", prop.yaml_key())
                }
            }
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

    #[test]
    fn go_back_at_top_level_predicate() {
        // Typing "back" at the first menu should return GoBack
        let mut answerer = CannedAnswerer::new(vec!["back".into()]);
        match build_predicate(&mut answerer).unwrap() {
            BuildResult::GoBack => {} // expected
            BuildResult::Built(p) => panic!("Expected GoBack, got {:?}", p),
        }
    }

    #[test]
    fn go_back_after_selection_restarts() {
        // Select "text-matches" (2), then type "back" for the regex → restarts,
        // then select "file-exists" (1)
        let mut answerer = CannedAnswerer::new(vec![
            "2".into(),    // select text-matches
            "back".into(), // back out of regex prompt → restart
            "1".into(),    // select file-exists
        ]);
        match build_predicate(&mut answerer).unwrap() {
            BuildResult::Built(FilePredicateAst::FileExists) => {} // expected
            other => panic!("Expected FileExists, got {:?}", other),
        }
    }

    #[test]
    fn select_by_exact_canonical_name() {
        // Type "file-exists" instead of "1"
        let mut answerer = CannedAnswerer::new(vec!["file-exists".into()]);
        match build_predicate(&mut answerer).unwrap() {
            BuildResult::Built(FilePredicateAst::FileExists) => {}
            other => panic!("Expected FileExists, got {:?}", other),
        }
    }

    #[test]
    fn select_by_canonical_name_case_insensitive() {
        let mut answerer = CannedAnswerer::new(vec!["File-Exists".into()]);
        match build_predicate(&mut answerer).unwrap() {
            BuildResult::Built(FilePredicateAst::FileExists) => {}
            other => panic!("Expected FileExists, got {:?}", other),
        }
    }

    #[test]
    fn select_by_prefix() {
        // "shell-exp" uniquely matches "shell-exports (variable)"
        let mut answerer = CannedAnswerer::new(vec!["shell-exp".into(), "MY_VAR".into()]);
        match build_predicate(&mut answerer).unwrap() {
            BuildResult::Built(FilePredicateAst::ShellExports(var)) => {
                assert_eq!(var, "MY_VAR");
            }
            other => panic!("Expected ShellExports, got {:?}", other),
        }
    }

    #[test]
    fn select_by_fuzzy_typo() {
        // "fil-exists" is 1 edit away from "file-exists"
        let mut answerer = CannedAnswerer::new(vec!["fil-exists".into()]);
        match build_predicate(&mut answerer).unwrap() {
            BuildResult::Built(FilePredicateAst::FileExists) => {}
            other => panic!("Expected FileExists, got {:?}", other),
        }
    }

    #[test]
    fn select_by_name_in_proposition() {
        // Type "file" to select "file (single file check)"
        let mut answerer =
            CannedAnswerer::new(vec!["file".into(), "~/test".into(), "file-exists".into()]);
        match build_proposition(&mut answerer).unwrap() {
            BuildResult::Built(Proposition::FileSatisfies { path, check }) => {
                assert_eq!(path.as_str(), "~/test");
                assert_eq!(check, FilePredicateAst::FileExists);
            }
            other => panic!("Expected FileSatisfies, got {:?}", other),
        }
    }

    #[test]
    fn select_by_name_in_data_schema() {
        // Type "is-string" to select the is-string option
        let mut answerer = CannedAnswerer::new(vec!["is-string".into()]);
        match build_data_schema(&mut answerer).unwrap() {
            BuildResult::Built(DataSchema::IsString) => {}
            other => panic!("Expected IsString, got {:?}", other),
        }
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
    }

    #[test]
    fn fuzzy_match_rejects_garbage() {
        let options = predicate_menu_items();
        assert!(fuzzy_match_option("xyzzy-nonsense-garbage", &options).is_none());
    }
}
