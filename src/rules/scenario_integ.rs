/// Integration test interpreter: executes scenarios by invoking the compiled binary.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use crate::rules::scenario::{all_scenarios, ScenarioStep};

    fn bin_path() -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest).join("target/debug/key")
    }

    #[test]
    fn run_all_scenarios_integ() {
        let bin = bin_path();

        for scenario in all_scenarios() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path();
            let yaml_dir = tmp.path().join("__yaml");
            std::fs::create_dir_all(&yaml_dir).unwrap();

            let mut last_output: Option<std::process::Output> = None;
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

                        let output = Command::new(&bin)
                            .arg("--test-only-home-dir")
                            .arg(home)
                            .arg("audit")
                            .arg("run")
                            .arg("--file")
                            .arg(&yaml_path)
                            .output()
                            .expect("run key binary");

                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditTest {
                        yaml_content,
                        expect_failure_messages,
                        expect_num_failures,
                    } => {
                        yaml_counter += 1;
                        let yaml_path = yaml_dir.join(format!("audit_{}.yaml", yaml_counter));
                        std::fs::write(&yaml_path, yaml_content).unwrap();

                        let mut cmd = Command::new(&bin);
                        cmd.arg("--test-only-home-dir")
                            .arg(home)
                            .arg("audit")
                            .arg("test")
                            .arg(&yaml_path)
                            .arg(home);

                        for msg in expect_failure_messages {
                            cmd.arg("--expect-failure-message").arg(msg);
                        }
                        if let Some(n) = expect_num_failures {
                            cmd.arg("--expect-failures").arg(n.to_string());
                        }

                        let output = cmd.output().expect("run key binary");
                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditProjectNew { work_dir, name } => {
                        let resolved = work_dir.resolve(home);
                        std::fs::create_dir_all(&resolved).unwrap();

                        let output = Command::new(&bin)
                            .arg("audit")
                            .arg("project")
                            .arg("new")
                            .arg(name)
                            .current_dir(&resolved)
                            .output()
                            .expect("run key binary");
                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditProjectTest { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let output = Command::new(&bin)
                            .arg("audit")
                            .arg("project")
                            .arg("test")
                            .current_dir(&resolved)
                            .output()
                            .expect("run key binary");
                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditProjectBuild { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let output = Command::new(&bin)
                            .arg("audit")
                            .arg("project")
                            .arg("build")
                            .current_dir(&resolved)
                            .output()
                            .expect("run key binary");
                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditProjectClean { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let output = Command::new(&bin)
                            .arg("audit")
                            .arg("project")
                            .arg("clean")
                            .current_dir(&resolved)
                            .output()
                            .expect("run key binary");
                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditProjectRun { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let output = Command::new(&bin)
                            .arg("--test-only-home-dir")
                            .arg(home)
                            .arg("audit")
                            .arg("project")
                            .arg("run")
                            .current_dir(&resolved)
                            .output()
                            .expect("run key binary");
                        last_output = Some(output);
                    }
                    ScenarioStep::RunAuditInstall {
                        yaml_content,
                        config_name,
                    } => {
                        yaml_counter += 1;
                        let yaml_path = yaml_dir.join(config_name);
                        std::fs::write(&yaml_path, yaml_content).unwrap();

                        let output = Command::new(&bin)
                            .arg("--test-only-home-dir")
                            .arg(home)
                            .arg("audit")
                            .arg("install")
                            .arg(&yaml_path)
                            .output()
                            .expect("run key binary");
                        last_output = Some(output);
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
                        if !output.status.success() {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            panic!(
                                "Scenario {:?} step {}: expected success but got exit {}\nstdout: {}\nstderr: {}",
                                scenario.name,
                                step_idx,
                                output.status.code().unwrap_or(-1),
                                stdout,
                                stderr
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
                        if output.status.success() {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            panic!(
                                "Scenario {:?} step {}: expected failure but got success\nstdout: {}",
                                scenario.name, step_idx, stdout
                            );
                        }
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let combined = format!("{}\n{}", stdout, stderr);
                        for msg in messages {
                            assert!(
                                combined.contains(msg.as_str()),
                                "Scenario {:?} step {}: expected output to contain {:?}\nstdout: {}\nstderr: {}",
                                scenario.name,
                                step_idx,
                                msg,
                                stdout,
                                stderr
                            );
                        }
                    }
                }
            }
        }
    }
}
