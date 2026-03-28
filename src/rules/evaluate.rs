use std::path::Path;

use crate::rules::ast::{Proposition, RuleFailure};
use crate::rules::predicates::evaluate_predicate;

/// Evaluate a proposition against a home directory.
/// Returns `Ok(())` if all checks pass, `Err(failures)` otherwise.
pub fn evaluate(proposition: &Proposition, home_dir: &Path) -> Result<(), Vec<RuleFailure>> {
    let mut failures = Vec::new();
    eval_prop(proposition, home_dir, &mut failures);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn eval_prop(prop: &Proposition, home_dir: &Path, failures: &mut Vec<RuleFailure>) {
    match prop {
        Proposition::FileSatisfies { path, check } => {
            let resolved = path.resolve(home_dir);
            if let Err(msg) = evaluate_predicate(check, &resolved) {
                failures.push(RuleFailure {
                    path: path.as_str().to_string(),
                    message: msg,
                });
            }
        }
        Proposition::Forall { files, check } => {
            for f in files {
                let resolved = f.resolve(home_dir);
                if let Err(msg) = evaluate_predicate(check, &resolved) {
                    failures.push(RuleFailure {
                        path: f.as_str().to_string(),
                        message: msg,
                    });
                }
            }
        }
        Proposition::Exists { files, check } => {
            let mut any_ok = false;
            let mut errs = Vec::new();
            for f in files {
                let resolved = f.resolve(home_dir);
                match evaluate_predicate(check, &resolved) {
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
                eval_prop(p, home_dir, failures);
            }
        }
        Proposition::Any(props) => {
            // Try each; if any succeeds, the whole thing passes
            let mut branch_failures: Vec<Vec<RuleFailure>> = Vec::new();
            for p in props {
                let mut branch = Vec::new();
                eval_prop(p, home_dir, &mut branch);
                if branch.is_empty() {
                    return; // success
                }
                branch_failures.push(branch);
            }
            // All branches failed — report all failures from all branches
            for branch in branch_failures {
                failures.extend(branch);
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
    fn json_matches_query() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join("data.json"), r#"{"user":{"name":"alice"}}"#).unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/data.json").unwrap(),
            check: FilePredicateAst::JsonMatchesQuery(".user.name".into()),
        };
        assert!(evaluate(&prop, home.path()).is_ok());
    }

    #[test]
    fn yaml_matches_query() {
        let home = with_temp_home(|h| {
            std::fs::write(h.join("cfg.yaml"), "models:\n  - name: gpt4\n").unwrap();
        });
        let prop = Proposition::FileSatisfies {
            path: SimplePath::new("~/cfg.yaml").unwrap(),
            check: FilePredicateAst::YamlMatchesQuery("models[0].name".into()),
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
        // ShellDefinesVariable matches "JAVA_HOME=/usr" (no export needed)
        assert!(evaluate(&prop, home.path()).is_ok());
    }
}
