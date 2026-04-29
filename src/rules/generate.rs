use crate::rules::ast::{
    Control, ControlFile, DataSchema, FilePredicateAst, Proposition, TestCase, TestExpectation,
    TestFile, TestSuite,
};
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
        FilePredicateAst::TextContains(s) => mk("text-contains", Value::String(s.clone())),
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
        FilePredicateAst::ShellExportsValueMatches { name, value_regex } => {
            let mut m = Mapping::new();
            m.insert(Value::String("name".into()), Value::String(name.clone()));
            m.insert(
                Value::String("value-matches".into()),
                Value::String(value_regex.clone()),
            );
            mk("shell-exports", Value::Mapping(m))
        }
        FilePredicateAst::ShellDefinesVariable(var) => {
            mk("shell-defines", Value::String(var.clone()))
        }
        FilePredicateAst::ShellDefinesVariableValueMatches { name, value_regex } => {
            let mut m = Mapping::new();
            m.insert(Value::String("name".into()), Value::String(name.clone()));
            m.insert(
                Value::String("value-matches".into()),
                Value::String(value_regex.clone()),
            );
            mk("shell-defines", Value::Mapping(m))
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
        FilePredicateAst::Not(inner) => mk("not", generate_predicate(inner)),
        FilePredicateAst::Conditionally { condition, then } => {
            let mut m = Mapping::new();
            m.insert(Value::String("if".into()), generate_predicate(condition));
            m.insert(Value::String("then".into()), generate_predicate(then));
            mk("conditionally", Value::Mapping(m))
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
        Proposition::Not(inner) => mk("not", generate_proposition(inner)),
        Proposition::Conditionally { condition, then } => {
            let mut m = Mapping::new();
            m.insert(Value::String("if".into()), generate_proposition(condition));
            m.insert(Value::String("then".into()), generate_proposition(then));
            mk("conditionally", Value::Mapping(m))
        }
    }
}

#[cfg(test)]
pub fn generate_predicate_string(pred: &FilePredicateAst) -> String {
    let value = generate_predicate(pred);
    serde_yaml::to_string(&value).expect("failed to serialize predicate")
}

#[cfg(test)]
pub fn generate_proposition_string(prop: &Proposition) -> String {
    let value = generate_proposition(prop);
    serde_yaml::to_string(&value).expect("failed to serialize proposition")
}

pub fn generate_control(control: &Control) -> Value {
    let mut m = Mapping::new();
    m.insert(
        Value::String("id".into()),
        Value::String(control.id.clone()),
    );
    m.insert(
        Value::String("title".into()),
        Value::String(control.title.clone()),
    );
    m.insert(
        Value::String("description".into()),
        Value::String(control.description.clone()),
    );
    m.insert(
        Value::String("remediation".into()),
        Value::String(control.remediation.clone()),
    );
    m.insert(
        Value::String("check".into()),
        generate_proposition(&control.check),
    );
    Value::Mapping(m)
}

pub fn generate_control_file(cf: &ControlFile) -> String {
    let mut m = Mapping::new();
    let controls: Vec<Value> = cf.controls.iter().map(generate_control).collect();
    m.insert(Value::String("controls".into()), Value::Sequence(controls));
    serde_yaml::to_string(&Value::Mapping(m)).expect("failed to serialize control file")
}

// ---------------------------------------------------------------------------
// TestAst generation
// ---------------------------------------------------------------------------

pub fn generate_test_expectation(exp: &TestExpectation) -> Value {
    match exp {
        TestExpectation::Pass => Value::String("pass".into()),
        TestExpectation::Fail(fail_exp) => {
            if fail_exp.count.is_none() && fail_exp.messages.is_empty() {
                Value::String("fail".into())
            } else {
                let mut m = Mapping::new();
                if let Some(count) = fail_exp.count {
                    m.insert(
                        Value::String("count".into()),
                        Value::Number((count as u64).into()),
                    );
                }
                if !fail_exp.messages.is_empty() {
                    let msgs: Vec<Value> = fail_exp
                        .messages
                        .iter()
                        .map(|s| Value::String(s.clone()))
                        .collect();
                    m.insert(Value::String("messages".into()), Value::Sequence(msgs));
                }
                mk("fail", Value::Mapping(m))
            }
        }
    }
}

#[cfg(test)]
pub fn generate_test_expectation_string(exp: &TestExpectation) -> String {
    let value = generate_test_expectation(exp);
    serde_yaml::to_string(&value).expect("failed to serialize test expectation")
}

pub fn generate_test_case(tc: &TestCase) -> Value {
    let mut m = Mapping::new();
    m.insert(
        Value::String("control-id".into()),
        Value::String(tc.control_id.clone()),
    );
    m.insert(
        Value::String("description".into()),
        Value::String(tc.description.clone()),
    );
    m.insert(
        Value::String("fixture".into()),
        Value::String(tc.fixture.clone()),
    );
    m.insert(
        Value::String("expect".into()),
        generate_test_expectation(&tc.expect),
    );
    Value::Mapping(m)
}

pub fn generate_test_suite(ts: &TestSuite) -> Value {
    let mut m = Mapping::new();
    m.insert(Value::String("name".into()), Value::String(ts.name.clone()));
    if let Some(ref desc) = ts.description {
        m.insert(
            Value::String("description".into()),
            Value::String(desc.clone()),
        );
    }
    let tests: Vec<Value> = ts.tests.iter().map(generate_test_case).collect();
    m.insert(Value::String("tests".into()), Value::Sequence(tests));
    Value::Mapping(m)
}

pub fn generate_test_file(tf: &TestFile) -> String {
    let mut m = Mapping::new();
    let suites: Vec<Value> = tf.test_suites.iter().map(generate_test_suite).collect();
    m.insert(Value::String("test-suites".into()), Value::Sequence(suites));
    serde_yaml::to_string(&Value::Mapping(m)).expect("failed to serialize test file")
}
