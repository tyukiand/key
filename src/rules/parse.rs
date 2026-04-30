use anyhow::{anyhow, bail, Context, Result};
use serde_yaml::{Mapping, Value};

use crate::rules::ast::{
    Control, ControlFile, DataArrayCheck, DataSchema, FailExpectation, FilePredicateAst,
    Proposition, SimplePath, TestCase, TestExpectation, TestFile, TestSuite,
};

fn mget<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
    m.get(Value::String(key.to_string()))
}

fn require_string(value: &Value, context: &str) -> Result<String> {
    value
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("{} requires a string value, got {:?}", context, value))
}

fn parse_simple_path(value: &Value) -> Result<SimplePath> {
    let s = value
        .as_str()
        .ok_or_else(|| anyhow!("expected a path string like ~/..., got {:?}", value))?;
    SimplePath::new(s)
}

fn parse_file_list(value: &Value) -> Result<Vec<SimplePath>> {
    let seq = value
        .as_sequence()
        .ok_or_else(|| anyhow!("'files' must be a list of paths, got {:?}", value))?;
    seq.iter().map(parse_simple_path).collect()
}

/// Parse a serde_yaml::Value as a DataSchema.
pub fn parse_data_schema(value: &Value) -> Result<DataSchema> {
    match value {
        Value::String(s) => match s.as_str() {
            "anything" => Ok(DataSchema::Anything),
            "is-string" => Ok(DataSchema::IsString),
            "is-number" => Ok(DataSchema::IsNumber),
            "is-bool" => Ok(DataSchema::IsBool),
            // Spec/0013 §A.7B — strict boolean equality.
            "is-true" => Ok(DataSchema::IsTrue),
            "is-false" => Ok(DataSchema::IsFalse),
            "is-null" => Ok(DataSchema::IsNull),
            _ => bail!(
                "Unknown data schema keyword: {:?}. Valid bare keywords: \
                 anything, is-string, is-number, is-bool, is-true, is-false, is-null. \
                 For structured checks use a mapping (is-string-matching, is-object, is-array).",
                s
            ),
        },
        Value::Mapping(m) => {
            if m.len() != 1 {
                bail!(
                    "Data schema mapping must have exactly one key, got {}",
                    m.len()
                );
            }
            let (k, v) = m.iter().next().unwrap();
            let key = k
                .as_str()
                .ok_or_else(|| anyhow!("Data schema key must be a string, got {:?}", k))?;
            match key {
                "is-string-matching" => {
                    let regex = require_string(v, "is-string-matching")?;
                    Ok(DataSchema::IsStringMatching(regex))
                }
                "is-object" => {
                    let obj = v
                        .as_mapping()
                        .ok_or_else(|| anyhow!("is-object requires a mapping of key: schema"))?;
                    let mut entries = Vec::new();
                    for (ok, ov) in obj.iter() {
                        let key_name = ok.as_str().ok_or_else(|| {
                            anyhow!("is-object key must be a string, got {:?}", ok)
                        })?;
                        let sub = parse_data_schema(ov)
                            .with_context(|| format!("in is-object key {:?}", key_name))?;
                        entries.push((key_name.to_string(), sub));
                    }
                    Ok(DataSchema::IsObject(entries))
                }
                "is-array" => {
                    let arr_m = v.as_mapping().ok_or_else(|| {
                        anyhow!("is-array requires a mapping with forall/exists/at")
                    })?;
                    let forall = if let Some(fv) = mget(arr_m, "forall") {
                        Some(Box::new(
                            parse_data_schema(fv).context("in is-array forall")?,
                        ))
                    } else {
                        None
                    };
                    let exists = if let Some(ev) = mget(arr_m, "exists") {
                        Some(Box::new(
                            parse_data_schema(ev).context("in is-array exists")?,
                        ))
                    } else {
                        None
                    };
                    let mut at = Vec::new();
                    if let Some(at_v) = mget(arr_m, "at") {
                        let at_m = at_v.as_mapping().ok_or_else(|| {
                            anyhow!("is-array 'at' must be a mapping of index: schema")
                        })?;
                        for (ak, av) in at_m.iter() {
                            let idx = match ak {
                                Value::Number(n) => n.as_u64().ok_or_else(|| {
                                    anyhow!("array index must be a non-negative integer")
                                })? as u32,
                                Value::String(s) => s.parse::<u32>().map_err(|_| {
                                    anyhow!(
                                        "array index must be a non-negative integer, got {:?}",
                                        s
                                    )
                                })?,
                                _ => bail!("array index must be a number, got {:?}", ak),
                            };
                            at.push((
                                idx,
                                parse_data_schema(av)
                                    .with_context(|| format!("in is-array at index {}", idx))?,
                            ));
                        }
                    }
                    Ok(DataSchema::IsArray(DataArrayCheck { forall, exists, at }))
                }
                _ => bail!(
                    "Unknown data schema key: {:?}. Valid keys: \
                     is-string-matching, is-object, is-array",
                    key
                ),
            }
        }
        _ => bail!(
            "Expected a data schema (string keyword or mapping), got {:?}",
            value
        ),
    }
}

