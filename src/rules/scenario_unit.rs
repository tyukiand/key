/// Unit test interpreter: executes scenarios using internal Rust APIs.
#[cfg(test)]
mod tests {
    use crate::rules::evaluate::evaluate;
    use crate::rules::parse::parse_proposition_from_str;
    use crate::rules::scenario::{all_scenarios, ScenarioStep};

    #[test]
    fn run_all_scenarios_unit() {
        for scenario in all_scenarios() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path();
            let mut last_result: Option<Result<(), Vec<crate::rules::ast::RuleFailure>>> = None;

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
                    ScenarioStep::RunRulesCheck { yaml_content } => {
                        let prop = parse_proposition_from_str(yaml_content).unwrap_or_else(|e| {
                            panic!(
                                "Scenario {:?} step {}: invalid YAML: {}",
                                scenario.name, step_idx, e
                            )
                        });
                        last_result = Some(evaluate(&prop, home));
                    }
                    ScenarioStep::RunRulesTest {
                        yaml_content,
                        expect_failure_messages,
                        expect_num_failures,
                    } => {
                        let prop = parse_proposition_from_str(yaml_content).unwrap_or_else(|e| {
                            panic!(
                                "Scenario {:?} step {}: invalid YAML: {}",
                                scenario.name, step_idx, e
                            )
                        });
                        let result = evaluate(&prop, home);
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
                        // For RunRulesTest, the step itself is the assertion,
                        // so set last_result to Ok to indicate overall success of the test step
                        last_result = Some(Ok(()));
                    }
                    ScenarioStep::ExpectSuccess => {
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
                    ScenarioStep::ExpectFailure { messages } => {
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
