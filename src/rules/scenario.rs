use crate::rules::ast::{
    Control, ControlFile, DataArrayCheck, DataSchema, FailExpectation, FilePredicateAst,
    Proposition, SimplePath, TestCase, TestExpectation, TestFile, TestSuite,
};

pub struct Scenario {
    pub name: String,
    pub description: String,
    pub steps: Vec<ScenarioStep>,
}

#[allow(dead_code)]
pub enum ScenarioStep {
    /// Narrative text — rendered in guide, no-op in tests
    Prose(String),
    /// Write content to a file (creates parent dirs)
    WriteFile { path: SimplePath, content: String },
    /// Create a directory
    CreateDir { path: SimplePath },
    /// Run `key audit run --file <yaml>` and store the result
    RunAuditRun { yaml_content: String },
    /// Run `key audit test <yaml> <home> [flags]` and store the result
    RunAuditTest {
        yaml_content: String,
        expect_failure_messages: Vec<String>,
        expect_num_failures: Option<usize>,
    },
    /// Run `key audit project new <name>` in a working directory
    RunAuditProjectNew { work_dir: SimplePath, name: String },
    /// Run `key audit project test` in a project directory
    RunAuditProjectTest { project_dir: SimplePath },
    /// Run `key audit project build` in a project directory
    RunAuditProjectBuild { project_dir: SimplePath },
    /// Run `key audit project clean` in a project directory
    RunAuditProjectClean { project_dir: SimplePath },
    /// Run `key audit project run` in a project directory
    RunAuditProjectRun { project_dir: SimplePath },
    /// Run `key audit install <yaml>`
    RunAuditInstall {
        yaml_content: String,
        config_name: String,
    },
    /// Assert a file exists at a path
    AssertFileExists { path: SimplePath },
    /// Assert a directory does NOT exist
    AssertDirMissing { path: SimplePath },
    /// Assert the last command succeeded
    ExpectSuccess,
    /// Assert the last command failed with these messages in output
    ExpectFailure { messages: Vec<String> },
}

fn gen(prop: &Proposition) -> String {
    gen_with_id("CTRL", "Scenario check", prop)
}

fn gen_with_id(id: &str, title: &str, prop: &Proposition) -> String {
    let cf = ControlFile {
        controls: vec![Control {
            id: id.to_string(),
            title: title.to_string(),
            description: "Scenario check".to_string(),
            remediation: "See guide for details".to_string(),
            check: prop.clone(),
        }],
    };
    crate::rules::generate::generate_control_file(&cf)
}

pub fn guide_intro() -> String {
    "\
`key audit` evaluates user-defined YAML audit files against the filesystem. \
Each audit file contains a list of **controls** — named checks with an ID, title, \
description, remediation instructions, and a check proposition.

Key subcommands:

- `key audit run --file <audit.yaml>` — evaluate controls against your real `$HOME`. \
Use this to verify your system is configured correctly.
- `key audit test <audit.yaml> <fake-home>` — evaluate controls against a fixture \
directory. Supports `--expect-failures <N>` and repeatable `--expect-failure-message <msg>` \
flags, so you can assert that your controls catch problems correctly.
- `key audit new <audit.yaml>` — create a new empty audit file.
- `key audit add <audit.yaml>` — interactively add a control.
- `key audit list <audit.yaml>` — list controls (use `--short` for one-line output).
- `key audit delete --file <audit.yaml> [--id <ID>]` — delete a control.
- `key audit install <audit.yaml>` — install a config for use with the picker.
- `key audit` (bare) — pick an installed config and run it.
- `key audit guide` — print this guide.

Audit projects (`key audit project`):

- `key audit project new <name>` — scaffold a new audit project.
- `key audit project test` — run tests.yaml against controls and fixtures.
- `key audit project build` — parse-check, test, and copy to target/.
- `key audit project clean` — remove target/.
- `key audit project run [--home <dir>]` — run controls against $HOME.

All file paths in controls use `~/...` notation, which resolves to `$HOME` in `run` \
mode or to the fake home directory in `test` mode.

The scenarios below are tested automatically — they are always up to date.\n"
        .to_string()
}

