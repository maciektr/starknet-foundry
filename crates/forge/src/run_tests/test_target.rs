use anyhow::Result;
use forge_runner::filtering::{ExcludeReason, FilterResult, TestCaseFilter};
use forge_runner::messages::TestResultMessage;
use forge_runner::{
    forge_config::ForgeConfig,
    maybe_generate_coverage, maybe_save_trace_and_profile,
    package_tests::with_config_resolved::TestTargetWithResolvedConfig,
    run_for_test_case,
    test_case_summary::{AnyTestCaseSummary, TestCaseSummary},
    test_target_summary::TestTargetSummary,
};
use foundry_ui::UI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[non_exhaustive]
pub enum TestTargetRunResult {
    Ok(TestTargetSummary),
    Interrupted(TestTargetSummary),
}

#[tracing::instrument(skip_all, level = "debug")]
pub fn run_for_test_target(
    tests: TestTargetWithResolvedConfig,
    forge_config: Arc<ForgeConfig>,
    tests_filter: &impl TestCaseFilter,
    ui: Arc<UI>,
) -> Result<TestTargetRunResult> {
    let casm_program = tests.casm_program.clone();

    let cancel = Arc::new(AtomicBool::new(false));

    let (tx, rx) = std::sync::mpsc::channel();

    enum FilteredCase {
        ExcludedFromPartition,
        Ignored(String),
        Included(forge_runner::package_tests::with_config_resolved::TestCaseWithResolvedConfig),
    }

    let filtered_cases: Vec<_> = tests
        .test_cases
        .into_iter()
        .map(|case| {
            let filter_result = tests_filter.filter(&case);
            match filter_result {
                FilterResult::Excluded(reason) => match reason {
                    ExcludeReason::ExcludedFromPartition => FilteredCase::ExcludedFromPartition,
                    ExcludeReason::Ignored => FilteredCase::Ignored(case.name.clone()),
                },
                FilterResult::Included => FilteredCase::Included(case),
            }
        })
        .collect();

    rayon::scope(|s| {
        for case in filtered_cases {
            match case {
                FilteredCase::ExcludedFromPartition => {
                    let tx = tx.clone();
                    s.spawn(move |_| {
                        let _ = tx.send(Ok(AnyTestCaseSummary::Single(
                            TestCaseSummary::ExcludedFromPartition {},
                        )));
                    });
                }
                FilteredCase::Ignored(name) => {
                    let tx = tx.clone();
                    s.spawn(move |_| {
                        let _ = tx.send(Ok(AnyTestCaseSummary::Single(
                            TestCaseSummary::Ignored { name },
                        )));
                    });
                }
                FilteredCase::Included(case) => {
                    let tx = tx.clone();
                    let casm_program = casm_program.clone();
                    let forge_config = forge_config.clone();
                    let sierra_program_path = tests.sierra_program_path.clone();
                    let cancel = cancel.clone();
                    s.spawn(move |_| {
                        let result = run_for_test_case(
                            Arc::new(case),
                            casm_program,
                            forge_config,
                            sierra_program_path,
                            cancel,
                        );
                        let _ = tx.send(result);
                    });
                }
            }
        }
        drop(tx);
    });

    let mut results = vec![];
    let mut saved_trace_data_paths = vec![];
    let mut interrupted = false;
    let deterministic_output = forge_config.test_runner_config.deterministic_output;

    let print_test_result = |result: &AnyTestCaseSummary| {
        let test_result_message = TestResultMessage::new(
            result,
            forge_config.output_config.detailed_resources,
            forge_config.test_runner_config.tracked_resource,
        );
        ui.println(&test_result_message);
    };

    for result in rx {
        let result = result?;

        if !deterministic_output && should_print_test_result_message(&result) {
            print_test_result(&result);
        }

        let trace_path = maybe_save_trace_and_profile(
            &result,
            &forge_config.output_config.execution_data_to_save,
        )?;
        if let Some(path) = trace_path {
            saved_trace_data_paths.push(path);
        }

        if result.is_failed() && forge_config.test_runner_config.exit_first {
            interrupted = true;
            cancel.store(true, Ordering::Release);
        }

        results.push(result);
    }

    if deterministic_output {
        let mut sorted_results: Vec<_> = results
            .iter()
            .filter(|r| should_print_test_result_message(r))
            .collect();
        sorted_results.sort_by_key(|r| r.name().unwrap_or(""));
        for result in sorted_results {
            print_test_result(result);
        }
    }

    maybe_generate_coverage(
        &forge_config.output_config.execution_data_to_save,
        &saved_trace_data_paths,
        &ui,
    )?;

    let summary = TestTargetSummary {
        test_case_summaries: results,
    };

    if interrupted {
        Ok(TestTargetRunResult::Interrupted(summary))
    } else {
        Ok(TestTargetRunResult::Ok(summary))
    }
}

fn should_print_test_result_message(result: &AnyTestCaseSummary) -> bool {
    !result.is_interrupted() && !result.is_excluded_from_partition()
}
