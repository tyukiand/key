use crate::rules::ast::{FilePredicateAst, Proposition, SimplePath};

pub struct Scenario {
    pub name: String,
    pub description: String,
    pub steps: Vec<ScenarioStep>,
}

pub enum ScenarioStep {
    /// Narrative text — rendered in guide, no-op in tests
    Prose(String),
    /// Write content to a file (creates parent dirs)
    WriteFile { path: SimplePath, content: String },
    /// Create a directory
    CreateDir { path: SimplePath },
    /// Run `key rules check <yaml>` and store the result
    RunRulesCheck { yaml_content: String },
    /// Run `key rules test <yaml> <home> [flags]` and store the result
    RunRulesTest {
        yaml_content: String,
        expect_failure_messages: Vec<String>,
        expect_num_failures: Option<usize>,
    },
    /// Assert the last command succeeded
    ExpectSuccess,
    /// Assert the last command failed with these messages in output
    ExpectFailure { messages: Vec<String> },
}

fn gen(prop: &Proposition) -> String {
    crate::rules::generate::generate_proposition_string(prop)
}

pub fn guide_intro() -> String {
    "\
`key rules` evaluates user-defined YAML rule files against the filesystem. \
Two subcommands serve different purposes:

- `key rules check <rules.yaml>` — evaluate rules against your real `$HOME`. \
Use this to verify your system is configured correctly.
- `key rules test <rules.yaml> <fake-home>` — evaluate rules against a fixture \
directory. Supports `--expect-failures <N>` and repeatable `--expect-failure-message <msg>` \
flags, so you can assert that your rules catch problems correctly.

All file paths in rules use `~/...` notation, which resolves to `$HOME` in `check` \
mode or to the fake home directory in `test` mode.

The scenarios below are tested automatically — they are always up to date.\n"
        .to_string()
}

pub fn all_scenarios() -> Vec<Scenario> {
    vec![
        scenario_basic_file_exists(),
        scenario_text_matches_regex(),
        scenario_text_has_lines(),
        scenario_shell_exports(),
        scenario_shell_defines_variable(),
        scenario_shell_adds_to_path(),
        scenario_properties_defines_key(),
        scenario_xml_matches_path(),
        scenario_json_matches_query(),
        scenario_yaml_matches_query(),
        scenario_predicate_all_and_any(),
        scenario_forall_with_fix(),
        scenario_exists_quantifier(),
        scenario_proposition_any(),
        scenario_rules_test_command(),
    ]
}

// ---------------------------------------------------------------------------
// Predicate scenarios — one per FilePredicateAst variant
// ---------------------------------------------------------------------------