pub fn all_scenarios() -> Vec<Scenario> {
    vec![
        scenario_basic_file_exists(),
        scenario_text_matches_regex(),
        scenario_text_contains(),
        scenario_text_has_lines(),
        scenario_shell_exports(),
        scenario_shell_defines_variable(),
        scenario_shell_adds_to_path(),
        scenario_properties_defines_key(),
        scenario_xml_matches_path(),
        scenario_json_matches_schema(),
        scenario_yaml_matches_schema(),
        scenario_json_array_schema(),
        scenario_predicate_all_and_any(),
        scenario_forall_with_fix(),
        scenario_exists_quantifier(),
        scenario_proposition_any(),
        scenario_not(),
        scenario_conditionally(),
        scenario_rules_test_command(),
        scenario_project_new(),
        scenario_project_test_pass(),
        scenario_project_test_fp_detected(),
        scenario_project_test_fn_detected(),
        scenario_project_test_bad_control_id(),
        scenario_project_build(),
        scenario_project_clean(),
        scenario_audit_install_and_pick(),
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
            ScenarioStep::RunAuditRun {
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
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "# my bashrc\nsource ~/.env\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_text_contains() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.npmrc").unwrap(),
        check: FilePredicateAst::TextContains("artifactory.mycompany.com".into()),
    });

    Scenario {
        name: "text-contains".into(),
        description: "Check that a file contains a literal substring".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`text-contains` checks that at least one line contains the given \
                 literal substring. Unlike `text-matches`, no regex escaping is needed."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.npmrc").unwrap(),
                content: "registry=https://registry.npmjs.org/\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line contains".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.npmrc").unwrap(),
                content: "registry=https://artifactory.mycompany.com/npm/\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["expected at least 1".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.ssh/authorized_keys").unwrap(),
                content: "ssh-ed25519 AAAA... user@host\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "MY_VAR=hello\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export PATH=\"$JAVA_HOME_BIN:$PATH\"\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
                 in a Java-style `.properties` or ini-style config file \
                 (e.g. `.npmrc`, `.gitconfig`, `.ini`). Works for any file \
                 using `key=value` format."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.gradle").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.gradle/gradle.properties").unwrap(),
                content: "org.gradle.parallel=true\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line defines key".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.gradle/gradle.properties").unwrap(),
                content: "org.gradle.parallel=true\nsigning.keyId=ABC123\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
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
            ScenarioStep::RunAuditRun { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_json_matches_schema() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.config/app.json").unwrap(),
        check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
            "settings".into(),
            DataSchema::IsObject(vec![("theme".into(), DataSchema::IsString)]),
        )])),
    });

    Scenario {
        name: "json-matches".into(),
        description: "Check that a JSON file matches a data schema".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`json-matches` validates a JSON file against a data schema. \
                 The schema describes the expected shape: types, object keys, \
                 and array constraints. Unmentioned keys are allowed."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.config").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/app.json").unwrap(),
                content: "{\"settings\":{}}\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["missing key".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/app.json").unwrap(),
                content: "{\"settings\":{\"theme\":\"dark\"}}\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_yaml_matches_schema() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.config/models.yaml").unwrap(),
        check: FilePredicateAst::YamlMatches(DataSchema::IsObject(vec![(
            "models".into(),
            DataSchema::IsArray(DataArrayCheck {
                forall: None,
                exists: None,
                at: vec![(
                    0,
                    DataSchema::IsObject(vec![("name".into(), DataSchema::IsString)]),
                )],
            }),
        )])),
    });

    Scenario {
        name: "yaml-matches".into(),
        description: "Check that a YAML file matches a data schema".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`yaml-matches` uses the same data schema as `json-matches`, \
                 applied to YAML files."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.config").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/models.yaml").unwrap(),
                content: "models: []\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["out of bounds".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/models.yaml").unwrap(),
                content: "models:\n  - name: gpt4\n    version: 1\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_json_array_schema() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.config/users.json").unwrap(),
        check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
            "users".into(),
            DataSchema::IsArray(DataArrayCheck {
                forall: Some(Box::new(DataSchema::IsObject(vec![(
                    "name".into(),
                    DataSchema::IsString,
                )]))),
                exists: None,
                at: vec![],
            }),
        )])),
    });

    Scenario {
        name: "json-array-schema".into(),
        description: "Validate array elements in JSON with `forall`".into(),
        steps: vec![
            ScenarioStep::Prose(
                "The `is-array` schema checks array contents. `forall` requires every \
                 element to match. `exists` requires at least one. `at` checks specific \
                 indices."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/.config").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/users.json").unwrap(),
                content: "{\"users\":[{\"name\":\"alice\"},{\"id\":42}]}\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["missing key".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.config/users.json").unwrap(),
                content: "{\"users\":[{\"name\":\"alice\"},{\"name\":\"bob\"}]}\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml_all.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunAuditRun {
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml_any.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["hint:".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.bashrc").unwrap(),
                content: "JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunAuditRun {
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
            ScenarioStep::RunAuditRun {
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
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["does not exist".into()],
            },
            ScenarioStep::Prose(
                "When all alternatives fail, `exists` reports one failure per file — \
                 here three files were listed and none satisfied the check, so there \
                 are three failures. In a test.yaml, use `count: 3`, not `count: 1`."
                    .into(),
            ),
            ScenarioStep::Prose(
                "Only one file needs to satisfy the check for `exists` to pass:".into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.zshrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
            ScenarioStep::RunAuditRun {
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
            ScenarioStep::RunAuditRun { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

// ---------------------------------------------------------------------------
// Negation and implication
// ---------------------------------------------------------------------------

fn scenario_not() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.env.local").unwrap(),
        check: FilePredicateAst::Not(Box::new(FilePredicateAst::TextMatchesRegex(
            r"(?i)password\s*=\s*\S+".into(),
        ))),
    });

    Scenario {
        name: "not".into(),
        description: "Negate a check with `not`".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`not` inverts a predicate: it passes when the inner check fails, \
                 and fails when the inner check passes. Use it to assert that \
                 something is absent."
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.env.local").unwrap(),
                content: "API_KEY=abc123\npassword = secret\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["expected check to fail but it passed".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.env.local").unwrap(),
                content: "API_KEY=abc123\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_conditionally() -> Scenario {
    let yaml = gen(&Proposition::FileSatisfies {
        path: SimplePath::new("~/.npmrc").unwrap(),
        check: FilePredicateAst::Conditionally {
            condition: Box::new(FilePredicateAst::FileExists),
            then: Box::new(FilePredicateAst::TextMatchesRegex(r"^registry=".into())),
        },
    });

    Scenario {
        name: "conditionally".into(),
        description: "Conditional check with `conditionally` (if-then)".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`conditionally` evaluates the `then` branch only when the `if` \
                 condition passes. If the condition fails, the whole check is \
                 vacuously true. This is useful for 'if the file exists, then \
                 it must contain X'."
                    .into(),
            ),
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::Prose(
                "The file does not exist, so the condition fails and the check \
                 passes vacuously. Now create the file without the required line:"
                    .into(),
            ),
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.npmrc").unwrap(),
                content: "# empty npmrc\n".into(),
            },
            ScenarioStep::RunAuditRun {
                yaml_content: yaml.clone(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["no line matches regex".into()],
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/.npmrc").unwrap(),
                content: "registry=https://registry.npmjs.org/\n".into(),
            },
            ScenarioStep::RunAuditRun { yaml_content: yaml },
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
        check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
            "settings".into(),
            DataSchema::IsObject(vec![("theme".into(), DataSchema::Anything)]),
        )])),
    };
    let yaml = gen(&prop);

    Scenario {
        name: "audit-test".into(),
        description: "Use `key audit test` to verify expected failures".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`key audit test` evaluates controls against a fake home directory. \
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
            ScenarioStep::RunAuditTest {
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
            ScenarioStep::RunAuditTest {
                yaml_content: yaml,
                expect_failure_messages: vec!["missing key".into()],
                expect_num_failures: Some(1),
            },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

// ---------------------------------------------------------------------------
// Project scenarios
// ---------------------------------------------------------------------------

/// Helper: generate a control file YAML string with given controls.
fn gen_cf(controls: Vec<Control>) -> String {
    crate::rules::generate::generate_control_file(&ControlFile { controls })
}

/// Helper: generate a tests.yaml string.
fn gen_tf(test_suites: Vec<TestSuite>) -> String {
    crate::rules::generate::generate_test_file(&TestFile { test_suites })
}

/// A simple control that checks ~/target_file exists.
fn simple_file_exists_control(id: &str, path: &str) -> Control {
    Control {
        id: id.to_string(),
        title: format!("{} exists", path),
        description: format!("Check that {} exists", path),
        remediation: format!("Create {}", path),
        check: Proposition::FileSatisfies {
            path: SimplePath::new(path).unwrap(),
            check: FilePredicateAst::FileExists,
        },
    }
}

fn scenario_project_new() -> Scenario {
    Scenario {
        name: "project-new".into(),
        description: "Scaffold a new audit project with `key audit project new`".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`key audit project new <name>` creates a project directory with \
                 the standard layout: src/main/<name>.yaml, src/test/tests.yaml, \
                 src/test/resources/, and .gitignore."
                    .into(),
            ),
            ScenarioStep::RunAuditProjectNew {
                work_dir: SimplePath::new("~").unwrap(),
                name: "demo-audit".into(),
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::AssertFileExists {
                path: SimplePath::new("~/demo-audit/src/main/demo-audit.yaml").unwrap(),
            },
            ScenarioStep::AssertFileExists {
                path: SimplePath::new("~/demo-audit/src/test/tests.yaml").unwrap(),
            },
            ScenarioStep::AssertFileExists {
                path: SimplePath::new("~/demo-audit/.gitignore").unwrap(),
            },
        ],
    }
}

fn scenario_project_test_pass() -> Scenario {
    // Case 1 (valid fixture, control passes, expect pass) +
    // Case 4 (invalid fixture, control fails, expect fail)
    let ctrl = simple_file_exists_control("CTRL-0001", "~/.ssh/config");
    let yaml_cf = gen_cf(vec![ctrl]);
    let yaml_tf = gen_tf(vec![TestSuite {
        name: "SSH checks".into(),
        description: Some("Verify SSH controls".into()),
        tests: vec![
            TestCase {
                control_id: "CTRL-0001".into(),
                description: "valid SSH setup passes".into(),
                fixture: "CTRL-0001-valid".into(),
                expect: TestExpectation::Pass,
            },
            TestCase {
                control_id: "CTRL-0001".into(),
                description: "missing config detected".into(),
                fixture: "CTRL-0001-invalid".into(),
                expect: TestExpectation::Fail(FailExpectation {
                    count: Some(1),
                    messages: vec!["does not exist".into()],
                }),
            },
        ],
    }]);

    Scenario {
        name: "project-test-pass".into(),
        description: "Project test: valid fixture passes, invalid fixture caught (cases 1+4)"
            .into(),
        steps: vec![
            ScenarioStep::Prose(
                "When a control passes on a valid fixture and fails on an invalid one, \
                 both tests report PASS — the control is working as expected."
                    .into(),
            ),
            // Set up project structure manually
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/proj/src/main").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/proj/src/main/proj.yaml").unwrap(),
                content: yaml_cf,
            },
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/proj/src/test/resources/CTRL-0001-valid/.ssh").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/proj/src/test/resources/CTRL-0001-valid/.ssh/config")
                    .unwrap(),
                content: "Host *\n".into(),
            },
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/proj/src/test/resources/CTRL-0001-invalid").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/proj/src/test/tests.yaml").unwrap(),
                content: yaml_tf,
            },
            ScenarioStep::RunAuditProjectTest {
                project_dir: SimplePath::new("~/proj").unwrap(),
            },
            ScenarioStep::ExpectSuccess,
        ],
    }
}