/// Parse the value of `shell-exports` / `shell-defines`. Bare-string form
/// preserves existing semantics; mapping form `{ name, value-matches }` adds
/// a regex constraint on the rhs (spec/0010 §6.X.2).
fn parse_shell_var_predicate(
    value: &Value,
    key: &str,
    require_export: bool,
) -> Result<FilePredicateAst> {
    match value {
        Value::String(s) => Ok(if require_export {
            FilePredicateAst::ShellExports(s.clone())
        } else {
            FilePredicateAst::ShellDefinesVariable(s.clone())
        }),
        Value::Mapping(m) => {
            let mut name: Option<String> = None;
            let mut value_regex: Option<String> = None;
            for (k, v) in m.iter() {
                let k_str = k
                    .as_str()
                    .ok_or_else(|| anyhow!("{} mapping key must be a string, got {:?}", key, k))?;
                match k_str {
                    "name" => name = Some(require_string(v, &format!("{}.name", key))?),
                    "value-matches" => {
                        value_regex = Some(require_string(v, &format!("{}.value-matches", key))?)
                    }
                    other => bail!(
                        "Unknown key {:?} in {} mapping. Valid keys: name, value-matches",
                        other,
                        key
                    ),
                }
            }
            let name =
                name.ok_or_else(|| anyhow!("{} mapping form requires a 'name' field", key))?;
            let value_regex = value_regex.ok_or_else(|| {
                anyhow!(
                    "{} mapping form requires a 'value-matches' field (use the bare-string form for existence-only)",
                    key
                )
            })?;
            Ok(if require_export {
                FilePredicateAst::ShellExportsValueMatches { name, value_regex }
            } else {
                FilePredicateAst::ShellDefinesVariableValueMatches { name, value_regex }
            })
        }
        _ => bail!(
            "{} requires a string (variable name) or mapping with name + value-matches, got {:?}",
            key,
            value
        ),
    }
}

/// Parse a single key-value pair as a FilePredicateAst.
fn parse_predicate_kv(key: &str, value: &Value) -> Result<FilePredicateAst> {
    match key {
        "file-exists" => Ok(FilePredicateAst::FileExists),
        "text-matches" => {
            let s = require_string(value, "text-matches")?;
            Ok(FilePredicateAst::TextMatchesRegex(s))
        }
        "text-contains" => {
            let s = require_string(value, "text-contains")?;
            Ok(FilePredicateAst::TextContains(s))
        }
        "text-has-lines" => {
            let m = value
                .as_mapping()
                .ok_or_else(|| anyhow!("text-has-lines requires a mapping with min/max"))?;
            let min = mget(m, "min").and_then(|v| v.as_u64()).map(|n| n as u32);
            let max = mget(m, "max").and_then(|v| v.as_u64()).map(|n| n as u32);
            Ok(FilePredicateAst::TextHasLines { min, max })
        }
        "shell-exports" => parse_shell_var_predicate(value, "shell-exports", true),
        "shell-defines" => parse_shell_var_predicate(value, "shell-defines", false),
        "shell-adds-to-path" => {
            let s = require_string(value, "shell-adds-to-path")?;
            Ok(FilePredicateAst::ShellAddsToPath(s))
        }
        "properties-defines-key" => {
            let s = require_string(value, "properties-defines-key")?;
            Ok(FilePredicateAst::PropertiesDefinesKey(s))
        }
        "xml-matches" => {
            let s = require_string(value, "xml-matches")?;
            Ok(FilePredicateAst::XmlMatchesPath(s))
        }
        "json-matches" => {
            let schema = parse_data_schema(value).context("parsing json-matches data schema")?;
            Ok(FilePredicateAst::JsonMatches(schema))
        }
        "yaml-matches" => {
            let schema = parse_data_schema(value).context("parsing yaml-matches data schema")?;
            Ok(FilePredicateAst::YamlMatches(schema))
        }
        "all" => {
            let seq = value
                .as_sequence()
                .ok_or_else(|| anyhow!("'all' (predicate) requires a list of predicates"))?;
            let preds: Result<Vec<_>> = seq.iter().map(parse_predicate).collect();
            Ok(FilePredicateAst::All(preds?))
        }
        "any" => {
            let m = value.as_mapping().ok_or_else(|| {
                anyhow!("'any' (predicate) requires a mapping with 'hint' and 'checks'")
            })?;
            let hint = mget(m, "hint")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("'any' (predicate) requires a 'hint' string"))?
                .to_string();
            let checks_val = mget(m, "checks")
                .ok_or_else(|| anyhow!("'any' (predicate) requires a 'checks' list"))?;
            let checks_seq = checks_val
                .as_sequence()
                .ok_or_else(|| anyhow!("'any.checks' must be a list of predicates"))?;
            let checks: Result<Vec<_>> = checks_seq.iter().map(parse_predicate).collect();
            Ok(FilePredicateAst::Any {
                hint,
                checks: checks?,
            })
        }
        "not" => {
            let inner = parse_predicate(value).context("parsing 'not' predicate")?;
            Ok(FilePredicateAst::Not(Box::new(inner)))
        }
        "conditionally" => {
            let m = value.as_mapping().ok_or_else(|| {
                anyhow!("'conditionally' (predicate) requires a mapping with 'if' and 'then'")
            })?;
            let cond_val = mget(m, "if")
                .ok_or_else(|| anyhow!("'conditionally' (predicate) requires an 'if' field"))?;
            let then_val = mget(m, "then")
                .ok_or_else(|| anyhow!("'conditionally' (predicate) requires a 'then' field"))?;
            let condition = parse_predicate(cond_val).context("parsing 'conditionally.if'")?;
            let then = parse_predicate(then_val).context("parsing 'conditionally.then'")?;
            Ok(FilePredicateAst::Conditionally {
                condition: Box::new(condition),
                then: Box::new(then),
            })
        }
        _ => bail!(
            "Unknown predicate key: {:?}. Valid keys: file-exists, text-matches, \
             text-contains, text-has-lines, shell-exports, shell-defines, shell-adds-to-path, \
             properties-defines-key, xml-matches, json-matches, yaml-matches, all, any, \
             not, conditionally",
            key,
        ),
    }
}

