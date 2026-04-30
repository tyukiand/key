/// Integration test interpreter: executes scenarios by invoking the compiled binary.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::rules::scenario::{all_scenarios, ScenarioStep};
    use crate::security::exec::{
        safe_exec, AllowedCommand, AllowedExecutablePath, AllowedSelfArgs, SafeExecResult,
    };

    fn bin_path() -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest).join("target/debug/key")
    }

    fn run_self(bin: &PathBuf, args: AllowedSelfArgs) -> SafeExecResult {
        let exe = AllowedExecutablePath::new(bin)
            .expect("test binary path must be absolute and free of `..`");
        safe_exec(AllowedCommand::AuditSelf { binary: exe, args })
    }

    #[test]
    fn run_all_scenarios_integ() {
        let bin = bin_path();

        for scenario in all_scenarios() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path();
            let yaml_dir = tmp.path().join("__yaml");
            std::fs::create_dir_all(&yaml_dir).unwrap();

            let mut last_output: Option<SafeExecResult> = None;
            let mut yaml_counter = 0u32;

            for (step_idx, step) in scenario.steps.iter().enumerate() {
                match step {
                    ScenarioStep::Prose(_) => {}
                    ScenarioStep::WriteFile { path, content } => {
                        let resolved = path.resolve(home);
                        if let Some(parent) = resolved.parent() {
                            std::fs::create_dir_all(parent).unwrap();
                        }
                        std::fs::write(&resolved, content).unwrap();
                    }
                    ScenarioStep::CreateDir { path } => {
                        let resolved = path.resolve(home);
                        std::fs::create_dir_all(&resolved).unwrap();
                    }
                    ScenarioStep::RunAuditRun { yaml_content } => {
                        yaml_counter += 1;
                        let yaml_path = yaml_dir.join(format!("audit_{}.yaml", yaml_counter));
                        std::fs::write(&yaml_path, yaml_content).unwrap();

                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditRunFile {
                                home: home.to_path_buf(),
                                yaml: yaml_path,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditTest {
                        yaml_content,
                        expect_failure_messages,
                        expect_num_failures,
                    } => {
                        yaml_counter += 1;
                        let yaml_path = yaml_dir.join(format!("audit_{}.yaml", yaml_counter));
                        std::fs::write(&yaml_path, yaml_content).unwrap();

                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditTest {
                                home: home.to_path_buf(),
                                yaml: yaml_path,
                                expect_failure_messages: expect_failure_messages.clone(),
                                expect_num_failures: *expect_num_failures,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditProjectNew { work_dir, name } => {
                        let resolved = work_dir.resolve(home);
                        std::fs::create_dir_all(&resolved).unwrap();

                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditProjectNew {
                                work_dir: resolved,
                                name: name.clone(),
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditProjectTest { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditProjectTest {
                                project_dir: resolved,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditProjectBuild { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditProjectBuild {
                                project_dir: resolved,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditProjectClean { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditProjectClean {
                                project_dir: resolved,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditProjectRun { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditProjectRun {
                                home: home.to_path_buf(),
                                project_dir: resolved,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::RunAuditInstall {
                        yaml_content,
                        config_name,
                    } => {
                        yaml_counter += 1;
                        let yaml_path = yaml_dir.join(config_name);
                        std::fs::write(&yaml_path, yaml_content).unwrap();

                        let result = run_self(
                            &bin,
                            AllowedSelfArgs::AuditInstall {
                                home: home.to_path_buf(),
                                yaml: yaml_path,
                            },
                        );
                        last_output = Some(result);
                    }
                    ScenarioStep::AssertFileExists { path } => {
                        let resolved = path.resolve(home);
                        assert!(
                            resolved.exists(),
                            "Scenario {:?} step {}: expected file to exist: {}",
                            scenario.name,
                            step_idx,
                            resolved.display()
                        );
                    }
                    ScenarioStep::AssertDirMissing { path } => {
                        let resolved = path.resolve(home);
                        assert!(
                            !resolved.is_dir(),
                            "Scenario {:?} step {}: expected directory to NOT exist: {}",
                            scenario.name,
                            step_idx,
                            resolved.display()
                        );
                    }
                    ScenarioStep::ExpectSuccess => {
                        let output = last_output.take().unwrap_or_else(|| {
                            panic!(
                                "Scenario {:?} step {}: ExpectSuccess with no preceding run",
                                scenario.name, step_idx
                            )
                        });
                        if !output.success {
                            panic!(
                                "Scenario {:?} step {}: expected success but got exit {}\nstdout: {}\nstderr: {}",
                                scenario.name,
                                step_idx,
                                output.exit.unwrap_or(-1),
                                output.stdout,
                                output.stderr
                            );
                        }
                    }
                    ScenarioStep::ExpectFailure { messages } => {
                        let output = last_output.take().unwrap_or_else(|| {
                            panic!(
                                "Scenario {:?} step {}: ExpectFailure with no preceding run",
                                scenario.name, step_idx
                            )
                        });
                        if output.success {
                            panic!(
                                "Scenario {:?} step {}: expected failure but got success\nstdout: {}",
                                scenario.name, step_idx, output.stdout
                            );
                        }
                        let combined = format!("{}\n{}", output.stdout, output.stderr);
                        for msg in messages {
                            assert!(
                                combined.contains(msg.as_str()),
                                "Scenario {:?} step {}: expected output to contain {:?}\nstdout: {}\nstderr: {}",
                                scenario.name,
                                step_idx,
                                msg,
                                output.stdout,
                                output.stderr
                            );
                        }
                    }
                }
            }
        }
    }
}