fn scenario_project_test_fp_detected() -> Scenario {
    // Case 2: control reports failure on a valid fixture (false positive)
    // We'll use a control that checks for a regex that the valid fixture doesn't match
    let ctrl = Control {
        id: "CTRL-FP".into(),
        title: "Bashrc has export".into(),
        description: "Check export".into(),
        remediation: "Add export".into(),
        check: Proposition::FileSatisfies {
            path: SimplePath::new("~/.bashrc").unwrap(),
            check: FilePredicateAst::ShellExports("NONEXISTENT_VAR".into()),
        },
    };
    let yaml_cf = gen_cf(vec![ctrl]);
    let yaml_tf = gen_tf(vec![TestSuite {
        name: "FP test".into(),
        description: None,
        tests: vec![TestCase {
            control_id: "CTRL-FP".into(),
            description: "valid setup should pass but control is buggy".into(),
            fixture: "valid-setup".into(),
            expect: TestExpectation::Pass,
        }],
    }]);

    Scenario {
        name: "project-test-fp-detected".into(),
        description: "Project test detects false positive (case 2)".into(),
        steps: vec![
            ScenarioStep::Prose(
                "When a control fails on a valid fixture but the test expects pass, \
                 the test runner reports the failure as a false positive detected by the test."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/fp-proj/src/main").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/fp-proj/src/main/fp.yaml").unwrap(),
                content: yaml_cf,
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/fp-proj/src/test/resources/valid-setup/.bashrc").unwrap(),
                content: "export JAVA_HOME=/usr/lib/jvm\n".into(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/fp-proj/src/test/tests.yaml").unwrap(),
                content: yaml_tf,
            },
            ScenarioStep::RunAuditProjectTest {
                project_dir: SimplePath::new("~/fp-proj").unwrap(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["expected pass but control reported".into()],
            },
        ],
    }
}

fn scenario_project_test_fn_detected() -> Scenario {
    // Case 3: control passes on an invalid fixture (false negative)
    // Use a control that always passes (file-exists on a file that exists in the invalid fixture)
    let ctrl = simple_file_exists_control("CTRL-FN", "~/.bashrc");
    let yaml_cf = gen_cf(vec![ctrl]);
    let yaml_tf = gen_tf(vec![TestSuite {
        name: "FN test".into(),
        description: None,
        tests: vec![TestCase {
            control_id: "CTRL-FN".into(),
            description: "invalid setup should fail but control misses it".into(),
            fixture: "invalid-setup".into(),
            expect: TestExpectation::Fail(FailExpectation {
                count: None,
                messages: vec![],
            }),
        }],
    }]);

    Scenario {
        name: "project-test-fn-detected".into(),
        description: "Project test detects false negative (case 3)".into(),
        steps: vec![
            ScenarioStep::Prose(
                "When a control passes on an invalid fixture but the test expects failure, \
                 the test runner reports a false negative — the control is not catching the problem."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/fn-proj/src/main").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/fn-proj/src/main/fn.yaml").unwrap(),
                content: yaml_cf,
            },
            // Invalid fixture but it has .bashrc (so control passes — that's the FN)
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/fn-proj/src/test/resources/invalid-setup/.bashrc").unwrap(),
                content: "# this file exists, so file-exists passes, but it shouldn't\n".into(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/fn-proj/src/test/tests.yaml").unwrap(),
                content: yaml_tf,
            },
            ScenarioStep::RunAuditProjectTest {
                project_dir: SimplePath::new("~/fn-proj").unwrap(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec![
                    "expected failure but control passed".into(),
                ],
            },
        ],
    }
}

fn scenario_project_test_bad_control_id() -> Scenario {
    let ctrl = simple_file_exists_control("CTRL-0001", "~/.ssh/config");
    let yaml_cf = gen_cf(vec![ctrl]);
    let yaml_tf = gen_tf(vec![TestSuite {
        name: "Bad ID test".into(),
        description: None,
        tests: vec![TestCase {
            control_id: "CTRL-9999".into(),
            description: "references nonexistent control".into(),
            fixture: "some-fixture".into(),
            expect: TestExpectation::Pass,
        }],
    }]);

    Scenario {
        name: "project-test-bad-control-id".into(),
        description: "Project test: unknown control ID gives a clear error".into(),
        steps: vec![
            ScenarioStep::Prose(
                "If tests.yaml references a control ID that doesn't exist in the main \
                 YAML file, the runner produces a clear authoring error."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/bad-id/src/main").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/bad-id/src/main/bad.yaml").unwrap(),
                content: yaml_cf,
            },
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/bad-id/src/test/resources/some-fixture").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/bad-id/src/test/tests.yaml").unwrap(),
                content: yaml_tf,
            },
            ScenarioStep::RunAuditProjectTest {
                project_dir: SimplePath::new("~/bad-id").unwrap(),
            },
            ScenarioStep::ExpectFailure {
                messages: vec!["unknown control ID".into(), "CTRL-9999".into()],
            },
        ],
    }
}