/// Parse a serde_yaml::Value as a FilePredicateAst.
pub fn parse_predicate(value: &Value) -> Result<FilePredicateAst> {
    match value {
        Value::String(s) => {
            if s == "file-exists" {
                Ok(FilePredicateAst::FileExists)
            } else {
                bail!(
                    "Unknown bare predicate string: {:?}. Only 'file-exists' can appear as a bare string.",
                    s
                )
            }
        }
        Value::Mapping(m) => {
            if m.is_empty() {
                bail!("Empty mapping is not a valid predicate");
            }
            if m.len() == 1 {
                let (k, v) = m.iter().next().unwrap();
                let key = k
                    .as_str()
                    .ok_or_else(|| anyhow!("Predicate key must be a string, got {:?}", k))?;
                parse_predicate_kv(key, v)
            } else {
                // Multi-key mapping: implicit All
                let mut preds = Vec::new();
                for (k, v) in m.iter() {
                    let key = k
                        .as_str()
                        .ok_or_else(|| anyhow!("Predicate key must be a string, got {:?}", k))?;
                    preds.push(parse_predicate_kv(key, v)?);
                }
                Ok(FilePredicateAst::All(preds))
            }
        }
        _ => bail!("Expected a predicate (string or mapping), got {:?}", value),
    }
}

/// Parse the `check:` field value as a FilePredicateAst.
pub fn parse_check(value: &Value) -> Result<FilePredicateAst> {
    parse_predicate(value).context("parsing 'check' field")
}

