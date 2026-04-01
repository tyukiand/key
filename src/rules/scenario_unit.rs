/// Unit test interpreter: executes scenarios using internal Rust APIs.
#[cfg(test)]
mod tests {
    use crate::effects::CannedEffects;
    use crate::rules::ast::RuleFailure;
    use crate::rules::evaluate::evaluate;
    use crate::rules::parse::parse_control_file;
    use crate::rules::scenario::{all_scenarios, ScenarioStep};

    /// Evaluate all controls in a control file, merging failures.
    fn evaluate_control_file(yaml: &str, home: &std::path::Path) -> Result<(), Vec<RuleFailure>> {
        let cf = parse_control_file(yaml).unwrap();
        let mut all_failures = Vec::new();
        for control in &cf.controls {
            if let Err(failures) = evaluate(&control.check, home) {
                all_failures.extend(failures);
            }
        }
        if all_failures.is_empty() {
            Ok(())
        } else {
            Err(all_failures)
        }
    }

    /// Result type for project commands: Ok(output) or Err(error_message)
    type CmdResult = Result<String, String>;

    fn cmd_result_from_anyhow(r: anyhow::Result<()>, fx: &CannedEffects) -> CmdResult {
        match r {
            Ok(()) => Ok(fx.output()),
            Err(e) => {
                let mut combined = fx.output();
                combined.push_str(&fx.err_output());
                combined.push_str(&format!("{:#}", e));
                Err(combined)
            }
        }
    }

