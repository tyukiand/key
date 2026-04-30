use anyhow::{anyhow, Result};

use crate::rules::ast::DataSchema;

/// Check if an XML document contains an element at the given slash-separated path.
/// Path example: "settings/servers/server/token"
pub fn xml_has_path(content: &str, path: &str) -> Result<bool> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Ok(true);
    }

    let mut reader = Reader::from_str(content);
    let mut depth_stack: Vec<String> = Vec::new();
    let mut max_matched = 0;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                depth_stack.push(name.clone());

                // Check if current stack matches the path prefix
                if depth_stack.len() <= segments.len() {
                    let matches = depth_stack.iter().zip(segments.iter()).all(|(a, b)| a == b);
                    if matches {
                        max_matched = max_matched.max(depth_stack.len());
                        if depth_stack.len() == segments.len() {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                depth_stack.pop();
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                depth_stack.push(name);
                if depth_stack.len() <= segments.len() {
                    let matches = depth_stack.iter().zip(segments.iter()).all(|(a, b)| a == b);
                    if matches && depth_stack.len() == segments.len() {
                        return Ok(true);
                    }
                }
                depth_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("XML parse error: {}", e)),
            _ => {}
        }
    }

    Ok(false)
}

/// Evaluate a DataSchema against a JSON string.
pub fn evaluate_data_schema_json_str(schema: &DataSchema, content: &str) -> Result<(), String> {
    let root: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("JSON parse error: {}", e))?;
    evaluate_data_schema(schema, &root)
}

/// Evaluate a DataSchema against a YAML string.
pub fn evaluate_data_schema_yaml_str(schema: &DataSchema, content: &str) -> Result<(), String> {
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| format!("YAML parse error: {}", e))?;
    let json_value = yaml_to_json(&yaml_value)?;
    evaluate_data_schema(schema, &json_value)
}