/// Parse a serde_yaml::Value as a Proposition.
pub fn parse_proposition(value: &Value) -> Result<Proposition> {
    let m = value.as_mapping().ok_or_else(|| {
        anyhow!(
            "A proposition must be a YAML mapping with one key \
             (file, forall, exists, all, any), got {:?}",
            value
        )
    })?;

    if m.len() != 1 {
        bail!(
            "A proposition must have exactly one key \
             (file, forall, exists, all, any), got {} keys",
            m.len()
        );
    }

    let (k, v) = m.iter().next().unwrap();
    let key = k
        .as_str()
        .ok_or_else(|| anyhow!("Proposition key must be a string, got {:?}", k))?;

    match key {
        "file" => {
            let inner = v.as_mapping().ok_or_else(|| {
                anyhow!("'file' proposition requires a mapping with 'path' and 'check'")
            })?;
            let path_val = mget(inner, "path")
                .ok_or_else(|| anyhow!("'file' proposition requires a 'path' field"))?;
            let path = parse_simple_path(path_val)?;
            let check_val = mget(inner, "check")
                .ok_or_else(|| anyhow!("'file' proposition requires a 'check' field"))?;
            let check = parse_check(check_val)?;
            Ok(Proposition::FileSatisfies { path, check })
        }
        "forall" => {
            let inner = v.as_mapping().ok_or_else(|| {
                anyhow!("'forall' proposition requires a mapping with 'files' and 'check'")
            })?;
            let files_val = mget(inner, "files")
                .ok_or_else(|| anyhow!("'forall' proposition requires a 'files' field"))?;
            let files = parse_file_list(files_val)?;
            let check_val = mget(inner, "check")
                .ok_or_else(|| anyhow!("'forall' proposition requires a 'check' field"))?;
            let check = parse_check(check_val)?;
            Ok(Proposition::Forall { files, check })
        }
        "exists" => {
            let inner = v.as_mapping().ok_or_else(|| {
                anyhow!("'exists' proposition requires a mapping with 'files' and 'check'")
            })?;
            let files_val = mget(inner, "files")
                .ok_or_else(|| anyhow!("'exists' proposition requires a 'files' field"))?;
            let files = parse_file_list(files_val)?;
            let check_val = mget(inner, "check")
                .ok_or_else(|| anyhow!("'exists' proposition requires a 'check' field"))?;
            let check = parse_check(check_val)?;
            Ok(Proposition::Exists { files, check })
        }
        "all" => {
            let seq = v
                .as_sequence()
                .ok_or_else(|| anyhow!("'all' proposition requires a list of sub-propositions"))?;
            let props: Result<Vec<_>> = seq.iter().map(parse_proposition).collect();
            Ok(Proposition::All(props?))
        }
        "any" => {
            let seq = v
                .as_sequence()
                .ok_or_else(|| anyhow!("'any' proposition requires a list of sub-propositions"))?;
            let props: Result<Vec<_>> = seq.iter().map(parse_proposition).collect();
            Ok(Proposition::Any(props?))
        }
        "not" => {
            let inner = parse_proposition(v).context("parsing 'not' proposition")?;
            Ok(Proposition::Not(Box::new(inner)))
        }
        "conditionally" => {
            let inner = v.as_mapping().ok_or_else(|| {
                anyhow!("'conditionally' proposition requires a mapping with 'if' and 'then'")
            })?;
            let cond_val = mget(inner, "if")
                .ok_or_else(|| anyhow!("'conditionally' proposition requires an 'if' field"))?;
            let then_val = mget(inner, "then")
                .ok_or_else(|| anyhow!("'conditionally' proposition requires a 'then' field"))?;
            let condition = parse_proposition(cond_val).context("parsing 'conditionally.if'")?;
            let then = parse_proposition(then_val).context("parsing 'conditionally.then'")?;
            Ok(Proposition::Conditionally {
                condition: Box::new(condition),
                then: Box::new(then),
            })
        }
        _ => bail!(
            "Unknown proposition key: {:?}. Valid keys: file, forall, exists, all, any, \
             not, conditionally",
            key,
        ),
    }
}

#[cfg(test)]
pub fn parse_data_schema_from_str(yaml: &str) -> Result<DataSchema> {
    let value: Value = serde_yaml::from_str(yaml).context("Invalid YAML")?;
    parse_data_schema(&value)
}

#[cfg(test)]
pub fn parse_predicate_from_str(yaml: &str) -> Result<FilePredicateAst> {
    let value: Value = serde_yaml::from_str(yaml).context("Invalid YAML")?;
    parse_predicate(&value)
}

pub fn parse_control(value: &Value) -> Result<Control> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("A control must be a YAML mapping"))?;
    let id = mget(m, "id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Control requires an 'id' string field"))?
        .to_string();
    crate::rules::ast::validate_control_id(&id)?;
    let title = mget(m, "title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Control requires a 'title' string field"))?
        .to_string();
    let description = mget(m, "description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Control requires a 'description' string field"))?
        .to_string();
    let remediation = mget(m, "remediation")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Control requires a 'remediation' string field"))?
        .to_string();
    let check_val = mget(m, "check")
        .ok_or_else(|| anyhow!("Control requires a 'check' field (proposition)"))?;
    let check = parse_proposition(check_val).context("parsing control check")?;
    Ok(Control {
        id,
        title,
        description,
        remediation,
        check,
    })
}

pub fn parse_control_file(yaml: &str) -> Result<ControlFile> {
    let value: Value = serde_yaml::from_str(yaml).context("Invalid YAML")?;
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("Control file must be a YAML mapping with a 'controls' key"))?;
    let controls_val =
        mget(m, "controls").ok_or_else(|| anyhow!("Control file must have a 'controls' key"))?;
    let seq = controls_val
        .as_sequence()
        .ok_or_else(|| anyhow!("'controls' must be a list"))?;
    let mut controls = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (i, item) in seq.iter().enumerate() {
        let control =
            parse_control(item).with_context(|| format!("parsing control at index {}", i))?;
        if !seen_ids.insert(control.id.clone()) {
            bail!("Duplicate control ID: {:?}", control.id);
        }
        controls.push(control);
    }
    Ok(ControlFile { controls })
}

