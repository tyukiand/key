use std::path::Path;

use crate::effects::OsEffectsRo;
use crate::rules::ast::{Proposition, RuleFailure};
use crate::rules::predicates::{evaluate_predicate_subject, Subject};
use crate::rules::pseudo::EvalContext;

/// Evaluate a proposition against a home directory (production entry point).
/// Equivalent to evaluating with no pseudo-file overrides.
pub fn evaluate(proposition: &Proposition, home_dir: &Path) -> Result<(), Vec<RuleFailure>> {
    let ctx = EvalContext::new(home_dir.to_path_buf());
    evaluate_with_ctx(proposition, &ctx)
}

/// Evaluate against a home directory using a caller-supplied OsEffects handle.
/// Spec/0017 §C.5: the project's `unredacted:` allowlist is threaded into a
/// `RealOsEffects::with_unredacted(...)` at the project-command boundary so
/// every env / file read funnels through the right redaction context.
pub fn evaluate_with_os(
    proposition: &Proposition,
    home_dir: &Path,
    os: Box<dyn OsEffectsRo>,
) -> Result<(), Vec<RuleFailure>> {
    let ctx = EvalContext::with_fixture_and_os(
        home_dir.to_path_buf(),
        crate::rules::ast::PseudoFileFixture::default(),
        os,
    );
    evaluate_with_ctx(proposition, &ctx)
}

