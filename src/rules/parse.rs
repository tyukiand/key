use anyhow::{anyhow, bail, Context, Result};
use serde_yaml::{Mapping, Value};

use crate::rules::ast::{DataArrayCheck, DataSchema, FilePredicateAst, Proposition, SimplePath};

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
            "is-null" => Ok(DataSchema::IsNull),
            _ => bail!(
                "Unknown data schema keyword: {:?}. Valid bare keywords: \
                 anything, is-string, is-number, is-bool, is-null. \
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

/// Parse a single key-value pair as a FilePredicateAst.
fn parse_predicate_kv(key: &str, value: &Value) -> Result<FilePredicateAst> {
    match key {
        "file-exists" => Ok(FilePredicateAst::FileExists),
        "text-matches" => {
            let s = require_string(value, "text-matches")?;
            Ok(FilePredicateAst::TextMatchesRegex(s))
        }
        "text-has-lines" => {
            let m = value
                .as_mapping()
                .ok_or_else(|| anyhow!("text-has-lines requires a mapping with min/max"))?;
            let min = mget(m, "min").and_then(|v| v.as_u64()).map(|n| n as u32);
            let max = mget(m, "max").and_then(|v| v.as_u64()).map(|n| n as u32);
            Ok(FilePredicateAst::TextHasLines { min, max })
        }
        "shell-exports" => {
            let s = require_string(value, "shell-exports")?;
            Ok(FilePredicateAst::ShellExports(s))
        }
        "shell-defines" => {
            let s = require_string(value, "shell-defines")?;
            Ok(FilePredicateAst::ShellDefinesVariable(s))
        }
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
        _ => bail!(
            "Unknown predicate key: {:?}. Valid keys: file-exists, text-matches, \
             text-has-lines, shell-exports, shell-defines, shell-adds-to-path, \
             properties-defines-key, xml-matches, json-matches, yaml-matches, all, any",
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
        _ => bail!(
            "Unknown proposition key: {:?}. Valid keys: file, forall, exists, all, any",
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
}
