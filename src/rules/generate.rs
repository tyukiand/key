use crate::rules::ast::{DataSchema, FilePredicateAst, Proposition};
use serde_yaml::{Mapping, Value};

fn mk(key: &str, value: Value) -> Value {
    let mut m = Mapping::new();
    m.insert(Value::String(key.into()), value);
    Value::Mapping(m)
}

pub fn generate_data_schema(schema: &DataSchema) -> Value {
    match schema {
        DataSchema::Anything => Value::String("anything".into()),
        DataSchema::IsString => Value::String("is-string".into()),
        DataSchema::IsStringMatching(re) => mk("is-string-matching", Value::String(re.clone())),
        DataSchema::IsNumber => Value::String("is-number".into()),
        DataSchema::IsBool => Value::String("is-bool".into()),
        DataSchema::IsNull => Value::String("is-null".into()),
        DataSchema::IsObject(entries) => {
            let mut m = Mapping::new();
            for (key, sub) in entries {
                m.insert(Value::String(key.clone()), generate_data_schema(sub));
            }
            mk("is-object", Value::Mapping(m))
        }
        DataSchema::IsArray(check) => {
            let mut m = Mapping::new();
            if let Some(ref f) = check.forall {
                m.insert(Value::String("forall".into()), generate_data_schema(f));
            }
            if let Some(ref e) = check.exists {
                m.insert(Value::String("exists".into()), generate_data_schema(e));
            }
            if !check.at.is_empty() {
                let mut at_m = Mapping::new();
                for (idx, sub) in &check.at {
                    at_m.insert(
                        Value::Number((*idx as u64).into()),
                        generate_data_schema(sub),
                    );
                }
                m.insert(Value::String("at".into()), Value::Mapping(at_m));
            }
            mk("is-array", Value::Mapping(m))
        }
    }
}

#[cfg(test)]
pub fn generate_data_schema_string(schema: &DataSchema) -> String {
    let value = generate_data_schema(schema);
    serde_yaml::to_string(&value).expect("failed to serialize data schema")
}

pub fn generate_predicate(pred: &FilePredicateAst) -> Value {
    match pred {
        FilePredicateAst::FileExists => Value::String("file-exists".into()),
        FilePredicateAst::TextMatchesRegex(re) => mk("text-matches", Value::String(re.clone())),
        FilePredicateAst::TextHasLines { min, max } => {
            let mut m = Mapping::new();
            if let Some(n) = min {
                m.insert(
                    Value::String("min".into()),
                    Value::Number((*n as u64).into()),
                );
            }
            if let Some(n) = max {
                m.insert(
                    Value::String("max".into()),
                    Value::Number((*n as u64).into()),
                );
            }
            mk("text-has-lines", Value::Mapping(m))
        }
        FilePredicateAst::ShellExports(var) => mk("shell-exports", Value::String(var.clone())),
        FilePredicateAst::ShellDefinesVariable(var) => {
            mk("shell-defines", Value::String(var.clone()))
        }
        FilePredicateAst::ShellAddsToPath(var) => {
            mk("shell-adds-to-path", Value::String(var.clone()))
        }
        FilePredicateAst::PropertiesDefinesKey(key_name) => {
            mk("properties-defines-key", Value::String(key_name.clone()))
        }
        FilePredicateAst::XmlMatchesPath(path) => mk("xml-matches", Value::String(path.clone())),
        FilePredicateAst::JsonMatches(schema) => mk("json-matches", generate_data_schema(schema)),
        FilePredicateAst::YamlMatches(schema) => mk("yaml-matches", generate_data_schema(schema)),
        FilePredicateAst::All(preds) => {
            let items: Vec<Value> = preds.iter().map(generate_predicate).collect();
            mk("all", Value::Sequence(items))
        }
        FilePredicateAst::Any { hint, checks } => {
            let mut m = Mapping::new();
            m.insert(Value::String("hint".into()), Value::String(hint.clone()));
            let items: Vec<Value> = checks.iter().map(generate_predicate).collect();
            m.insert(Value::String("checks".into()), Value::Sequence(items));
            mk("any", Value::Mapping(m))
        }
    }
}

pub fn generate_proposition(prop: &Proposition) -> Value {
    match prop {
        Proposition::FileSatisfies { path, check } => {
            let mut m = Mapping::new();
            m.insert(
                Value::String("path".into()),
                Value::String(path.as_str().into()),
            );
            m.insert(Value::String("check".into()), generate_predicate(check));
            mk("file", Value::Mapping(m))
        }
        Proposition::Forall { files, check } => {
            let mut m = Mapping::new();
            let file_list: Vec<Value> = files
                .iter()
                .map(|f| Value::String(f.as_str().into()))
                .collect();
            m.insert(Value::String("files".into()), Value::Sequence(file_list));
            m.insert(Value::String("check".into()), generate_predicate(check));
            mk("forall", Value::Mapping(m))
        }
        Proposition::Exists { files, check } => {
            let mut m = Mapping::new();
            let file_list: Vec<Value> = files
                .iter()
                .map(|f| Value::String(f.as_str().into()))
                .collect();
            m.insert(Value::String("files".into()), Value::Sequence(file_list));
            m.insert(Value::String("check".into()), generate_predicate(check));
            mk("exists", Value::Mapping(m))
        }
        Proposition::All(props) => {
            let items: Vec<Value> = props.iter().map(generate_proposition).collect();
            mk("all", Value::Sequence(items))
        }
        Proposition::Any(props) => {
            let items: Vec<Value> = props.iter().map(generate_proposition).collect();
            mk("any", Value::Sequence(items))
        }
    }
}

pub fn generate_predicate_string(pred: &FilePredicateAst) -> String {
    let value = generate_predicate(pred);
    serde_yaml::to_string(&value).expect("failed to serialize predicate")
}

pub fn generate_proposition_string(prop: &Proposition) -> String {
    let value = generate_proposition(prop);
    serde_yaml::to_string(&value).expect("failed to serialize proposition")
}
