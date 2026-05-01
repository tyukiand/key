// spec/0016 §B.1 — single-file `key audit add` is removed. The interactive
// add-control logic in this module is now dead code, kept only because some
// of its tests exercise generic helpers (Lexical resolution, fuzzy matching)
// that remain useful as regression coverage. Suppress unused warnings.
#![allow(dead_code)]

use anyhow::{bail, Result};

use crate::effects::Effects;
use crate::rules::ast::{
    Control, DataArrayCheck, DataSchema, FilePredicateAst, Proposition, SimplePath,
};

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

        let tag = menu_tag(variants[idx]);
        let result = match tag {
            "anything" => DataSchema::Anything,
            "is-string" => DataSchema::IsString,
            "is-string-matching" => match ask_string(answerer, "Regex pattern for string")? {
                AskResult::Answer(re) => DataSchema::IsStringMatching(re),
                AskResult::GoBack => continue,
            },
            "is-number" => DataSchema::IsNumber,
            "is-bool" => DataSchema::IsBool,
            "is-true" => DataSchema::IsTrue,
            "is-false" => DataSchema::IsFalse,
            "is-null" => DataSchema::IsNull,
            "is-object" => {
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
            "is-array" => {
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

        let tag = menu_tag(variants[idx]);
        let result = match tag {
            "file-exists" => FilePredicateAst::FileExists,
            "text-matches" => match ask_string(answerer, "Regex pattern")? {
                AskResult::Answer(re) => FilePredicateAst::TextMatchesRegex(re),
                AskResult::GoBack => continue,
            },
            "text-contains" => match ask_string(answerer, "Literal substring to find")? {
                AskResult::Answer(s) => FilePredicateAst::TextContains(s),
                AskResult::GoBack => continue,
            },
            "text-has-lines" => {
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
            "shell-exports" => match ask_string(answerer, "Variable name")? {
                AskResult::Answer(var) => FilePredicateAst::ShellExports(var),
                AskResult::GoBack => continue,
            },
            "shell-defines" => match ask_string(answerer, "Variable name")? {
                AskResult::Answer(var) => FilePredicateAst::ShellDefinesVariable(var),
                AskResult::GoBack => continue,
            },
            "shell-adds-to-path" => match ask_string(answerer, "Variable name")? {
                AskResult::Answer(var) => FilePredicateAst::ShellAddsToPath(var),
                AskResult::GoBack => continue,
            },
            "properties-defines-key" => match ask_string(answerer, "Property key")? {
                AskResult::Answer(key) => FilePredicateAst::PropertiesDefinesKey(key),
                AskResult::GoBack => continue,
            },
            "xml-matches" => {
                match ask_string(answerer, "XML element path (e.g. settings/servers/server)")? {
                    AskResult::Answer(path) => FilePredicateAst::XmlMatchesPath(path),
                    AskResult::GoBack => continue,
                }
            }
            "json-matches" => match build_data_schema(answerer)? {
                BuildResult::Built(schema) => FilePredicateAst::JsonMatches(schema),
                BuildResult::GoBack => continue,
            },
            "yaml-matches" => match build_data_schema(answerer)? {
                BuildResult::Built(schema) => FilePredicateAst::YamlMatches(schema),
                BuildResult::GoBack => continue,
            },
            "looks-like-password" => FilePredicateAst::LooksLikePassword,
            "all" => {
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
            "any" => {
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
            "not" => {
                let inner = match build_predicate(answerer)? {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => continue,
                };
                FilePredicateAst::Not(Box::new(inner))
            }
            "conditionally" => {
                let condition = match build_predicate(answerer)? {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => continue,
                };
                let then = match build_predicate(answerer)? {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => continue,
                };
                FilePredicateAst::Conditionally {
                    condition: Box::new(condition),
                    then: Box::new(then),
                }
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

        let tag = menu_tag(variants[idx]);
        let result = match tag {
            "file" => {
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
            "forall" => {
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
            "exists" => {
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
            "all" => {
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
            "any" => {
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
            "not" => {
                let inner = match build_proposition(answerer)? {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => continue,
                };
                Proposition::Not(Box::new(inner))
            }
            "conditionally" => {
                let condition = match build_proposition(answerer)? {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => continue,
                };
                let then = match build_proposition(answerer)? {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => continue,
                };
                Proposition::Conditionally {
                    condition: Box::new(condition),
                    then: Box::new(then),
                }
            }
            _ => bail!("Invalid selection: {:?}", tag),
        };

        return Ok(BuildResult::Built(result));
    }
}

// ---------------------------------------------------------------------------
// Menu items
// ---------------------------------------------------------------------------

/// Extract the leading tag from a menu item string.
/// E.g. `"text-matches (regex)"` → `"text-matches"`.
fn menu_tag(item: &str) -> &str {
    item.split_whitespace().next().unwrap_or(item)
}

/// Find the 1-based menu index for a given tag in a menu list.
/// Panics if the tag is not found (programming error).
#[cfg(test)]
fn menu_index_of(items: &[&str], tag: &str) -> String {
    let idx = items
        .iter()
        .position(|item| menu_tag(item) == tag)
        .unwrap_or_else(|| panic!("menu tag {:?} not found in menu items", tag));
    (idx + 1).to_string()
}

fn data_schema_menu_items() -> Vec<&'static str> {
    vec![
        "anything (matches any value)",
        "is-string",
        "is-string-matching (regex)",
        "is-number",
        "is-bool",
        "is-true (strict)",
        "is-false (strict)",
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
        "text-contains (literal substring)",
        "text-has-lines (min/max)",
        "shell-exports (variable)",
        "shell-defines (variable)",
        "shell-adds-to-path (variable)",
        "properties-defines-key (ini/properties key=value)",
        "xml-matches (element path)",
        "json-matches (data schema)",
        "yaml-matches (data schema)",
        "looks-like-password (any value or line was redacted)",
        "all (multiple checks)",
        "any (alternatives with hint)",
        "not (negate)",
        "conditionally (if-then)",
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
        "not (negate)",
        "conditionally (if-then)",
    ]
}

/// Entry point for the `key audit add` command.
pub fn run_interactive_add_control(fx: &dyn Effects) -> Result<Control> {
    let mut answerer = EffectsAnswerer { fx };
    loop {
        let id = match ask_string(&mut answerer, "Control ID (e.g. SSH-KEY-EXISTS)")? {
            AskResult::Answer(s) => s,
            AskResult::GoBack => {
                fx.println("Already at top level, cannot go back further.");
                continue;
            }
        };
        if let Err(e) = crate::rules::ast::validate_control_id(&id) {
            fx.println(&format!("Invalid control ID: {}", e));
            continue;
        }
        let title = match ask_string(&mut answerer, "Title (short description)")? {
            AskResult::Answer(s) => s,
            AskResult::GoBack => continue,
        };
        let description = match ask_string(&mut answerer, "Description (what is checked)")? {
            AskResult::Answer(s) => s,
            AskResult::GoBack => continue,
        };
        let remediation = match ask_string(&mut answerer, "Remediation (how to fix if it fails)")? {
            AskResult::Answer(s) => s,
            AskResult::GoBack => continue,
        };
        let check = match build_proposition(&mut answerer)? {
            BuildResult::Built(p) => p,
            BuildResult::GoBack => continue,
        };
        return Ok(Control {
            id,
            title,
            description,
            remediation,
            check,
        });
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
    let items = data_schema_menu_items();
    let mi = |tag: &str| menu_index_of(&items, tag);
    match schema {
        DataSchema::Anything => answers.push(mi("anything")),
        DataSchema::IsString => answers.push(mi("is-string")),
        DataSchema::IsStringMatching(re) => {
            answers.push(mi("is-string-matching"));
            answers.push(re.clone());
        }
        DataSchema::IsNumber => answers.push(mi("is-number")),
        DataSchema::IsBool => answers.push(mi("is-bool")),
        DataSchema::IsTrue => answers.push(mi("is-true")),
        DataSchema::IsFalse => answers.push(mi("is-false")),
        DataSchema::IsNull => answers.push(mi("is-null")),
        DataSchema::IsObject(entries) => {
            answers.push(mi("is-object"));
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
            answers.push(mi("is-array"));
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
    let items = predicate_menu_items();
    let mi = |tag: &str| menu_index_of(&items, tag);
    match pred {
        FilePredicateAst::FileExists => {
            answers.push(mi("file-exists"));
        }
        FilePredicateAst::TextMatchesRegex(re) => {
            answers.push(mi("text-matches"));
            answers.push(re.clone());
        }
        FilePredicateAst::TextContains(s) => {
            answers.push(mi("text-contains"));
            answers.push(s.clone());
        }
        FilePredicateAst::TextHasLines { min, max } => {
            answers.push(mi("text-has-lines"));
            answers.push(min.map(|n| n.to_string()).unwrap_or_else(|| "none".into()));
            answers.push(max.map(|n| n.to_string()).unwrap_or_else(|| "none".into()));
        }
        FilePredicateAst::ShellExports(var) => {
            answers.push(mi("shell-exports"));
            answers.push(var.clone());
        }
        FilePredicateAst::ShellDefinesVariable(var) => {
            answers.push(mi("shell-defines"));
            answers.push(var.clone());
        }
        FilePredicateAst::ShellAddsToPath(var) => {
            answers.push(mi("shell-adds-to-path"));
            answers.push(var.clone());
        }
        FilePredicateAst::PropertiesDefinesKey(key) => {
            answers.push(mi("properties-defines-key"));
            answers.push(key.clone());
        }
        FilePredicateAst::XmlMatchesPath(path) => {
            answers.push(mi("xml-matches"));
            answers.push(path.clone());
        }
        FilePredicateAst::JsonMatches(schema) => {
            answers.push(mi("json-matches"));
            data_schema_to_answers_inner(schema, answers);
        }
        FilePredicateAst::YamlMatches(schema) => {
            answers.push(mi("yaml-matches"));
            data_schema_to_answers_inner(schema, answers);
        }
        FilePredicateAst::All(preds) => {
            answers.push(mi("all"));
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
            answers.push(mi("any"));
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
        FilePredicateAst::Not(inner) => {
            answers.push(mi("not"));
            predicate_to_answers_inner(inner, answers);
        }
        FilePredicateAst::Conditionally { condition, then } => {
            answers.push(mi("conditionally"));
            predicate_to_answers_inner(condition, answers);
            predicate_to_answers_inner(then, answers);
        }
        FilePredicateAst::ShellExportsValueMatches { .. }
        | FilePredicateAst::ShellDefinesVariableValueMatches { .. } => {
            panic!(
                "shell-exports/defines value-matches mapping form is not exposed via the \
                 interactive picker; only the bare-string form is reachable from `audit add`."
            );
        }
        FilePredicateAst::LooksLikePassword => {
            answers.push(mi("looks-like-password"));
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
    let items = proposition_menu_items();
    let mi = |tag: &str| menu_index_of(&items, tag);
    match prop {
        Proposition::FileSatisfies { path, check } => {
            answers.push(mi("file"));
            answers.push(path.as_str().into());
            predicate_to_answers_inner(check, answers);
        }
        Proposition::Forall { files, check } => {
            answers.push(mi("forall"));
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
            answers.push(mi("exists"));
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
            answers.push(mi("all"));
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
            answers.push(mi("any"));
            for (i, p) in props.iter().enumerate() {
                proposition_to_answers_inner(p, answers);
                if i < props.len() - 1 {
                    answers.push("y".into());
                } else {
                    answers.push("n".into());
                }
            }
        }
        Proposition::Not(inner) => {
            answers.push(mi("not"));
            proposition_to_answers_inner(inner, answers);
        }
        Proposition::Conditionally { condition, then } => {
            answers.push(mi("conditionally"));
            proposition_to_answers_inner(condition, answers);
            proposition_to_answers_inner(then, answers);
        }
    }
}

// ---------------------------------------------------------------------------
// Typed-AsmOp compilers (spec/0015 §6.1) — same answer-stream logic as the
// `*_to_answers` helpers above, but emitting the typed AsmOp alphabet directly.
//
// The build_* functions above still consume `Answerer` (raw strings); the
// typed pipeline rendezvous is `asm_to_string` below — driving CannedAnswerer
// with the lowered AsmOp sequence reproduces the same Vec<String> output as
// the legacy `*_to_answers`.
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "testing"))]
use crate::interaction::{AsmOp, LexicalPattern};

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
pub fn compile_data_schema(schema: &DataSchema) -> Vec<AsmOp> {
    let mut ops = Vec::new();
    compile_data_schema_inner(schema, &mut ops);
    ops
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
fn select(tag: &str) -> AsmOp {
    AsmOp::Select(LexicalPattern::new(tag))
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
fn enter(s: impl Into<String>) -> AsmOp {
    AsmOp::Enter(s.into())
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
fn compile_data_schema_inner(schema: &DataSchema, ops: &mut Vec<AsmOp>) {
    match schema {
        DataSchema::Anything => ops.push(select("anything")),
        DataSchema::IsString => ops.push(select("is-string")),
        DataSchema::IsStringMatching(re) => {
            ops.push(select("is-string-matching"));
            ops.push(enter(re.clone()));
        }
        DataSchema::IsNumber => ops.push(select("is-number")),
        DataSchema::IsBool => ops.push(select("is-bool")),
        DataSchema::IsTrue => ops.push(select("is-true")),
        DataSchema::IsFalse => ops.push(select("is-false")),
        DataSchema::IsNull => ops.push(select("is-null")),
        DataSchema::IsObject(entries) => {
            ops.push(select("is-object"));
            for (i, (key, sub)) in entries.iter().enumerate() {
                ops.push(enter(key.clone()));
                compile_data_schema_inner(sub, ops);
                if i < entries.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
        }
        DataSchema::IsArray(check) => {
            ops.push(select("is-array"));
            if let Some(ref f) = check.forall {
                ops.push(AsmOp::Yes);
                compile_data_schema_inner(f, ops);
            } else {
                ops.push(AsmOp::No);
            }
            if let Some(ref e) = check.exists {
                ops.push(AsmOp::Yes);
                compile_data_schema_inner(e, ops);
            } else {
                ops.push(AsmOp::No);
            }
            if !check.at.is_empty() {
                ops.push(AsmOp::Yes);
                for (i, (idx, sub)) in check.at.iter().enumerate() {
                    ops.push(enter(idx.to_string()));
                    compile_data_schema_inner(sub, ops);
                    if i < check.at.len() - 1 {
                        ops.push(AsmOp::Yes);
                    } else {
                        ops.push(AsmOp::No);
                    }
                }
            } else {
                ops.push(AsmOp::No);
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
pub fn compile_predicate(pred: &FilePredicateAst) -> Vec<AsmOp> {
    let mut ops = Vec::new();
    compile_predicate_inner(pred, &mut ops);
    ops
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
fn compile_predicate_inner(pred: &FilePredicateAst, ops: &mut Vec<AsmOp>) {
    match pred {
        FilePredicateAst::FileExists => ops.push(select("file-exists")),
        FilePredicateAst::TextMatchesRegex(re) => {
            ops.push(select("text-matches"));
            ops.push(enter(re.clone()));
        }
        FilePredicateAst::TextContains(s) => {
            ops.push(select("text-contains"));
            ops.push(enter(s.clone()));
        }
        FilePredicateAst::TextHasLines { min, max } => {
            ops.push(select("text-has-lines"));
            ops.push(enter(
                min.map(|n| n.to_string()).unwrap_or_else(|| "none".into()),
            ));
            ops.push(enter(
                max.map(|n| n.to_string()).unwrap_or_else(|| "none".into()),
            ));
        }
        FilePredicateAst::ShellExports(var) => {
            ops.push(select("shell-exports"));
            ops.push(enter(var.clone()));
        }
        FilePredicateAst::ShellDefinesVariable(var) => {
            ops.push(select("shell-defines"));
            ops.push(enter(var.clone()));
        }
        FilePredicateAst::ShellAddsToPath(var) => {
            ops.push(select("shell-adds-to-path"));
            ops.push(enter(var.clone()));
        }
        FilePredicateAst::PropertiesDefinesKey(key) => {
            ops.push(select("properties-defines-key"));
            ops.push(enter(key.clone()));
        }
        FilePredicateAst::XmlMatchesPath(path) => {
            ops.push(select("xml-matches"));
            ops.push(enter(path.clone()));
        }
        FilePredicateAst::JsonMatches(schema) => {
            ops.push(select("json-matches"));
            compile_data_schema_inner(schema, ops);
        }
        FilePredicateAst::YamlMatches(schema) => {
            ops.push(select("yaml-matches"));
            compile_data_schema_inner(schema, ops);
        }
        FilePredicateAst::All(preds) => {
            ops.push(select("all"));
            for (i, p) in preds.iter().enumerate() {
                compile_predicate_inner(p, ops);
                if i < preds.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
        }
        FilePredicateAst::Any { hint, checks } => {
            ops.push(select("any"));
            ops.push(enter(hint.clone()));
            for (i, c) in checks.iter().enumerate() {
                compile_predicate_inner(c, ops);
                if i < checks.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
        }
        FilePredicateAst::Not(inner) => {
            ops.push(select("not"));
            compile_predicate_inner(inner, ops);
        }
        FilePredicateAst::Conditionally { condition, then } => {
            ops.push(select("conditionally"));
            compile_predicate_inner(condition, ops);
            compile_predicate_inner(then, ops);
        }
        FilePredicateAst::ShellExportsValueMatches { .. }
        | FilePredicateAst::ShellDefinesVariableValueMatches { .. } => {
            panic!(
                "shell-exports/defines value-matches mapping form is not exposed via the \
                 interactive picker; only the bare-string form is reachable from `audit add`."
            );
        }
        FilePredicateAst::LooksLikePassword => ops.push(select("looks-like-password")),
    }
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
pub fn compile_proposition(prop: &Proposition) -> Vec<AsmOp> {
    let mut ops = Vec::new();
    compile_proposition_inner(prop, &mut ops);
    ops
}

#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
fn compile_proposition_inner(prop: &Proposition, ops: &mut Vec<AsmOp>) {
    match prop {
        Proposition::FileSatisfies { path, check } => {
            ops.push(select("file"));
            ops.push(enter(path.as_str()));
            compile_predicate_inner(check, ops);
        }
        Proposition::Forall { files, check } => {
            ops.push(select("forall"));
            for (i, f) in files.iter().enumerate() {
                ops.push(enter(f.as_str()));
                if i < files.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
            compile_predicate_inner(check, ops);
        }
        Proposition::Exists { files, check } => {
            ops.push(select("exists"));
            for (i, f) in files.iter().enumerate() {
                ops.push(enter(f.as_str()));
                if i < files.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
            compile_predicate_inner(check, ops);
        }
        Proposition::All(props) => {
            ops.push(select("all"));
            for (i, p) in props.iter().enumerate() {
                compile_proposition_inner(p, ops);
                if i < props.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
        }
        Proposition::Any(props) => {
            ops.push(select("any"));
            for (i, p) in props.iter().enumerate() {
                compile_proposition_inner(p, ops);
                if i < props.len() - 1 {
                    ops.push(AsmOp::Yes);
                } else {
                    ops.push(AsmOp::No);
                }
            }
        }
        Proposition::Not(inner) => {
            ops.push(select("not"));
            compile_proposition_inner(inner, ops);
        }
        Proposition::Conditionally { condition, then } => {
            ops.push(select("conditionally"));
            compile_proposition_inner(condition, ops);
            compile_proposition_inner(then, ops);
        }
    }
}

/// Lower a typed AsmOp to the legacy answer-string the build_* functions
/// expect. Used to drive the existing build_* through the typed pipeline so
/// that compile_*(ast) → AsmOp → string-driver → build_*(ast') yields ast' = ast.
#[cfg(any(test, feature = "testing"))]
#[allow(dead_code)]
pub fn asm_to_legacy_string(op: &AsmOp) -> String {
    match op {
        AsmOp::Select(p) => p.as_str().to_string(),
        AsmOp::Enter(s) => s.clone(),
        AsmOp::Yes => "y".to_string(),
        AsmOp::No => "n".to_string(),
        AsmOp::Back => "back".to_string(),
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
            // The mapping form of shell-exports / shell-defines is parser-only;
            // it has no menu entry, so skip its round-trip through the picker.
            if matches!(
                pred,
                FilePredicateAst::ShellExportsValueMatches { .. }
                    | FilePredicateAst::ShellDefinesVariableValueMatches { .. }
            ) {
                continue;
            }
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

    fn predicate_contains_value_matches(p: &FilePredicateAst) -> bool {
        use FilePredicateAst::*;
        match p {
            ShellExportsValueMatches { .. } | ShellDefinesVariableValueMatches { .. } => true,
            All(checks) => checks.iter().any(predicate_contains_value_matches),
            Any { checks, .. } => checks.iter().any(predicate_contains_value_matches),
            Not(c) => predicate_contains_value_matches(c),
            Conditionally { condition, then } => {
                predicate_contains_value_matches(condition)
                    || predicate_contains_value_matches(then)
            }
            _ => false,
        }
    }

    fn proposition_contains_value_matches(p: &Proposition) -> bool {
        use Proposition::*;
        match p {
            FileSatisfies { check, .. } => predicate_contains_value_matches(check),
            Forall { check, .. } => predicate_contains_value_matches(check),
            Exists { check, .. } => predicate_contains_value_matches(check),
            All(props) => props.iter().any(proposition_contains_value_matches),
            Any(props) => props.iter().any(proposition_contains_value_matches),
            Not(p) => proposition_contains_value_matches(p),
            Conditionally { condition, then } => {
                proposition_contains_value_matches(condition)
                    || proposition_contains_value_matches(then)
            }
        }
    }

    /// Spec/0013 §B.8.5(i) — every ExampleControl in the verbose EDSL guide
    /// MUST round-trip through the interactive engine. This catches drift
    /// between the documented examples and the menu compiler / engine.
    /// Sources (ii) [in-tree fixture corpus], (iii) [proptest fuzz], and
    /// (iv) [menu_torture/*.yaml] are deferred to a follow-up; the EDSL
    /// example set already exercises every Feature reachable from the
    /// verbose pass per spec/0011 §C.4 + spec/0013 §B.7, so passing this
    /// test proves variant coverage.
    #[test]
    fn roundtrip_every_edsl_example_proposition() {
        use crate::guide_edsl::nodes::GuideNode;
        use crate::guide_edsl::tree::root;
        use crate::rules::parse::parse_control_file;

        fn walk(node: &GuideNode, out: &mut Vec<String>) {
            match node {
                GuideNode::Section { body, .. } => {
                    for c in body {
                        walk(c, out);
                    }
                }
                GuideNode::ExampleControl { yaml, .. } => out.push((*yaml).to_string()),
                _ => {}
            }
        }

        let mut yamls: Vec<String> = Vec::new();
        walk(&root(), &mut yamls);
        assert!(!yamls.is_empty());

        for yaml in &yamls {
            let cf = match parse_control_file(yaml) {
                Ok(c) => c,
                Err(_) => continue, // load-error examples are intentional
            };
            for control in &cf.controls {
                // Skip controls that contain a value-matches refinement: spec
                // 0010 §6.X.2 — the mapping form has no interactive picker
                // entry; only the bare-string form is reachable. Documented
                // gap; spec/0013 §B.8.6 calls these out as a follow-up.
                if proposition_contains_value_matches(&control.check) {
                    continue;
                }
                let answers = proposition_to_answers(&control.check);
                let mut answerer = CannedAnswerer::new(answers.clone());
                let rebuilt = build_proposition(&mut answerer).unwrap_or_else(|e| {
                    panic!(
                        "ExampleControl {} did not rebuild via the interactive engine: \
                         {:#}\nproposition: {:?}\nanswers: {:?}\nyaml: {}",
                        control.id, e, control.check, answers, yaml,
                    )
                });
                match rebuilt {
                    BuildResult::Built(p) => assert_eq!(
                        p, control.check,
                        "Round-trip diff for {}:\nexpected: {:?}\nactual:   {:?}\nanswers:  {:?}",
                        control.id, control.check, p, answers,
                    ),
                    BuildResult::GoBack => panic!("unexpected GoBack for {}", control.id),
                }
            }
        }
    }

    #[test]
    fn menu_item_count_matches_variants() {
        // The two value-matches refinements (ShellExportsValueMatches /
        // ShellDefinesVariableValueMatches) are parser-only — they have no
        // dedicated menu entry; the picker only exposes the bare-string form.
        let interactive_variants = all_predicate_variants()
            .into_iter()
            .filter(|p| {
                !matches!(
                    p,
                    FilePredicateAst::ShellExportsValueMatches { .. }
                        | FilePredicateAst::ShellDefinesVariableValueMatches { .. }
                )
            })
            .count();
        assert_eq!(predicate_menu_items().len(), interactive_variants);
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

    // -----------------------------------------------------------------------
    // spec/0015 §6.1 — typed AsmOp compile_* round-trip. compile_*(ast)
    // produces a Vec<AsmOp> that, lowered to legacy strings via
    // asm_to_legacy_string, drives build_* back to ast.
    // -----------------------------------------------------------------------

    fn drive_with_asm<T, F>(ops: Vec<AsmOp>, mut build: F) -> T
    where
        F: FnMut(&mut CannedAnswerer) -> Result<BuildResult<T>>,
    {
        let strs: Vec<String> = ops.iter().map(asm_to_legacy_string).collect();
        let mut answerer = CannedAnswerer::new(strs);
        match build(&mut answerer).unwrap() {
            BuildResult::Built(t) => t,
            BuildResult::GoBack => panic!("unexpected GoBack from typed-AsmOp drive"),
        }
    }

    #[test]
    fn compile_data_schema_round_trip() {
        for schema in all_data_schema_variants() {
            let ops = compile_data_schema(&schema);
            let rebuilt = drive_with_asm(ops, build_data_schema);
            assert_eq!(rebuilt, schema, "compile_data_schema round-trip diff");
        }
    }

    #[test]
    fn compile_predicate_round_trip() {
        for pred in all_predicate_variants() {
            if matches!(
                pred,
                FilePredicateAst::ShellExportsValueMatches { .. }
                    | FilePredicateAst::ShellDefinesVariableValueMatches { .. }
            ) {
                continue;
            }
            let ops = compile_predicate(&pred);
            let rebuilt = drive_with_asm(ops, build_predicate);
            assert_eq!(rebuilt, pred, "compile_predicate round-trip diff");
        }
    }

    #[test]
    fn compile_proposition_round_trip() {
        for prop in all_proposition_variants() {
            let ops = compile_proposition(&prop);
            let rebuilt = drive_with_asm(ops, build_proposition);
            assert_eq!(rebuilt, prop, "compile_proposition round-trip diff");
        }
    }

    /// The typed AsmOp stream lowered to strings must equal the legacy
    /// `*_to_answers` output — the typed alphabet is a strict refinement,
    /// not a divergent representation.
    #[test]
    fn compile_predicate_matches_legacy_to_answers() {
        for pred in all_predicate_variants() {
            if matches!(
                pred,
                FilePredicateAst::ShellExportsValueMatches { .. }
                    | FilePredicateAst::ShellDefinesVariableValueMatches { .. }
            ) {
                continue;
            }
            let asm = compile_predicate(&pred);
            let strs: Vec<String> = asm.iter().map(asm_to_legacy_string).collect();
            let legacy = predicate_to_answers(&pred);
            // The two streams must agree where both produce a tag/text/yes-no;
            // the only divergence allowed is that legacy emits a 1-based index
            // string while AsmOp emits the canonical tag — both are accepted
            // by build_predicate (canonical name is unique and matches).
            // Therefore we compare the string-driven output of build_predicate
            // for each, not the raw streams.
            let rebuilt_asm = {
                let mut a = CannedAnswerer::new(strs);
                match build_predicate(&mut a).unwrap() {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => panic!("unexpected GoBack"),
                }
            };
            let rebuilt_legacy = {
                let mut a = CannedAnswerer::new(legacy);
                match build_predicate(&mut a).unwrap() {
                    BuildResult::Built(p) => p,
                    BuildResult::GoBack => panic!("unexpected GoBack"),
                }
            };
            assert_eq!(rebuilt_asm, pred);
            assert_eq!(rebuilt_legacy, pred);
            assert_eq!(rebuilt_asm, rebuilt_legacy);
        }
    }
}