// ---------------------------------------------------------------------------
// TestAst parsing
// ---------------------------------------------------------------------------

pub fn parse_test_expectation(value: &Value) -> Result<TestExpectation> {
    match value {
        Value::String(s) => match s.as_str() {
            "pass" => Ok(TestExpectation::Pass),
            "fail" => Ok(TestExpectation::Fail(FailExpectation {
                count: None,
                messages: vec![],
            })),
            _ => bail!(
                "Unknown test expectation: {:?}. Valid values: pass, fail",
                s
            ),
        },
        Value::Mapping(m) => {
            if m.len() != 1 {
                bail!(
                    "Test expectation mapping must have exactly one key, got {}",
                    m.len()
                );
            }
            let (k, v) = m.iter().next().unwrap();
            let key = k
                .as_str()
                .ok_or_else(|| anyhow!("Test expectation key must be a string, got {:?}", k))?;
            match key {
                "fail" => {
                    let inner = v.as_mapping().ok_or_else(|| {
                        anyhow!("fail expectation requires a mapping with optional count/messages")
                    })?;
                    let count = mget(inner, "count")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as usize);
                    let messages = if let Some(msgs_val) = mget(inner, "messages") {
                        let seq = msgs_val
                            .as_sequence()
                            .ok_or_else(|| anyhow!("'messages' must be a list of strings"))?;
                        seq.iter()
                            .map(|v| require_string(v, "messages item"))
                            .collect::<Result<Vec<_>>>()?
                    } else {
                        vec![]
                    };
                    Ok(TestExpectation::Fail(FailExpectation { count, messages }))
                }
                _ => bail!("Unknown test expectation key: {:?}. Valid keys: fail", key),
            }
        }
        _ => bail!(
            "Expected a test expectation (string or mapping), got {:?}",
            value
        ),
    }
}

/// Spec/0013 §A.6.2 — strict-key enforcement. The accepted keys for each
/// AST level. Anything else is rejected with the offending key named.
const TEST_CASE_KEYS: &[&str] = &["control-id", "description", "fixture", "expect"];
const TEST_SUITE_KEYS: &[&str] = &["name", "description", "tests"];
const TEST_FILE_KEYS: &[&str] = &["test-suites"];

fn check_unknown_keys(m: &serde_yaml::Mapping, valid: &[&str], context: &str) -> Result<()> {
    for (k, _) in m.iter() {
        let key = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if !valid.contains(&key) {
            bail!(
                "Unknown key {:?} in {}. Valid keys: {}.",
                key,
                context,
                valid
                    .iter()
                    .map(|k| format!("`{}`", k))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
    }
    Ok(())
}

pub fn parse_test_case(value: &Value) -> Result<TestCase> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("A test case must be a YAML mapping"))?;
    check_unknown_keys(m, TEST_CASE_KEYS, "test case")?;
    let control_id = mget(m, "control-id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Test case requires a 'control-id' string field"))?
        .to_string();
    let description = mget(m, "description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Test case requires a 'description' string field"))?
        .to_string();
    let fixture = mget(m, "fixture")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Test case requires a 'fixture' string field"))?
        .to_string();
    let expect_val =
        mget(m, "expect").ok_or_else(|| anyhow!("Test case requires an 'expect' field"))?;
    let expect = parse_test_expectation(expect_val).context("parsing test case expect")?;
    Ok(TestCase {
        control_id,
        description,
        fixture,
        expect,
    })
}

pub fn parse_test_suite(value: &Value) -> Result<TestSuite> {
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("A test suite must be a YAML mapping"))?;
    check_unknown_keys(m, TEST_SUITE_KEYS, "test suite")?;
    let name = mget(m, "name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Test suite requires a 'name' string field"))?
        .to_string();
    let description = mget(m, "description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let tests_val =
        mget(m, "tests").ok_or_else(|| anyhow!("Test suite requires a 'tests' list"))?;
    let seq = tests_val
        .as_sequence()
        .ok_or_else(|| anyhow!("'tests' must be a list"))?;
    let mut tests = Vec::new();
    for (i, item) in seq.iter().enumerate() {
        tests.push(
            parse_test_case(item).with_context(|| format!("parsing test case at index {}", i))?,
        );
    }
    Ok(TestSuite {
        name,
        description,
        tests,
    })
}