/// Evaluate a DataSchema against a serde_json::Value.
fn evaluate_data_schema(schema: &DataSchema, value: &serde_json::Value) -> Result<(), String> {
    match schema {
        DataSchema::Anything => Ok(()),
        DataSchema::IsString => {
            if value.is_string() {
                Ok(())
            } else {
                Err(format!("expected string, got {}", json_type_name(value)))
            }
        }
        DataSchema::IsStringMatching(pattern) => {
            if let Some(s) = value.as_str() {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| format!("invalid regex {:?}: {}", pattern, e))?;
                if re.is_match(s) {
                    Ok(())
                } else {
                    Err(format!("string {:?} does not match regex {:?}", s, pattern))
                }
            } else {
                Err(format!("expected string, got {}", json_type_name(value)))
            }
        }
        DataSchema::IsNumber => {
            if value.is_number() {
                Ok(())
            } else {
                Err(format!("expected number, got {}", json_type_name(value)))
            }
        }
        DataSchema::IsBool => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(format!("expected bool, got {}", json_type_name(value)))
            }
        }
        // Spec/0013 §A.7B — strict equality with the JSON boolean true/false.
        DataSchema::IsTrue => match value.as_bool() {
            Some(true) => Ok(()),
            Some(false) => Err("expected `true`, got `false`".into()),
            None => Err(format!(
                "expected `true`, got {} ({})",
                json_type_name(value),
                value
            )),
        },
        DataSchema::IsFalse => match value.as_bool() {
            Some(false) => Ok(()),
            Some(true) => Err("expected `false`, got `true`".into()),
            None => Err(format!(
                "expected `false`, got {} ({})",
                json_type_name(value),
                value
            )),
        },
        DataSchema::IsNull => {
            if value.is_null() {
                Ok(())
            } else {
                Err(format!("expected null, got {}", json_type_name(value)))
            }
        }
        DataSchema::IsObject(entries) => {
            if let Some(obj) = value.as_object() {
                for (key, sub_schema) in entries {
                    match obj.get(key) {
                        Some(sub_value) => {
                            evaluate_data_schema(sub_schema, sub_value)
                                .map_err(|e| format!("at key {:?}: {}", key, e))?;
                        }
                        None => return Err(format!("missing key {:?}", key)),
                    }
                }
                Ok(())
            } else {
                Err(format!("expected object, got {}", json_type_name(value)))
            }
        }
        DataSchema::IsArray(check) => {
            if let Some(arr) = value.as_array() {
                if let Some(ref forall_schema) = check.forall {
                    for (i, elem) in arr.iter().enumerate() {
                        evaluate_data_schema(forall_schema, elem)
                            .map_err(|e| format!("at index {}: {}", i, e))?;
                    }
                }
                if let Some(ref exists_schema) = check.exists {
                    let any_match = arr
                        .iter()
                        .any(|elem| evaluate_data_schema(exists_schema, elem).is_ok());
                    if !any_match {
                        return Err("no array element matches the exists schema".into());
                    }
                }
                for (idx, sub_schema) in &check.at {
                    match arr.get(*idx as usize) {
                        Some(elem) => {
                            evaluate_data_schema(sub_schema, elem)
                                .map_err(|e| format!("at index {}: {}", idx, e))?;
                        }
                        None => {
                            return Err(format!(
                                "array index {} out of bounds (length {})",
                                idx,
                                arr.len()
                            ))
                        }
                    }
                }
                Ok(())
            } else {
                Err(format!("expected array, got {}", json_type_name(value)))
            }
        }
    }
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Convert a serde_yaml::Value to a serde_json::Value.
fn yaml_to_json(v: &serde_yaml::Value) -> Result<serde_json::Value, String> {
    match v {
        serde_yaml::Value::Null => Ok(serde_json::Value::Null),
        serde_yaml::Value::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(serde_json::Value::Number(i.into()))
            } else if let Some(u) = n.as_u64() {
                Ok(serde_json::Value::Number(u.into()))
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| "cannot convert YAML number to JSON".into())
            } else {
                Err("unknown YAML number type".into())
            }
        }
        serde_yaml::Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        serde_yaml::Value::Sequence(seq) => {
            let arr: Result<Vec<_>, _> = seq.iter().map(yaml_to_json).collect();
            Ok(serde_json::Value::Array(arr?))
        }
        serde_yaml::Value::Mapping(m) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m.iter() {
                let key = match k {
                    serde_yaml::Value::String(s) => s.clone(),
                    other => format!("{:?}", other),
                };
                map.insert(key, yaml_to_json(v)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        _ => Err("unsupported YAML value type".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::DataArrayCheck;

    #[test]
    fn xml_simple_path() {
        let xml = r#"<settings><servers><server><token>abc</token></server></servers></settings>"#;
        assert!(xml_has_path(xml, "settings/servers/server/token").unwrap());
        assert!(!xml_has_path(xml, "settings/servers/server/missing").unwrap());
        assert!(xml_has_path(xml, "settings").unwrap());
    }

    #[test]
    fn xml_empty_element() {
        let xml = r#"<root><empty/></root>"#;
        assert!(xml_has_path(xml, "root/empty").unwrap());
    }

    #[test]
    fn schema_anything() {
        assert!(evaluate_data_schema_json_str(&DataSchema::Anything, "42").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::Anything, "\"hello\"").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::Anything, "null").is_ok());
    }

    #[test]
    fn schema_is_string() {
        assert!(evaluate_data_schema_json_str(&DataSchema::IsString, "\"hello\"").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::IsString, "42").is_err());
    }

    #[test]
    fn schema_is_string_matching() {
        let schema = DataSchema::IsStringMatching("^hello".into());
        assert!(evaluate_data_schema_json_str(&schema, "\"hello world\"").is_ok());
        assert!(evaluate_data_schema_json_str(&schema, "\"goodbye\"").is_err());
        assert!(evaluate_data_schema_json_str(&schema, "42").is_err());
    }

    #[test]
    fn schema_is_number() {
        assert!(evaluate_data_schema_json_str(&DataSchema::IsNumber, "42").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::IsNumber, "\"hello\"").is_err());
    }

    #[test]
    fn schema_is_bool() {
        assert!(evaluate_data_schema_json_str(&DataSchema::IsBool, "true").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::IsBool, "42").is_err());
    }

    /// Spec/0013 §A.7B.5 — `is-true` PASSes on JSON `true`, FAILs on `false`,
    /// FAILs on non-bool. `is-bool` continues to accept both for regression.
    #[test]
    fn schema_is_true_strict() {
        assert!(evaluate_data_schema_json_str(&DataSchema::IsTrue, "true").is_ok());
        let err = evaluate_data_schema_json_str(&DataSchema::IsTrue, "false").unwrap_err();
        assert!(err.contains("expected `true`"), "got {}", err);
        let err = evaluate_data_schema_json_str(&DataSchema::IsTrue, "42").unwrap_err();
        assert!(err.contains("expected `true`"), "got {}", err);
        // Regression: is-bool still passes on both.
        assert!(evaluate_data_schema_json_str(&DataSchema::IsBool, "false").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::IsBool, "true").is_ok());
    }

    #[test]
    fn schema_is_false_strict() {
        assert!(evaluate_data_schema_json_str(&DataSchema::IsFalse, "false").is_ok());
        let err = evaluate_data_schema_json_str(&DataSchema::IsFalse, "true").unwrap_err();
        assert!(err.contains("expected `false`"), "got {}", err);
    }

    /// Spec/0013 §A.7B.5 (executable found scenario) — assert is-true behavior
    /// against an `<executable:NAME>` snapshot's `.found` field shape.
    #[test]
    fn schema_is_true_against_executable_snapshot_shape() {
        let schema = DataSchema::IsObject(vec![("found".into(), DataSchema::IsTrue)]);
        // Found = true: PASS
        assert!(
            evaluate_data_schema_json_str(&schema, r#"{"found": true, "name": "docker"}"#).is_ok()
        );
        // Found = false: FAIL with a message naming the path AND value.
        let err = evaluate_data_schema_json_str(&schema, r#"{"found": false, "name": "docker"}"#)
            .unwrap_err();
        assert!(err.contains("found"), "expected path in error; got {}", err);
        assert!(
            err.contains("expected `true`"),
            "expected actual-value mention; got {}",
            err
        );
    }

    #[test]
    fn schema_is_null() {
        assert!(evaluate_data_schema_json_str(&DataSchema::IsNull, "null").is_ok());
        assert!(evaluate_data_schema_json_str(&DataSchema::IsNull, "42").is_err());
    }

    #[test]
    fn schema_is_object() {
        let schema = DataSchema::IsObject(vec![
            ("name".into(), DataSchema::IsString),
            ("age".into(), DataSchema::IsNumber),
        ]);
        assert!(evaluate_data_schema_json_str(&schema, r#"{"name":"alice","age":30}"#).is_ok());
        // Extra keys are allowed
        assert!(evaluate_data_schema_json_str(
            &schema,
            r#"{"name":"alice","age":30,"extra":true}"#
        )
        .is_ok());
        // Missing key
        assert!(evaluate_data_schema_json_str(&schema, r#"{"name":"alice"}"#).is_err());
        // Wrong type
        assert!(
            evaluate_data_schema_json_str(&schema, r#"{"name":"alice","age":"thirty"}"#).is_err()
        );
    }

    #[test]
    fn schema_is_array_forall() {
        let schema = DataSchema::IsArray(DataArrayCheck {
            forall: Some(Box::new(DataSchema::IsString)),
            exists: None,
            at: vec![],
        });
        assert!(evaluate_data_schema_json_str(&schema, r#"["a","b","c"]"#).is_ok());
        assert!(evaluate_data_schema_json_str(&schema, r#"["a",42]"#).is_err());
        assert!(evaluate_data_schema_json_str(&schema, r#"[]"#).is_ok()); // vacuously true
    }

    #[test]
    fn schema_is_array_exists() {
        let schema = DataSchema::IsArray(DataArrayCheck {
            forall: None,
            exists: Some(Box::new(DataSchema::IsNumber)),
            at: vec![],
        });
        assert!(evaluate_data_schema_json_str(&schema, r#"["a",42,"b"]"#).is_ok());
        assert!(evaluate_data_schema_json_str(&schema, r#"["a","b"]"#).is_err());
    }

    #[test]
    fn schema_is_array_at() {
        let schema = DataSchema::IsArray(DataArrayCheck {
            forall: None,
            exists: None,
            at: vec![(0, DataSchema::IsString), (2, DataSchema::IsNumber)],
        });
        assert!(evaluate_data_schema_json_str(&schema, r#"["a",true,42]"#).is_ok());
        assert!(evaluate_data_schema_json_str(&schema, r#"[42,true,42]"#).is_err()); // index 0 not string
        assert!(evaluate_data_schema_json_str(&schema, r#"["a"]"#).is_err()); // index 2 out of bounds
    }

    #[test]
    fn schema_nested_object() {
        let schema = DataSchema::IsObject(vec![(
            "user".into(),
            DataSchema::IsObject(vec![
                ("name".into(), DataSchema::IsString),
                (
                    "tags".into(),
                    DataSchema::IsArray(DataArrayCheck {
                        forall: Some(Box::new(DataSchema::IsString)),
                        exists: None,
                        at: vec![],
                    }),
                ),
            ]),
        )]);
        let json = r#"{"user":{"name":"alice","tags":["admin","dev"]}}"#;
        assert!(evaluate_data_schema_json_str(&schema, json).is_ok());

        let bad_json = r#"{"user":{"name":"alice","tags":["admin",42]}}"#;
        assert!(evaluate_data_schema_json_str(&schema, bad_json).is_err());
    }

    #[test]
    fn schema_yaml_evaluation() {
        let schema = DataSchema::IsObject(vec![
            ("name".into(), DataSchema::IsString),
            ("count".into(), DataSchema::IsNumber),
        ]);
        let yaml = "name: alice\ncount: 42\n";
        assert!(evaluate_data_schema_yaml_str(&schema, yaml).is_ok());

        let bad_yaml = "name: alice\ncount: not-a-number\n";
        assert!(evaluate_data_schema_yaml_str(&schema, bad_yaml).is_err());
    }
}