    #[test]
    fn run_all_scenarios_unit() {
        for scenario in all_scenarios() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path();
            let mut last_result: Option<Result<(), Vec<RuleFailure>>> = None;
            let mut last_cmd_result: Option<CmdResult> = None;

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
                        last_result = Some(evaluate_control_file(yaml_content, home));
                        last_cmd_result = None;
                    }
                    ScenarioStep::RunAuditTest {
                        yaml_content,
                        expect_failure_messages,
                        expect_num_failures,
                    } => {
                        let result = evaluate_control_file(yaml_content, home);
                        let expect_pass =
                            expect_failure_messages.is_empty() && expect_num_failures.is_none();
                        match &result {
                            Ok(()) => {
                                if !expect_pass {
                                    panic!(
                                        "Scenario {:?} step {}: expected failures but got success",
                                        scenario.name, step_idx
                                    );
                                }
                            }
                            Err(failures) => {
                                if expect_pass {
                                    panic!(
                                        "Scenario {:?} step {}: expected success but got {} failures: {:?}",
                                        scenario.name,
                                        step_idx,
                                        failures.len(),
                                        failures
                                    );
                                }
                                if let Some(expected_count) = expect_num_failures {
                                    assert_eq!(
                                        failures.len(),
                                        *expected_count,
                                        "Scenario {:?} step {}: expected {} failures, got {}",
                                        scenario.name,
                                        step_idx,
                                        expected_count,
                                        failures.len()
                                    );
                                }
                                let all_output: String = failures
                                    .iter()
                                    .map(|f| f.to_string())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                for msg in expect_failure_messages {
                                    assert!(
                                        all_output.contains(msg.as_str()),
                                        "Scenario {:?} step {}: expected failure containing {:?}, got:\n{}",
                                        scenario.name,
                                        step_idx,
                                        msg,
                                        all_output
                                    );
                                }
                            }
                        }
                        last_result = Some(Ok(()));
                        last_cmd_result = None;
                    }
                    ScenarioStep::RunAuditProjectNew { work_dir, name } => {
                        let resolved = work_dir.resolve(home);
                        std::fs::create_dir_all(&resolved).unwrap();
                        // Temporarily change to work_dir to run project_new
                        let orig_dir = std::env::current_dir().unwrap();
                        std::env::set_current_dir(&resolved).unwrap();
                        let fx = CannedEffects::new();
                        let r = crate::commands::audit::project_new(name, &fx);
                        std::env::set_current_dir(&orig_dir).unwrap();
                        last_cmd_result = Some(cmd_result_from_anyhow(r, &fx));
                        last_result = None;
                    }
                    ScenarioStep::RunAuditProjectTest { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let fx = CannedEffects::new();
                        let r = crate::commands::audit::project_test(&resolved, &fx);
                        last_cmd_result = Some(cmd_result_from_anyhow(r, &fx));
                        last_result = None;
                    }
                    ScenarioStep::RunAuditProjectBuild { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let fx = CannedEffects::new();
                        let r = crate::commands::audit::project_build(&resolved, &fx);
                        last_cmd_result = Some(cmd_result_from_anyhow(r, &fx));
                        last_result = None;
                    }
                    ScenarioStep::RunAuditProjectClean { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let fx = CannedEffects::new();
                        let r = crate::commands::audit::project_clean(&resolved, &fx);
                        last_cmd_result = Some(cmd_result_from_anyhow(r, &fx));
                        last_result = None;
                    }
                    ScenarioStep::RunAuditProjectRun { project_dir } => {
                        let resolved = project_dir.resolve(home);
                        let fx = CannedEffects::new();
                        let r = crate::commands::audit::project_run(&resolved, home, &fx);
                        last_cmd_result = Some(cmd_result_from_anyhow(r, &fx));
                        last_result = None;
                    }
                    ScenarioStep::RunAuditInstall {
                        yaml_content,
                        config_name,
                    } => {
                        // Write yaml to a temp file, then install
                        let yaml_path = tmp.path().join("__install").join(config_name);
                        std::fs::create_dir_all(yaml_path.parent().unwrap()).unwrap();
                        std::fs::write(&yaml_path, yaml_content).unwrap();
                        let fx = CannedEffects::new();
                        let r = crate::commands::audit::install_config(
                            &yaml_path.to_string_lossy(),
                            home,
                            &fx,
                        );
                        last_cmd_result = Some(cmd_result_from_anyhow(r, &fx));
                        last_result = None;
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
                        if let Some(cmd_res) = last_cmd_result.take() {
                            if let Err(msg) = &cmd_res {
                                panic!(
                                    "Scenario {:?} step {}: expected success but got error:\n{}",
                                    scenario.name, step_idx, msg
                                );
                            }
                        } else {
                            let result = last_result.take().unwrap_or_else(|| {
                                panic!(
                                    "Scenario {:?} step {}: ExpectSuccess with no preceding run",
                                    scenario.name, step_idx
                                )
                            });
                            if let Err(failures) = &result {
                                panic!(
                                    "Scenario {:?} step {}: expected success but got {} failure(s): {:?}",
                                    scenario.name,
                                    step_idx,
                                    failures.len(),
                                    failures
                                );
                            }
                        }
                    }
                    ScenarioStep::ExpectFailure { messages } => {
                        if let Some(cmd_res) = last_cmd_result.take() {
                            match cmd_res {
                                Ok(output) => {
                                    panic!(
                                        "Scenario {:?} step {}: expected failure but got success\noutput: {}",
                                        scenario.name, step_idx, output
                                    );
                                }
                                Err(combined) => {
                                    for msg in messages {
                                        assert!(
                                            combined.contains(msg.as_str()),
                                            "Scenario {:?} step {}: expected error to contain {:?}, got:\n{}",
                                            scenario.name,
                                            step_idx,
                                            msg,
                                            combined
                                        );
                                    }
                                }
                            }
                        } else {
                            let result = last_result.take().unwrap_or_else(|| {
                                panic!(
                                    "Scenario {:?} step {}: ExpectFailure with no preceding run",
                                    scenario.name, step_idx
                                )
                            });
                            match result {
                                Ok(()) => {
                                    panic!(
                                        "Scenario {:?} step {}: expected failure but got success",
                                        scenario.name, step_idx
                                    );
                                }
                                Err(failures) => {
                                    let all_output: String = failures
                                        .iter()
                                        .map(|f| f.to_string())
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    for msg in messages {
                                        assert!(
                                            all_output.contains(msg.as_str()),
                                            "Scenario {:?} step {}: expected failure containing {:?}, got:\n{}",
                                            scenario.name,
                                            step_idx,
                                            msg,
                                            all_output
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