pub fn parse_test_file(yaml: &str) -> Result<TestFile> {
    let value: Value = serde_yaml::from_str(yaml).context("Invalid YAML")?;
    let m = value
        .as_mapping()
        .ok_or_else(|| anyhow!("Test file must be a YAML mapping with a 'test-suites' key"))?;
    check_unknown_keys(m, TEST_FILE_KEYS, "test file")?;
    let suites_val =
        mget(m, "test-suites").ok_or_else(|| anyhow!("Test file must have a 'test-suites' key"))?;
    let seq = suites_val
        .as_sequence()
        .ok_or_else(|| anyhow!("'test-suites' must be a list"))?;
    let mut test_suites = Vec::new();
    for (i, item) in seq.iter().enumerate() {
        test_suites.push(
            parse_test_suite(item).with_context(|| format!("parsing test suite at index {}", i))?,
        );
    }
    Ok(TestFile { test_suites })
}

#[cfg(test)]
pub fn parse_test_expectation_from_str(yaml: &str) -> Result<TestExpectation> {
    let value: Value = serde_yaml::from_str(yaml).context("Invalid YAML")?;
    parse_test_expectation(&value)
}

#[cfg(test)]
pub fn parse_proposition_from_str(yaml: &str) -> Result<Proposition> {
    let value: Value = serde_yaml::from_str(yaml).context("Invalid YAML")?;
    parse_proposition(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::*;
    use crate::rules::generate;

    #[test]
    fn roundtrip_predicate_variants() {
        for pred in all_predicate_variants() {
            let yaml_str = generate::generate_predicate_string(&pred);
            let parsed = parse_predicate_from_str(&yaml_str).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse generated YAML for {:?}: {}\nYAML:\n{}",
                    pred.yaml_key(),
                    e,
                    yaml_str
                )
            });
            assert_eq!(
                parsed,
                pred,
                "Roundtrip failed for {:?}\nYAML:\n{}",
                pred.yaml_key(),
                yaml_str
            );
        }
    }

    #[test]
    fn roundtrip_proposition_variants() {
        for prop in all_proposition_variants() {
            let yaml_str = generate::generate_proposition_string(&prop);
            let parsed = parse_proposition_from_str(&yaml_str).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse generated YAML for {:?}: {}\nYAML:\n{}",
                    prop.yaml_key(),
                    e,
                    yaml_str
                )
            });
            assert_eq!(
                parsed,
                prop,
                "Roundtrip failed for {:?}\nYAML:\n{}",
                prop.yaml_key(),
                yaml_str
            );
        }
    }

    #[test]
    fn roundtrip_data_schema_variants() {
        for schema in all_data_schema_variants() {
            let yaml_str = generate::generate_data_schema_string(&schema);
            let parsed = parse_data_schema_from_str(&yaml_str).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse generated YAML for DataSchema: {}\nYAML:\n{}",
                    e, yaml_str
                )
            });
            assert_eq!(
                parsed, schema,
                "Roundtrip failed for DataSchema\nYAML:\n{}",
                yaml_str
            );
        }
    }

    #[test]
    fn pretty_print_idempotence_data_schemas() {
        for schema in all_data_schema_variants() {
            let yaml1 = generate::generate_data_schema_string(&schema);
            let parsed = parse_data_schema_from_str(&yaml1).unwrap();
            let yaml2 = generate::generate_data_schema_string(&parsed);
            assert_eq!(yaml1, yaml2, "Idempotence failed for DataSchema");
        }
    }

    #[test]
    fn variant_count_matches_yaml_keys() {
        assert_eq!(all_predicate_variants().len(), PREDICATE_YAML_KEYS.len());
        assert_eq!(
            all_proposition_variants().len(),
            PROPOSITION_YAML_KEYS.len()
        );
    }

    #[test]
    fn pretty_print_idempotence_predicates() {
        for pred in all_predicate_variants() {
            let yaml1 = generate::generate_predicate_string(&pred);
            let parsed = parse_predicate_from_str(&yaml1).unwrap();
            let yaml2 = generate::generate_predicate_string(&parsed);
            assert_eq!(yaml1, yaml2, "Idempotence failed for {:?}", pred.yaml_key());
        }
    }

    #[test]
    fn pretty_print_idempotence_propositions() {
        for prop in all_proposition_variants() {
            let yaml1 = generate::generate_proposition_string(&prop);
            let parsed = parse_proposition_from_str(&yaml1).unwrap();
            let yaml2 = generate::generate_proposition_string(&parsed);
            assert_eq!(yaml1, yaml2, "Idempotence failed for {:?}", prop.yaml_key());
        }
    }

    #[test]
    fn parse_multi_key_check_as_implicit_all() {
        let yaml = r#"
file:
  path: ~/.bashrc
  check:
    shell-exports: JAVA_HOME
    shell-adds-to-path: JAVA_HOME_BIN
"#;
        let prop = parse_proposition_from_str(yaml).unwrap();
        match prop {
            Proposition::FileSatisfies {
                check: FilePredicateAst::All(preds),
                ..
            } => {
                assert_eq!(preds.len(), 2);
            }
            other => panic!("Expected FileSatisfies with All check, got {:?}", other),
        }
    }

    #[test]
    fn parse_error_unknown_predicate_key() {
        let yaml = "bogus-key: hello";
        let result = parse_predicate_from_str(yaml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown predicate key"),
            "error should mention unknown key"
        );
    }

    #[test]
    fn parse_error_unknown_proposition_key() {
        let yaml = "bogus: []";
        let result = parse_proposition_from_str(yaml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown proposition key"),
            "error should mention unknown key"
        );
    }

    #[test]
    fn parse_bare_file_exists() {
        let pred = parse_predicate_from_str("file-exists").unwrap();
        assert_eq!(pred, FilePredicateAst::FileExists);
    }

    #[test]
    fn parse_complex_proposition() {
        let yaml = r#"
all:
  - forall:
      files:
        - ~/.bashrc
        - ~/.zshrc
      check:
        shell-exports: JAVA_HOME
  - file:
      path: ~/.gradle/gradle.properties
      check:
        properties-defines-key: signing.keyId
"#;
        let prop = parse_proposition_from_str(yaml).unwrap();
        match prop {
            Proposition::All(items) => assert_eq!(items.len(), 2),
            other => panic!("Expected All, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_control_file() {
        use crate::rules::ast::{Control, ControlFile};
        let cf = ControlFile {
            controls: vec![
                Control {
                    id: "SSH-KEY".into(),
                    title: "SSH key exists".into(),
                    description: "Check SSH key".into(),
                    remediation: "Run ssh-keygen".into(),
                    check: Proposition::FileSatisfies {
                        path: SimplePath::new("~/.ssh/id_ed25519.pub").unwrap(),
                        check: FilePredicateAst::FileExists,
                    },
                },
                Control {
                    id: "JAVA-HOME".into(),
                    title: "JAVA_HOME is set".into(),
                    description: "Check JAVA_HOME".into(),
                    remediation: "Add export JAVA_HOME=...".into(),
                    check: Proposition::FileSatisfies {
                        path: SimplePath::new("~/.bashrc").unwrap(),
                        check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
                    },
                },
            ],
        };
        let yaml_str = generate::generate_control_file(&cf);
        let parsed = parse_control_file(&yaml_str).unwrap();
        assert_eq!(parsed, cf);
    }

    #[test]
    fn parse_control_file_duplicate_id() {
        let yaml = r#"
controls:
  - id: SAME
    title: First
    description: d
    remediation: r
    check:
      file:
        path: ~/a
        check: file-exists
  - id: SAME
    title: Second
    description: d
    remediation: r
    check:
      file:
        path: ~/b
        check: file-exists
"#;
        let result = parse_control_file(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate"));
    }

    #[test]
    fn parse_control_file_invalid_id() {
        let yaml = r#"
controls:
  - id: lower-case
    title: Bad
    description: d
    remediation: r
    check:
      file:
        path: ~/a
        check: file-exists
"#;
        let result = parse_control_file(yaml);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("Control ID must start with"),
            "unexpected error: {}",
            err_msg
        );
    }

    // --- TestAst round-trip tests (invariants 16-19) ---

    #[test]
    fn roundtrip_test_expectation_variants() {
        for exp in all_test_expectation_variants() {
            let yaml_str = generate::generate_test_expectation_string(&exp);
            let parsed = parse_test_expectation_from_str(&yaml_str).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse generated YAML for TestExpectation: {}\nYAML:\n{}",
                    e, yaml_str
                )
            });
            assert_eq!(
                parsed, exp,
                "Roundtrip failed for TestExpectation\nYAML:\n{}",
                yaml_str
            );
        }
    }

    #[test]
    fn idempotence_test_expectation() {
        for exp in all_test_expectation_variants() {
            let yaml1 = generate::generate_test_expectation_string(&exp);
            let parsed = parse_test_expectation_from_str(&yaml1).unwrap();
            let yaml2 = generate::generate_test_expectation_string(&parsed);
            assert_eq!(yaml1, yaml2, "Idempotence failed for TestExpectation");
        }
    }

    #[test]
    fn variant_count_matches_yaml_keys_test_expectation() {
        assert_eq!(
            all_test_expectation_variants().len(),
            TEST_EXPECTATION_YAML_KEYS.len()
        );
    }

    // ----- Spec/0010 §6.X.3 parser tests for shell-exports / shell-defines mapping form -----

    #[test]
    fn shell_exports_bare_string_form_parses() {
        let pred = parse_predicate_from_str("shell-exports: PATH").unwrap();
        assert_eq!(pred, FilePredicateAst::ShellExports("PATH".into()));
    }

    #[test]
    fn shell_exports_mapping_form_parses() {
        let yaml = r#"
shell-exports:
  name: PATH
  value-matches: "(^|:)/usr/bin(:|$)"
"#;
        let pred = parse_predicate_from_str(yaml).unwrap();
        assert_eq!(
            pred,
            FilePredicateAst::ShellExportsValueMatches {
                name: "PATH".into(),
                value_regex: "(^|:)/usr/bin(:|$)".into(),
            }
        );
    }

    #[test]
    fn shell_defines_mapping_form_parses() {
        let yaml = r#"
shell-defines:
  name: MY_VAR
  value-matches: "^/opt/.*"
"#;
        let pred = parse_predicate_from_str(yaml).unwrap();
        assert_eq!(
            pred,
            FilePredicateAst::ShellDefinesVariableValueMatches {
                name: "MY_VAR".into(),
                value_regex: "^/opt/.*".into(),
            }
        );
    }

    #[test]
    fn shell_exports_mapping_unknown_key_rejected() {
        let yaml = r#"
shell-exports:
  name: PATH
  bogus-key: nope
"#;
        let err = parse_predicate_from_str(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("Unknown key"),
            "error must mention 'Unknown key'; got: {}",
            msg
        );
        assert!(
            msg.contains("bogus-key"),
            "error must name the offending key; got: {}",
            msg
        );
    }

    #[test]
    fn shell_exports_mapping_missing_name_rejected() {
        let yaml = r#"
shell-exports:
  value-matches: "x"
"#;
        let err = parse_predicate_from_str(yaml).unwrap_err();
        assert!(format!("{:#}", err).contains("name"));
    }

    #[test]
    fn shell_exports_mapping_missing_value_matches_rejected() {
        let yaml = r#"
shell-exports:
  name: PATH
"#;
        let err = parse_predicate_from_str(yaml).unwrap_err();
        assert!(format!("{:#}", err).contains("value-matches"));
    }

    #[test]
    fn roundtrip_test_file() {
        use crate::rules::ast::{FailExpectation, TestCase, TestFile, TestSuite};
        let tf = TestFile {
            test_suites: vec![TestSuite {
                name: "SSH checks".into(),
                description: Some("Verify SSH setup".into()),
                tests: vec![
                    TestCase {
                        control_id: "CTRL-0001".into(),
                        description: "valid SSH key setup passes".into(),
                        fixture: "CTRL-0001-valid".into(),
                        expect: TestExpectation::Pass,
                    },
                    TestCase {
                        control_id: "CTRL-0001".into(),
                        description: "missing SSH key detected".into(),
                        fixture: "CTRL-0001-invalid".into(),
                        expect: TestExpectation::Fail(FailExpectation {
                            count: Some(1),
                            messages: vec!["does not exist".into()],
                        }),
                    },
                ],
            }],
        };
        let yaml_str = generate::generate_test_file(&tf);
        let parsed = parse_test_file(&yaml_str).unwrap();
        assert_eq!(parsed, tf);
    }

    /// Spec/0013 §A.6.2 — `expecet:` (typo of `expect:`) is rejected
    /// naming the offending key, NOT silently ignored.
    #[test]
    fn tests_yaml_typo_expecet_is_rejected() {
        let yaml = r#"
test-suites:
  - name: s
    tests:
      - control-id: FOO
        description: bar
        fixture: x
        expect: pass
        expecet: fail
"#;
        let err = parse_test_file(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("expecet"),
            "expected error naming the typo `expecet`; got {}",
            msg
        );
        assert!(
            msg.contains("test case"),
            "expected error to identify the test case context; got {}",
            msg
        );
    }

    /// Spec/0013 §A.6.2 — unknown top-level key in tests.yaml rejected.
    #[test]
    fn tests_yaml_unknown_top_level_key_is_rejected() {
        let yaml = r#"
test-suites: []
something-else: 1
"#;
        let err = parse_test_file(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("something-else"));
    }

    /// Spec/0013 §A.6.2 — unknown key inside a test suite rejected.
    #[test]
    fn tests_yaml_unknown_key_in_suite_is_rejected() {
        let yaml = r#"
test-suites:
  - name: s
    tests: []
    bogus: 1
"#;
        let err = parse_test_file(yaml).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("bogus"));
    }
}