/// Evaluate a proposition with an explicit `EvalContext` (test harness entry
/// point — supports `env_override` / `executable_override` per spec §2.5, §3.8).
pub fn evaluate_with_ctx(
    proposition: &Proposition,
    ctx: &EvalContext,
) -> Result<(), Vec<RuleFailure>> {
    let mut failures = Vec::new();
    eval_prop(proposition, ctx, &mut failures);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn eval_prop(prop: &Proposition, ctx: &EvalContext, failures: &mut Vec<RuleFailure>) {
    match prop {
        Proposition::FileSatisfies { path, check } => {
            if let Some(pseudo) = path.pseudo() {
                let snap = ctx.resolve(pseudo);
                let subject = Subject::Pseudo(pseudo, &snap);
                if let Err(msg) = evaluate_predicate_subject(check, &subject) {
                    failures.push(RuleFailure {
                        path: path.as_str().to_string(),
                        message: msg,
                    });
                }
            } else {
                let resolved = path.resolve(&ctx.home_dir);
                let subject = Subject::Concrete(resolved);
                if let Err(msg) = evaluate_predicate_subject(check, &subject) {
                    failures.push(RuleFailure {
                        path: path.as_str().to_string(),
                        message: msg,
                    });
                }
            }
        }
        Proposition::Forall { files, check } => {
            for f in files {
                if let Some(pseudo) = f.pseudo() {
                    let snap = ctx.resolve(pseudo);
                    let subject = Subject::Pseudo(pseudo, &snap);
                    if let Err(msg) = evaluate_predicate_subject(check, &subject) {
                        failures.push(RuleFailure {
                            path: f.as_str().to_string(),
                            message: msg,
                        });
                    }
                } else {
                    let resolved = f.resolve(&ctx.home_dir);
                    let subject = Subject::Concrete(resolved);
                    if let Err(msg) = evaluate_predicate_subject(check, &subject) {
                        failures.push(RuleFailure {
                            path: f.as_str().to_string(),
                            message: msg,
                        });
                    }
                }
            }
        }
        Proposition::Exists { files, check } => {
            let mut any_ok = false;
            let mut errs = Vec::new();
            for f in files {
                let result = if let Some(pseudo) = f.pseudo() {
                    let snap = ctx.resolve(pseudo);
                    let subject = Subject::Pseudo(pseudo, &snap);
                    evaluate_predicate_subject(check, &subject)
                } else {
                    let resolved = f.resolve(&ctx.home_dir);
                    let subject = Subject::Concrete(resolved);
                    evaluate_predicate_subject(check, &subject)
                };
                match result {
                    Ok(()) => {
                        any_ok = true;
                        break;
                    }
                    Err(msg) => errs.push((f.as_str().to_string(), msg)),
                }
            }
            if !any_ok {
                for (path, message) in errs {
                    failures.push(RuleFailure { path, message });
                }
            }
        }
        Proposition::All(props) => {
            for p in props {
                eval_prop(p, ctx, failures);
            }
        }
        Proposition::Any(props) => {
            let mut branch_failures: Vec<Vec<RuleFailure>> = Vec::new();
            for p in props {
                let mut branch = Vec::new();
                eval_prop(p, ctx, &mut branch);
                if branch.is_empty() {
                    return;
                }
                branch_failures.push(branch);
            }
            for branch in branch_failures {
                failures.extend(branch);
            }
        }
        Proposition::Not(inner) => {
            let mut inner_failures = Vec::new();
            eval_prop(inner, ctx, &mut inner_failures);
            if inner_failures.is_empty() {
                failures.push(RuleFailure {
                    path: "(not)".to_string(),
                    message: "expected check to fail but it passed".to_string(),
                });
            }
        }
        Proposition::Conditionally { condition, then } => {
            let mut cond_failures = Vec::new();
            eval_prop(condition, ctx, &mut cond_failures);
            if cond_failures.is_empty() {
                eval_prop(then, ctx, failures);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ast::{FilePredicateAst, SimplePath};

    fn with_temp_home(setup: impl FnOnce(&Path)) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        setup(tmp.path());
        tmp
    }

    #[test]
    fn file_exists_pass() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join("test.txt"), "hello").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/test.txt").unwrap(),
            check: FilePredicateAst::FileExists,
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn file_exists_fail() {
        let home = with_temp_home(|_| {});
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/missing.txt").unwrap(),
            check: FilePredicateAst::FileExists,
        };
        let err = evaluate(&prop, home.path()).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("does not exist"));
    }

    #[test]
    fn shell_exports_pass() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join(".bashrc"), "export JAVA_HOME=/usr/lib/jvm\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/.bashrc").unwrap(),
            check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn shell_exports_fail() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join(".bashrc"), "# nothing here\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/.bashrc").unwrap(),
            check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
        };
        assert!(evaluate(&prop, home.path()).is_err());
    }

    #[test]
    fn shell_defines_variable_pass() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join(".bashrc"), "MY_VAR=hello\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/.bashrc").unwrap(),
            check: FilePredicateAst::ShellDefinesVariable("MY_VAR".into()),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn forall_partial_fail() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join(".bashrc"), "export JAVA_HOME=/usr\n").unwrap();
            std::fs::write(h.join(".zshrc"), "# empty\n").unwrap();
        });
        let prop = Proposition::Forall {
            files: vec![
                SimplePath::new("~/.bashrc").unwrap(),
                SimplePath::new("~/.zshrc").unwrap(),
            ],
            check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
        };
        let err = evaluate(&prop, home.path()).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].path, "~/.zshrc");
    }

    #[test]
    fn exists_one_passes() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join(".zshrc"), "export JAVA_HOME=/usr\n").unwrap();
        });
        let prop = Proposition::Exists {
            files: vec![
                SimplePath::new("~/.bashrc").unwrap(),
                SimplePath::new("~/.zshrc").unwrap(),
            ],
            check: FilePredicateAst::ShellExports("JAVA_HOME".into()),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn exists_none_pass() {
        let home = with_temp_home(|_| {});
        let prop = Proposition::Exists {
            files: vec![
                SimplePath::new("~/.bashrc").unwrap(),
                SimplePath::new("~/.zshrc").unwrap(),
            ],
            check: FilePredicateAst::FileExists,
        };
        let err = evaluate(&prop, home.path()).unwrap_err();
        assert_eq!(err.len(), 2);
    }

    #[test]
    fn all_proposition() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join("a.txt"), "hello").unwrap();
        });
        let prop = Proposition::All(vec![
            Proposition::FileSatisfies {
                path: SimplePath::new("~/a.txt").unwrap(),
                check: FilePredicateAst::FileExists,
            },
            Proposition::FileSatisfies {
                path: SimplePath::new("~/b.txt").unwrap(),
                check: FilePredicateAst::FileExists,
            },
        ]);
        let err = evaluate(&prop, home.path()).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err[0].path.contains("b.txt"));
    }

    #[test]
    fn any_proposition_one_passes() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join("b.txt"), "hi").unwrap();
        });
        let prop = Proposition::Any(vec![
            Proposition::FileSatisfies {
                path: SimplePath::new("~/a.txt").unwrap(),
                check: FilePredicateAst::FileExists,
            },
            Proposition::FileSatisfies {
                path: SimplePath::new("~/b.txt").unwrap(),
                check: FilePredicateAst::FileExists,
            },
        ]);
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn properties_defines_key() {
        let home = with_temp_home(|h| {
            std::fs::create_dir_all(h.join(".gradle")).unwrap();
            std::fs::write(
                h.join(".gradle/gradle.properties"),
                "signing.keyId=ABC123\norg.gradle.parallel=true\n",
            )
            .unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/.gradle/gradle.properties").unwrap(),
            check: FilePredicateAst::PropertiesDefinesKey("signing.keyId".into()),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn text_has_lines() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join("f.txt"), "a\nb\nc\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/f.txt").unwrap(),
            check: FilePredicateAst::TextHasLines {
                min: Some(2),
                max: Some(5),
            },
        };
        assert!(evaluate(&prop, home.path()).is_ok());

        let too_few = Proposition::FileSatisfies {
            path: SimplePath::new("~/f.txt").unwrap(),
            check: FilePredicateAst::TextHasLines {
                min: Some(10),
                max: None,
            },
        };
        assert!(evaluate(&too_few, home.path()).is_err());
    }

    #[test]
    fn xml_matches_path() {
        let home = with_temp_home(|h| {
            std::fs::write(
                h.join("settings.xml"),
                "<settings><servers><server><token>x</token></server></servers></settings>",
            )
            .unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/settings.xml").unwrap(),
            check: FilePredicateAst::XmlMatchesPath("settings/servers/server/token".into()),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn json_matches_schema() {
        use crate::rules::ast::DataSchema;
        let home = with_temp_home(|h| {
            std::fs::write(h.join("data.json"), r#"{"user":{"name":"alice"}}"#).unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/data.json").unwrap(),
            check: FilePredicateAst::JsonMatches(DataSchema::IsObject(vec![(
                "user".into(),
                DataSchema::IsObject(vec![("name".into(), DataSchema::IsString)]),
            )])),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn yaml_matches_schema() {
        use crate::rules::ast::{DataArrayCheck, DataSchema};
        let home = with_temp_home(|h| {
            std::fs::write(h.join("cfg.yaml"), "models:\n  - name: gpt4\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/cfg.yaml").unwrap(),
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
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn predicate_any() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join(".bashrc"), "JAVA_HOME=/usr\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/.bashrc").unwrap(),
            check: FilePredicateAst::Any {
                hint: "set up Java".into(),
                checks: vec![
                    FilePredicateAst::ShellExports("JAVA_HOME".into()),
                    FilePredicateAst::ShellDefinesVariable("JAVA_HOME".into()),
                ],
            },
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }
}
