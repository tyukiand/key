use crate::rules::scenario::{all_scenarios, guide_intro, ScenarioStep};

/// Render all scenarios as a human-readable guide.
pub fn render_guide() -> String {
    let mut out = String::new();
    out.push_str("# key audit — Guide\n\n");
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
                ScenarioStep::RunAuditRun { yaml_content } => {
                    out.push_str("Audit file:\n\n");
                    out.push_str("```yaml\n");
                    out.push_str(yaml_content);
                    if !yaml_content.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n\n");
                    out.push_str("```\n$ key audit run --file audit.yaml\n```\n\n");
                }
                ScenarioStep::RunAuditTest {
                    yaml_content,
                    expect_failure_messages,
                    expect_num_failures,
                } => {
                    out.push_str("Audit file:\n\n");
                    out.push_str("```yaml\n");
                    out.push_str(yaml_content);
                    if !yaml_content.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n\n");

                    let mut cmd = "$ key audit test audit.yaml ~/fake-home".to_string();
                    for msg in expect_failure_messages {
                        cmd.push_str(&format!(" --expect-failure-message {:?}", msg));
                    }
                    if let Some(n) = expect_num_failures {
                        cmd.push_str(&format!(" --expect-failures {}", n));
                    }
                    out.push_str(&format!("```\n{}\n```\n\n", cmd));
                }
                ScenarioStep::RunAuditProjectNew { name, .. } => {
                    out.push_str(&format!("```\n$ key audit project new {}\n```\n\n", name));
                }
                ScenarioStep::RunAuditProjectTest { .. } => {
                    out.push_str("```\n$ key audit project test\n```\n\n");
                }
                ScenarioStep::RunAuditProjectBuild { .. } => {
                    out.push_str("```\n$ key audit project build\n```\n\n");
                }
                ScenarioStep::RunAuditProjectClean { .. } => {
                    out.push_str("```\n$ key audit project clean\n```\n\n");
                }
                ScenarioStep::RunAuditProjectRun { .. } => {
                    out.push_str("```\n$ key audit project run\n```\n\n");
                }
                ScenarioStep::RunAuditInstall { config_name, .. } => {
                    out.push_str(&format!(
                        "```\n$ key audit install {}\n```\n\n",
                        config_name
                    ));
                }
                ScenarioStep::AssertFileExists { path } => {
                    out.push_str(&format!("Assert file exists: `{}`\n\n", path));
                }
                ScenarioStep::AssertDirMissing { path } => {
                    out.push_str(&format!("Assert directory does not exist: `{}`\n\n", path));
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
            "Literal substring match (no regex escaping needed)",
            F::TextContains("artifactory.mycompany.com".into()),
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
            "Properties / ini-style config: key=...",
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
        (
            "Negation (passes when inner check fails)",
            F::Not(Box::new(F::TextMatchesRegex("(?i)password".into()))),
        ),
        (
            "Conditional (if condition holds, then check must hold)",
            F::Conditionally {
                condition: Box::new(F::FileExists),
                then: Box::new(F::TextMatchesRegex("^registry=".into())),
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
        (
            "Negation (passes when inner rule fails)",
            P::Not(Box::new(P::FileSatisfies {
                path: sp("~/.ssh/id_dsa.pub"),
                check: F::FileExists,
            })),
        ),
        (
            "Conditional (if condition holds, then rule must hold)",
            P::Conditionally {
                condition: Box::new(P::FileSatisfies {
                    path: sp("~/.npmrc"),
                    check: F::FileExists,
                }),
                then: Box::new(P::FileSatisfies {
                    path: sp("~/.npmrc"),
                    check: F::TextMatchesRegex("^registry=".into()),
                }),
            },
        ),
    ];

    let mut out = String::new();
    out.push_str("## Quick Reference\n\n");

    out.push_str("### Control file format\n\n");
    out.push_str("An audit file is a YAML mapping with a `controls` list. Each control has:\n\n");
    out.push_str("```yaml\ncontrols:\n  - id: SSH-KEY-EXISTS\n    title: SSH key is present\n    description: Checks that an SSH key exists\n    remediation: Run ssh-keygen to create a key\n    check:\n      file:\n        path: ~/.ssh/id_ed25519.pub\n        check: file-exists\n```\n\n");
    out.push_str("The `id` must match `[A-Z][A-Z0-9-]*` and be unique within the file.\n\n");

    out.push_str("### Predicates (used inside `check:`)\n\n");
    for (description, pred) in &predicate_examples {
        out.push_str(&format!("{}:\n\n", description));
        out.push_str("```yaml\n");
        out.push_str(&pred_yaml(pred));
        out.push_str("```\n\n");
    }

    out.push_str("### Propositions (used in control `check:`)\n\n");
    for (description, prop) in &proposition_examples {
        out.push_str(&format!("{}:\n\n", description));
        out.push_str("```yaml\n");
        out.push_str(&prop_yaml(prop));
        out.push_str("```\n\n");
    }

    out.push_str("### Pseudo-files (`<env>` and `<executable:NAME>`)\n\n");
    out.push_str(
        "Pseudo-files let predicates run against virtual subjects (the current \
         shell environment, an introspected executable on PATH) instead of files \
         on disk. Pseudo-file identifiers begin with `<` and end with `>`; they \
         appear anywhere a concrete simple-path appears.\n\n",
    );

    out.push_str(
        "`<env>` materializes as `export NAME=VALUE` lines (sorted, \
                  newlines escaped). Use shell-* and text-* predicates against it:\n\n",
    );
    let env_example = P::FileSatisfies {
        path: sp("<env>"),
        check: F::ShellExports("RUSTUP_HOME".into()),
    };
    out.push_str("```yaml\n");
    out.push_str(&prop_yaml(&env_example));
    out.push_str("```\n\n");
    let env_path_example = P::FileSatisfies {
        path: sp("<env>"),
        check: F::ShellAddsToPath("/home/u/.cargo/bin".into()),
    };
    out.push_str("```yaml\n");
    out.push_str(&prop_yaml(&env_path_example));
    out.push_str("```\n\n");

    out.push_str(
        "`<executable:NAME>` materializes as a JSON snapshot of the \
                  executable resolved on PATH (`{name, found, executable, path, \
                  command-full, version-full, version}`). Use `file-exists` for a \
                  presence check or `json-matches` for the shape:\n\n",
    );
    let exec_example = P::FileSatisfies {
        path: sp("<executable:docker>"),
        check: F::FileExists,
    };
    out.push_str("```yaml\n");
    out.push_str(&prop_yaml(&exec_example));
    out.push_str("```\n\n");
    let exec_version_example = P::FileSatisfies {
        path: sp("<executable:python3>"),
        check: F::JsonMatches(DataSchema::IsObject(vec![(
            "version".into(),
            DataSchema::IsStringMatching(r"^3\.\d+".into()),
        )])),
    };
    out.push_str("```yaml\n");
    out.push_str(&prop_yaml(&exec_version_example));
    out.push_str("```\n\n");

    out.push_str(
        "Pseudo-files are read-only and cached for the duration of a single \
         `key audit run` invocation. Inapplicable predicates (e.g. `xml-matches` \
         on `<env>`, `shell-exports` on `<executable:NAME>`) fail explicitly with \
         a message naming both the predicate and the pseudo-file.\n\n",
    );

    out.push_str("### tests.yaml format (for audit projects)\n\n");
    out.push_str(
        "A test file defines suites of test cases that verify controls against fixtures:\n\n",
    );
    out.push_str("```yaml\ntest-suites:\n  - name: \"SSH key checks\"\n    description: \"Verify SSH key existence controls\"\n    tests:\n      - control-id: CTRL-0001\n        description: \"valid SSH key setup passes\"\n        fixture: CTRL-0001-valid\n        expect: pass\n\n      - control-id: CTRL-0001\n        description: \"missing SSH key detected\"\n        fixture: CTRL-0001-invalid\n        expect:\n          fail:\n            count: 1\n            messages:\n              - \"does not exist\"\n```\n\n");
    out.push_str("- `expect: pass` — control must pass on this fixture\n");
    out.push_str("- `expect: fail` — control must fail (any failure)\n");
    out.push_str("- `expect: { fail: { count: N, messages: [...] } }` — detailed expectations (both optional)\n\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_renders_without_panic() {
        let guide = render_guide();
        assert!(guide.contains("# key audit"));
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

    /// Spec/0009 §6.6: the guide must list `<env>` and `<executable:NAME>` with
    /// at least one YAML example each, and the examples must round-trip
    /// parse/evaluate against fixture overrides.
    #[test]
    fn guide_round_trips_pseudo_file_yaml_examples() {
        use crate::rules::ast::{ExecutableSnapshot, PseudoFileFixture, SimplePath};
        use crate::rules::evaluate::evaluate_with_ctx;
        use crate::rules::parse::parse_proposition;
        use crate::rules::pseudo::EvalContext;
        use std::collections::BTreeMap;

        let guide = render_guide();
        assert!(guide.contains("<env>"), "guide missing <env> section");
        assert!(
            guide.contains("<executable:"),
            "guide missing <executable:NAME> section"
        );

        // Extract YAML code-fenced examples between ```yaml and ``` lines and
        // try parsing each one as a proposition. Then drive the evaluator with
        // a fixture override that satisfies the pseudo-file references.
        let mut env = BTreeMap::new();
        env.insert("RUSTUP_HOME".into(), "/home/u/.rustup".into());
        env.insert("PATH".into(), "/home/u/.cargo/bin:/usr/bin".into());
        let mut exes = BTreeMap::new();
        exes.insert(
            "docker".into(),
            ExecutableSnapshot {
                name: "docker".into(),
                found: true,
                executable: true,
                path: Some("/usr/bin/docker".into()),
                command_full: Some("docker --version".into()),
                version_full: Some("Docker version 20.10.7".into()),
                version: Some("20.10.7".into()),
            },
        );
        exes.insert(
            "python3".into(),
            ExecutableSnapshot {
                name: "python3".into(),
                found: true,
                executable: true,
                path: Some("/usr/bin/python3".into()),
                command_full: Some("python3 --version".into()),
                version_full: Some("Python 3.11.4".into()),
                version: Some("3.11.4".into()),
            },
        );

        let tmp = tempfile::tempdir().unwrap();
        let ctx = EvalContext::with_fixture(
            tmp.path().to_path_buf(),
            PseudoFileFixture {
                env_override: Some(env),
                executable_override: Some(exes),
            },
        );

        let mut found_pseudo_examples = 0usize;
        let mut in_yaml = false;
        let mut block = String::new();
        for line in guide.lines() {
            if line.trim_start().starts_with("```yaml") {
                in_yaml = true;
                block.clear();
                continue;
            }
            if in_yaml && line.trim_start().starts_with("```") {
                in_yaml = false;
                if block.contains("<env>") || block.contains("<executable:") {
                    let parsed: serde_yaml::Value =
                        serde_yaml::from_str(&block).unwrap_or_else(|e| {
                            panic!(
                                "YAML parse failed for guide example:\n{}\nerr: {}",
                                block, e
                            )
                        });
                    if let Ok(prop) = parse_proposition(&parsed) {
                        // Best-effort: evaluate; the example may legitimately
                        // not pass against this fixture, but it must not panic
                        // and must reach the evaluator.
                        let _ = evaluate_with_ctx(&prop, &ctx);
                        // Also ensure the path round-trips.
                        match &prop {
                            crate::rules::ast::Proposition::FileSatisfies { path, .. } => {
                                assert!(
                                    path.is_pseudo() || path.as_str().starts_with('~'),
                                    "unexpected path in pseudo example: {}",
                                    path
                                );
                                if path.is_pseudo() {
                                    found_pseudo_examples += 1;
                                    let p = SimplePath::new(path.as_str()).unwrap();
                                    assert_eq!(p.as_str(), path.as_str());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                continue;
            }
            if in_yaml {
                block.push_str(line);
                block.push('\n');
            }
        }
        assert!(
            found_pseudo_examples >= 2,
            "expected at least two pseudo-file YAML examples in guide; got {}",
            found_pseudo_examples
        );
    }
}
