use crate::rules::scenario::{all_scenarios, guide_intro, ScenarioStep};

/// Render all scenarios as a human-readable guide.
pub fn render_guide() -> String {
    let mut out = String::new();
    out.push_str("# key rules — Guide\n\n");
    out.push_str(&guide_intro());
    out.push('\n');

    for scenario in all_scenarios() {
        out.push_str(&format!(
            "## {} — {}\n\n",
            scenario.name, scenario.description
        ));

        for step in &scenario.steps {
            match step {
                ScenarioStep::Prose(text) => {
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                ScenarioStep::WriteFile { path, content } => {
                    out.push_str(&format!("Assume `{}` contains:\n\n", path));
                    out.push_str("```\n");
                    out.push_str(content);
                    if !content.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n\n");
                }
                ScenarioStep::CreateDir { path } => {
                    out.push_str(&format!("Ensure the directory `{}/` exists.\n\n", path));
                }
                ScenarioStep::RunRulesCheck { yaml_content } => {
                    out.push_str("Rules file:\n\n");
                    out.push_str("```yaml\n");
                    out.push_str(yaml_content);
                    if !yaml_content.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n\n");
                    out.push_str("```\n$ key rules check rules.yaml\n```\n\n");
                }
                ScenarioStep::RunRulesTest {
                    yaml_content,
                    expect_failure_messages,
                    expect_num_failures,
                } => {
                    out.push_str("Rules file:\n\n");
                    out.push_str("```yaml\n");
                    out.push_str(yaml_content);
                    if !yaml_content.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n\n");

                    let mut cmd = "$ key rules test rules.yaml ~/fake-home".to_string();
                    for msg in expect_failure_messages {
                        cmd.push_str(&format!(" --expect-failure-message {:?}", msg));
                    }
                    if let Some(n) = expect_num_failures {
                        cmd.push_str(&format!(" --expect-failures {}", n));
                    }
                    out.push_str(&format!("```\n{}\n```\n\n", cmd));
                }
                ScenarioStep::ExpectSuccess => {
                    out.push_str("Expected: all checks pass.\n\n");
                }
                ScenarioStep::ExpectFailure { messages } => {
                    out.push_str("Expected failure");
                    if !messages.is_empty() {
                        out.push_str(" containing: ");
                        let quoted: Vec<String> =
                            messages.iter().map(|m| format!("{:?}", m)).collect();
                        out.push_str(&quoted.join(", "));
                    }
                    out.push_str(".\n\n");
                }
            }
        }

        out.push_str("---\n\n");
    }

    out.push_str(&quick_reference());

    out
}

fn quick_reference() -> String {
    use crate::rules::ast::{
        DataArrayCheck, DataSchema, FilePredicateAst as F, Proposition as P, SimplePath,
    };
    use crate::rules::generate::{generate_predicate_string, generate_proposition_string};

    fn strip_doc_marker(s: &str) -> String {
        s.strip_prefix("---\n").unwrap_or(s).to_string()
    }

    fn pred_yaml(pred: &F) -> String {
        strip_doc_marker(&generate_predicate_string(pred))
    }

    fn prop_yaml(prop: &P) -> String {
        strip_doc_marker(&generate_proposition_string(prop))
    }

    fn sp(s: &str) -> SimplePath {
        SimplePath::new(s).unwrap()
    }

    let predicate_examples: Vec<(&str, F)> = vec![
        (
            "File existence (bare string — no key: value)",
            F::FileExists,
        ),
        (
            "Regex match (at least one line must match)",
            F::TextMatchesRegex("^import .*".into()),
        ),
        (
            "Line count bounds (both optional, inclusive)",
            F::TextHasLines {
                min: Some(1),
                max: Some(500),
            },
        ),
        ("Shell: export VAR=...", F::ShellExports("JAVA_HOME".into())),
        (
            "Shell: VAR=... (with or without export)",
            F::ShellDefinesVariable("MY_VAR".into()),
        ),
        (
            "Shell: export PATH=\"$VAR:$PATH\"",
            F::ShellAddsToPath("JAVA_HOME_BIN".into()),
        ),
        (
            "Java .properties: key=...",
            F::PropertiesDefinesKey("signing.keyId".into()),
        ),
        (
            "XML element path (slash-separated)",
            F::XmlMatchesPath("settings/servers/server/id".into()),
        ),
        (
            "JSON data schema (object with typed keys)",
            F::JsonMatches(DataSchema::IsObject(vec![(
                "user".into(),
                DataSchema::IsObject(vec![("name".into(), DataSchema::IsString)]),
            )])),
        ),
        (
            "YAML data schema (array with forall constraint)",
            F::YamlMatches(DataSchema::IsArray(DataArrayCheck {
                forall: Some(Box::new(DataSchema::IsObject(vec![(
                    "name".into(),
                    DataSchema::IsString,
                )]))),
                exists: None,
                at: vec![],
            })),
        ),
        (
            "All predicates must hold",
            F::All(vec![F::FileExists, F::ShellExports("JAVA_HOME".into())]),
        ),
        (
            "At least one must hold (hint shown on failure)",
            F::Any {
                hint: "Configure Java".into(),
                checks: vec![
                    F::ShellExports("JAVA_HOME".into()),
                    F::ShellDefinesVariable("JAVA_HOME".into()),
                ],
            },
        ),
    ];

    let proposition_examples: Vec<(&str, P)> = vec![
        (
            "Single file",
            P::FileSatisfies {
                path: sp("~/.bashrc"),
                check: F::FileExists,
            },
        ),
        (
            "All files must satisfy check",
            P::Forall {
                files: vec![sp("~/.bashrc"), sp("~/.zshrc")],
                check: F::ShellExports("JAVA_HOME".into()),
            },
        ),
        (
            "At least one file must satisfy check (existential quantification)",
            P::Exists {
                files: vec![sp("~/.bash_profile"), sp("~/.profile"), sp("~/.zshrc")],
                check: F::ShellExports("JAVA_HOME".into()),
            },
        ),
        (
            "All sub-rules must hold",
            P::All(vec![
                P::FileSatisfies {
                    path: sp("~/.bashrc"),
                    check: F::FileExists,
                },
                P::FileSatisfies {
                    path: sp("~/.zshrc"),
                    check: F::FileExists,
                },
            ]),
        ),
        (
            "At least one sub-rule must hold",
            P::Any(vec![
                P::FileSatisfies {
                    path: sp("~/.ssh/id_ed25519.pub"),
                    check: F::FileExists,
                },
                P::FileSatisfies {
                    path: sp("~/.ssh/id_rsa.pub"),
                    check: F::FileExists,
                },
            ]),
        ),
    ];

    let mut out = String::new();
    out.push_str("## Quick Reference\n\n");

    out.push_str("### Predicates (used inside `check:`)\n\n");
    for (description, pred) in &predicate_examples {
        out.push_str(&format!("{}:\n\n", description));
        out.push_str("```yaml\n");
        out.push_str(&pred_yaml(pred));
        out.push_str("```\n\n");
    }

    out.push_str("### Propositions (top-level rules)\n\n");
    for (description, prop) in &proposition_examples {
        out.push_str(&format!("{}:\n\n", description));
        out.push_str("```yaml\n");
        out.push_str(&prop_yaml(prop));
        out.push_str("```\n\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_renders_without_panic() {
        let guide = render_guide();
        assert!(guide.contains("# key rules"));
        assert!(guide.contains("Quick Reference"));
        // Ensure all scenarios are represented
        for scenario in all_scenarios() {
            assert!(
                guide.contains(&scenario.description),
                "Guide missing scenario: {}",
                scenario.name
            );
        }
    }
}