fn scenario_project_build() -> Scenario {
    let ctrl = simple_file_exists_control("CTRL-0001", "~/.ssh/config");
    let yaml_cf = gen_cf(vec![ctrl]);
    let yaml_tf = gen_tf(vec![TestSuite {
        name: "Build test".into(),
        description: None,
        tests: vec![TestCase {
            control_id: "CTRL-0001".into(),
            description: "valid passes".into(),
            fixture: "valid".into(),
            expect: TestExpectation::Pass,
        }],
    }]);

    Scenario {
        name: "project-build".into(),
        description: "Successful build copies controls to target/".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`key audit project build` validates controls and tests, then copies \
                 the main YAML to target/."
                    .into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/build-proj/src/main").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/build-proj/src/main/build.yaml").unwrap(),
                content: yaml_cf,
            },
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/build-proj/src/test/resources/valid/.ssh").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/build-proj/src/test/resources/valid/.ssh/config").unwrap(),
                content: "Host *\n".into(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/build-proj/src/test/tests.yaml").unwrap(),
                content: yaml_tf,
            },
            ScenarioStep::RunAuditProjectBuild {
                project_dir: SimplePath::new("~/build-proj").unwrap(),
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::AssertFileExists {
                path: SimplePath::new("~/build-proj/target/build.yaml").unwrap(),
            },
        ],
    }
}