fn scenario_basic_file_exists() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.ssh/config").unwrap(),
        check: FilePredicateAst::FileExists,
    });

    Scenario {
        name: "file-exists".into(),
        description: "Check that a file exists".into(),
        steps: vec![
            ScenarioStep::Prose(
                "The simplest predicate checks whether a file exists. \
                 `file-exists` is the only predicate that can appear as a bare string."
                    .into(),
            ),
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["does not exist".into()],
            },
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.ssh").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.ssh/config").unwrap(),
                content: "Host *\n  AddKeysToAgent yes\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_text_matches_regex() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.bashrc").unwrap(),
        check: FilePredicateAst::TextMatchesRegex(r"^\s*source\s+.*\.env".into()),
    });

    Scenario {
        name: "text-matches".into(),
        description: "Check that a file contains a line matching a regex".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`text-matches` checks that at least one line matches the given regex. \
                 Uses Rust regex syntax."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "# my bashrc\necho hello\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "# my bashrc\nsource ~/.env\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_text_has_lines() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.ssh/authorized_keys").unwrap(),
        check: FilePredicateAst::TextHasLines {
            min: Some(1),
            max: Some(50),
        },
    });

    Scenario {
        name: "text-has-lines".into(),
        description: "Check that a file has a line count within bounds".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`text-has-lines` checks line count. Both `min` and `max` are optional \
                 and inclusive. Use `min: 1` to check the file is non-empty."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.ssh").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.ssh/authorized_keys").unwrap(),
                content: "".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["expected at least 1".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.ssh/authorized_keys").unwrap(),
                content: "ssh-ed25519 AAAA... user@host\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_shell_exports() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.bashrc").unwrap(),
        check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
    });

    Scenario {
        name: "shell-exports".into(),
        description: "Check that a shell file exports a variable".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`shell-exports` checks for a line matching `export VAR=...`. \
                 It desugars to `text-matches` with the appropriate regex."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "# my bashrc\necho hello\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_shell_defines_variable() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.bashrc").unwrap(),
        check: FilePredicateAst::ShellDefinesVariable("MY_VAR".into()),
    });

    Scenario {
        name: "shell-defines".into(),
        description: "Check that a shell file defines a variable (with or without export)".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`shell-defines` is like `shell-exports` but also accepts plain \
                 `VAR=value` without `export`."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "echo hello\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "MY_VAR=hello\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_shell_adds_to_path() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.bashrc").unwrap(),
        check: FilePredicateAst::ShellAddsToPath("JAVA_HOME_BIN".into()),
    });

    Scenario {
        name: "shell-adds-to-path".into(),
        description: "Check that a shell file adds a variable to PATH".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`shell-adds-to-path` checks for `export PATH=\"$VAR:$PATH\"`. \
                 Intended to be used together with `shell-exports` or `shell-defines` \
                 for the same variable."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "echo hello\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export PATH=\"$JAVA_HOME_BIN:$PATH\"\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_properties_defines_key() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.gradle/gradle.properties").unwrap(),
        check: FilePredicateAst::PropertiesDefinesKey("signing.keyId".into()),
    });

    Scenario {
        name: "properties-defines-key".into(),
        description: "Check that a .properties file defines a key".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`properties-defines-key` checks for a line starting with `key=` \
                 in a Java-style `.properties` file."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.gradle").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.gradle/gradle.properties").unwrap(),
                content: "org.gradle.parallel=true\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line defines key".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.gradle/gradle.properties").unwrap(),
                content: "org.gradle.parallel=true\nsigning.keyId=ABC123\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_xml_matches_path() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.m2/settings.xml").unwrap(),
        check: FilePredicateAst::XmlMatchesPath("settings/servers/server/id".into()),
    });

    Scenario {
        name: "xml-matches".into(),
        description: "Check that an XML file contains an element path".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`xml-matches` checks for a slash-separated element path in XML. \
                 No attributes, no wildcards — just element names."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.m2").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.m2/settings.xml").unwrap(),
                content: "<settings>\n</settings>\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["XML path".into(), "not found".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.m2/settings.xml").unwrap(),
                content: "<settings>\n  <servers>\n    <server>\n      \
                          <id>central</id>\n    </server>\n  </servers>\n</settings>\n"
                    .into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_json_matches_query() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.config/app.json").unwrap(),
        check: FilePredicateAst::JsonMatchesQuery(".settings.theme".into()),
    });

    Scenario {
        name: "json-matches".into(),
        description: "Check that a JSON file contains a path".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`json-matches` uses jq-style dot-paths: `.key`, `.key.sub`, \
                 `.arr[0]`, `.arr[0].name`."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.config").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/app.json").unwrap(),
                content: "{\"settings\":{}}\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["not found".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/app.json").unwrap(),
                content: "{\"settings\":{\"theme\":\"dark\"}}\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_yaml_matches_query() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.config/models.yaml").unwrap(),
        check: FilePredicateAst::YamlMatchesQuery("models[0].name".into()),
    });

    Scenario {
        name: "yaml-matches".into(),
        description: "Check that a YAML file contains a path".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`yaml-matches` uses the same dot-path syntax as `json-matches`, \
                 including array indices like `models[0].name`."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.config").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/models.yaml").unwrap(),
                content: "models: []\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["not found".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/models.yaml").unwrap(),
                content: "models:\n  - name: gpt4\n    version: 1\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_predicate_all_and_any() -> Scenario {
    let check_all = FilePredicateAst::All(vec![
        FilePredicateAst::FileExists,
        FilePredicateAst::ShellExports("JAVA_HOME".into()),
    ]);
    let yaml_all = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.bashrc").unwrap(),
        check: check_all,
    });

    let check_any = FilePredicateAst::Any {
        hint: "Configure Java: either export JAVA_HOME or define it without export".into(),
        checks: vec![
            FilePredicateAst::ShellExports("JAVA_HOME".into()),
            FilePredicateAst::ShellDefinesVariable("JAVA_HOME".into()),
        ],
    };
    let yaml_any = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.bashrc").unwrap(),
        check: check_any,
    });

    Scenario {
        name: "predicate-all-and-any".into(),
        description: "Combine predicates with `all` and `any`".into(),
        steps: vec![
            ScenarioStep::Prose(
                "Predicates can be combined inside `check:`. \
                 `all` requires every sub-check to pass. \
                 `any` requires at least one — the `hint` field is shown \
                 when all alternatives fail."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "echo hello\n".into(),
            },
            ScenarioStep::Prose(
                "With `all`, both `file-exists` and `shell-exports` must hold:".into(),
            ),
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml_all.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml_all,
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::Prose(
                "With `any`, the hint is shown when no alternative matches:".into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "echo hello\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml_any.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["hint:".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml_any,
            },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

// ---------------------------------------------------------------------------
// Proposition-level scenarios
// ---------------------------------------------------------------------------

fn scenario_forall_with_fix() -> Scenario {
    let prop = Proposition::Forall {
        files: vec![
            SimplePath::new("~/.bashrc").unwrap(),
            SimplePath::new("~/.zshrc").unwrap(),
        ],
        check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
    };
    let yaml = gen(&prop);

    Scenario {
        name: "forall".into(),
        description: "`forall` — every listed file must satisfy the check".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`forall` requires ALL listed files to satisfy the check. \
                 If any file fails, its path appears in the error."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.zshrc").unwrap(),
                content: "# empty\n".into(),
            },
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["~/.zshrc".into()],
            },
            ScenarioStep::Prose("After adding the export to .zshrc, all files pass.".into()),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.zshrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_exists_quantifier() -> Scenario {
    let prop = Proposition::Exists {
        files: vec![
            SimplePath::new("~/.bash_profile").unwrap(),
            SimplePath::new("~/.profile").unwrap(),
            SimplePath::new("~/.zshrc").unwrap(),
        ],
        check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
    };
    let yaml = gen(&prop);

    Scenario {
        name: "exists".into(),
        description: "`exists` — at least one listed file must satisfy the check".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`exists` is existential quantification over a list of files: \
                 it succeeds if AT LEAST ONE file satisfies the check. \
                 This is useful when a variable could reasonably be set \
                 in any of several shell config files."
                    .into(),
            ),
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["does not exist".into()],
            },
            ScenarioStep::Prose(
                "Only one file needs to satisfy the check for `exists` to pass:".into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.zshrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_proposition_any() -> Scenario {
    let prop = Proposition::Any(vec![
        Proposition::FileSatisfies {
            path: SimplePath::new("~/.ssh/id_ed25519.pub").unwrap(),
            check: FilePredicateAst::FileExists,
        },
        Proposition::FileSatisfies {
            path: SimplePath::new("~/.ssh/id_rsa.pub").unwrap(),
            check: FilePredicateAst::FileExists,
        },
    ]);
    let yaml = gen(&prop);

    Scenario {
        name: "proposition-any".into(),
        description: "`any` — at least one sub-rule must hold".into(),
        steps: vec![
            ScenarioStep::Prose(
                "Proposition-level `any` succeeds when at least one of its \
                 sub-propositions holds. Unlike predicate-level `any`, \
                 it does not take a hint."
                    .into(),
            ),
            ScenarioStep::RunRulesCheck {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["does not exist".into()],
            },
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.ssh").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.ssh/id_ed25519.pub").unwrap(),
                content: "ssh-ed25519 AAAA... user@host\n".into(),
            },
            ScenarioStep::RunRulesCheck { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

// ---------------------------------------------------------------------------
// Testing workflow
// ---------------------------------------------------------------------------

fn scenario_rules_test_command() -> Scenario {
    let prop = Proposition::FileSatisfies {
        path: SimplePath::new("~/.config/app.json").unwrap(),
        check: FilePredicateAst::JsonMatchesQuery(".settings.theme".into()),
    };
    let yaml = gen(&prop);

    Scenario {
        name: "rules-test".into(),
        description: "Use `key rules test` to verify expected failures".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`key rules test` evaluates rules against a fake home directory. \
                 Use `--expect-failures <N>` to assert the number of failures, \
                 and repeat `--expect-failure-message <msg>` to assert that \
                 specific messages appear in the output."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.config").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/app.json").unwrap(),
                content: "{\"settings\":{\"theme\":\"dark\"}}\n".into(),
            },
            ScenarioStep::RunRulesTest {
                yaml_content: yaml.clone(),
                expect_failure_messages: vec![],
                expect_num_failures: None,
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::Prose(
                "When the file lacks the expected key, the test confirms the failure:".into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/app.json").unwrap(),
                content: "{\"settings\":{}}\n".into(),
            },
            ScenarioStep::RunRulesTest {
                yaml_content: yaml,
                expect_failure_messages: vec!["not found".into()],
                expect_num_failures: Some(1),
            },
            ScenarioStep::ExpectSuccess,
        ],
    }
}
