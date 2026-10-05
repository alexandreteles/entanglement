//! Render analysis results as readable terminal text.

use std::fmt::Write;

use super::maintainability::{
    write_bands as write_maintainability_bands, write_delta as write_maintainability_delta,
    write_index as write_maintainability,
};
use crate::metrics::halstead::HalsteadMetrics;
use crate::metrics::selection::Selection;
use crate::model::{
    AnalysisResult, ComplexityContribution, FileAnalysis, HalsteadDelta, HalsteadTokenChange,
    MetricDelta,
};

mod selection;

/// Render file metrics or patch changes as readable terminal text.
///
/// The renderer uses values already present in `result`. It does not calculate
/// metrics or change contribution signs. It returns an empty string when the
/// result has no files and no patch data.
pub fn render(result: &AnalysisResult, selection: Selection) -> String {
    selection::render(result, selection)
}

fn metric_value(value: Option<usize>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| value.to_string())
}

pub(super) fn write_unsupported_injections(output: &mut String, file: &FileAnalysis) {
    let unsupported: Vec<_> = file
        .injections
        .iter()
        .filter(|injection| !injection.analyzed)
        .collect();
    if unsupported.is_empty() {
        return;
    }
    let _ = writeln!(output, "Injections not analyzed:");
    for injection in unsupported {
        let _ = writeln!(
            output,
            "  {} bytes {}..{}",
            injection.language, injection.start_byte, injection.end_byte
        );
    }
}

fn write_halstead(output: &mut String, label: &str, metrics: &HalsteadMetrics) {
    let _ = writeln!(output, "{label}:");
    let _ = writeln!(
        output,
        "  n1 {}  n2 {}  N1 {}  N2 {}",
        metrics.distinct_operators,
        metrics.distinct_operands,
        metrics.total_operators,
        metrics.total_operands,
    );
    let _ = writeln!(
        output,
        "  vocabulary {}  length {}  estimated length {:.3}",
        metrics.vocabulary, metrics.length, metrics.estimated_length
    );
    let _ = writeln!(
        output,
        "  volume {:.3}  difficulty {:.3}  effort {:.3}",
        metrics.volume, metrics.difficulty, metrics.effort
    );
    let _ = writeln!(
        output,
        "  time {:.3} seconds  program level {:.3}  estimated bugs {:.3}",
        metrics.time, metrics.program_level, metrics.estimated_bugs
    );
}

fn write_halstead_delta(output: &mut String, label: &str, delta: &HalsteadDelta) {
    let _ = writeln!(output, "  {label} changes:");
    write_delta(output, "n1", &delta.distinct_operators);
    write_delta(output, "n2", &delta.distinct_operands);
    write_delta(output, "N1", &delta.total_operators);
    write_delta(output, "N2", &delta.total_operands);
    write_delta(output, "vocabulary", &delta.vocabulary);
    write_delta(output, "length", &delta.length);
    write_float_delta(output, "estimated length", &delta.estimated_length);
    write_float_delta(output, "volume", &delta.volume);
    write_float_delta(output, "difficulty", &delta.difficulty);
    write_float_delta(output, "effort", &delta.effort);
    write_float_delta(output, "time (seconds)", &delta.time);
    write_float_delta(output, "program level", &delta.program_level);
    write_float_delta(output, "estimated bugs", &delta.estimated_bugs);
    write_token_changes(output, "added", &delta.added_tokens);
    write_token_changes(output, "removed", &delta.removed_tokens);
}

fn write_delta<T: std::fmt::Display, D: std::fmt::Display>(
    output: &mut String,
    label: &str,
    delta: &MetricDelta<T, D>,
) {
    let _ = writeln!(
        output,
        "    {label}: {} → {} ({:+})",
        delta.before, delta.after, delta.delta
    );
}

fn write_float_delta(output: &mut String, label: &str, delta: &MetricDelta<f64, f64>) {
    let _ = writeln!(
        output,
        "    {label}: {:.3} → {:.3} ({:+.3})",
        delta.before, delta.after, delta.delta
    );
}

fn write_token_changes(output: &mut String, label: &str, tokens: &[HalsteadTokenChange]) {
    if tokens.is_empty() {
        return;
    }
    let _ = writeln!(output, "    {label} token changes (volume is nonlinear):");
    for token in tokens {
        let sign = if label == "added" { "+" } else { "-" };
        let _ = writeln!(
            output,
            "      {sign}{} {:?} {:?} line {}",
            token.count, token.kind, token.token, token.line
        );
    }
}

fn write_contributions(
    output: &mut String,
    title: &str,
    added: &[ComplexityContribution],
    removed: &[ComplexityContribution],
) {
    if added.is_empty() && removed.is_empty() {
        return;
    }
    let _ = writeln!(output, "  {title}");
    for contribution in added.iter().chain(removed) {
        let _ = writeln!(
            output,
            "    {:+} {:<20} line {}",
            contribution.value, contribution.kind, contribution.line
        );
    }
}