fn scenario_project_clean() -> Scenario {
    Scenario {
        name: "project-clean".into(),
        description: "Clean removes the target/ directory".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`key audit project clean` removes the target/ directory if it exists.".into(),
            ),
            ScenarioStep::CreateDir {
                path: SimplePath::new("~/clean-proj/target").unwrap(),
            },
            ScenarioStep::WriteFile {
                path: SimplePath::new("~/clean-proj/target/something.yaml").unwrap(),
                content: "placeholder\n".into(),
            },
            ScenarioStep::RunAuditProjectClean {
                project_dir: SimplePath::new("~/clean-proj").unwrap(),
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::AssertDirMissing {
                path: SimplePath::new("~/clean-proj/target").unwrap(),
            },
        ],
    }
}

fn scenario_audit_install_and_pick() -> Scenario {
    let ctrl = simple_file_exists_control("CTRL-0001", "~/.ssh/config");
    let yaml_cf = gen_cf(vec![ctrl]);

    Scenario {
        name: "audit-install-and-pick".into(),
        description: "Install an audit config for use with the picker".into(),
        steps: vec![
            ScenarioStep::Prose(
                "`key audit install <file.yaml>` validates and copies an audit config \
                 into ~/.key/audit-configs/. Running bare `key audit` shows a picker \
                 of installed configs."
                    .into(),
            ),
            ScenarioStep::RunAuditInstall {
                yaml_content: yaml_cf,
                config_name: "my-audit.yaml".into(),
            },
            ScenarioStep::ExpectSuccess,
            ScenarioStep::AssertFileExists {
                path: SimplePath::new("~/.key/audit-configs/my-audit.yaml").unwrap(),
            },
        ],
    }
}
