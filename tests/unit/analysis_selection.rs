use std::path::Path;

use super::Worker;
use crate::metrics::selection::{Metric, Selection};
use crate::model::FileAnalysis;

fn analyze(selection: Selection) -> FileAnalysis {
    let path = Path::new("selection-analysis.rs");
    let mut worker = Worker::new(selection).unwrap();
    let choice = worker.select_file(path).unwrap().unwrap();
    worker
        .analyze_selected_source(
            path,
            b"fn evaluate(flag: bool) { if flag && true { call(\"x\"); } }".to_vec(),
            None,
            &[],
            choice,
        )
        .unwrap()
        .facts
        .analysis
}

fn assert_unselected_placeholders(report: &FileAnalysis) {
    let function = &report.functions[0];
    for halstead in [&report.halstead, &function.halstead] {
        assert_eq!(halstead.length, 0);
        assert!(halstead.operators.is_empty());
        assert!(halstead.operands.is_empty());
    }
    assert_eq!(function.cyclomatic_complexity, 0);
    assert_eq!(function.cyclomatic_density, 0.0);
    assert!(function.contributions.is_empty());
    assert_eq!(function.cognitive_complexity, 0);
    assert!(function.cognitive_contributions.is_empty());
}

#[test]
fn nloc_only_skips_all_other_metric_calculations() {
    let report = analyze(Selection::new(&[Metric::Nloc]));
    assert!(report.nloc > 0);
    assert_eq!(report.functions.len(), 1);
    assert!(report.functions[0].nloc > 0);
    assert_unselected_placeholders(&report);
    assert!(report.maintainability_index.is_none());
    assert!(report.functions[0].maintainability_index.is_none());
}

#[test]
fn mi_computes_hidden_halstead_and_cyclomatic_inputs() {
    let report = analyze(Selection::new(&[Metric::Mi]));
    let function_mi = report.functions[0].maintainability_index.unwrap();
    let file_mi = report.maintainability_index.unwrap();

    assert!(function_mi.volume > 0.0);
    assert!(function_mi.cyclomatic_complexity > 1);
    assert!(function_mi.nloc > 0);
    assert!(file_mi.volume > 0.0);
    assert!(file_mi.cyclomatic_complexity > 1);
    assert_unselected_placeholders(&report);
}

#[test]
fn density_uses_hidden_cc_without_reporting_cc_or_contributors() {
    let report = analyze(Selection::new(&[Metric::Density]));
    let function = &report.functions[0];

    assert!(function.cyclomatic_density > 0.0);
    assert_eq!(function.cyclomatic_complexity, 0);
    assert!(function.contributions.is_empty());
    assert_eq!(function.cognitive_complexity, 0);
    assert!(function.maintainability_index.is_none());
    assert!(report.maintainability_index.is_none());
    assert_eq!(report.halstead.length, 0);
    assert_eq!(function.halstead.length, 0);
}
