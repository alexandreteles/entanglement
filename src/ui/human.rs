//! Render analysis results as readable terminal text.

use std::fmt::Write;

use super::maintainability::{
    write_bands as write_maintainability_bands, write_delta as write_maintainability_delta,
    write_index as write_maintainability,
};
use crate::metrics::halstead::HalsteadMetrics;
use crate::model::{
    AnalysisResult, ComplexityContribution, FileAnalysis, HalsteadDelta, HalsteadTokenChange,
    MetricDelta,
};

/// Render file metrics or patch changes as readable terminal text.
///
/// The renderer uses values already present in `result`. It does not calculate
/// metrics or change contribution signs. It returns an empty string when the
/// result has no files and no patch data.
pub fn render(result: &AnalysisResult) -> String {
    let mut output = String::new();
    if let Some(patch) = &result.patch {
        write_maintainability_bands(&mut output, &result.maintainability_index_bands);
        for file in &patch.files {
            if !output.is_empty() {
                output.push('\n');
            }
            let _ = writeln!(output, "{}", file.path);
            if file.before.is_none() && file.after.is_none() {
                let _ = writeln!(output, "No registered grammar for this file.");
                continue;
            }
            let before_nloc = file.before.as_ref().map(|before| before.nloc);
            let after_nloc = file.after.as_ref().map(|after| after.nloc);
            let _ = writeln!(
                output,
                "File NLOC: {} → {}",
                metric_value(before_nloc),
                metric_value(after_nloc)
            );
            write_maintainability_delta(
                &mut output,
                "File maintainability index",
                &file.maintainability_index,
            );
            write_halstead_delta(&mut output, "File Halstead", &file.halstead);
            for function in &file.functions {
                let _ = writeln!(
                    output,
                    "\n{}\n  NLOC    {} → {} ({:+})\n  CC      {} → {} ({:+})\n  density {:.3} → {:.3} ({:+.3})",
                    function.name,
                    function.nloc.before,
                    function.nloc.after,
                    function.nloc.delta,
                    function.cyclomatic_complexity.before,
                    function.cyclomatic_complexity.after,
                    function.cyclomatic_complexity.delta,
                    function.cyclomatic_density.before,
                    function.cyclomatic_density.after,
                    function.cyclomatic_density.delta,
                );
                write_contributions(
                    &mut output,
                    "Complexity changes",
                    &function.added_contributions,
                    &function.removed_contributions,
                );
                let _ = writeln!(
                    output,
                    "  Cognitive complexity {} → {} ({:+})",
                    function.cognitive_complexity.before,
                    function.cognitive_complexity.after,
                    function.cognitive_complexity.delta,
                );
                write_contributions(
                    &mut output,
                    "Cognitive complexity changes",
                    &function.added_cognitive_contributions,
                    &function.removed_cognitive_contributions,
                );
                write_maintainability_delta(
                    &mut output,
                    "Maintainability index",
                    &function.maintainability_index,
                );
                write_halstead_delta(&mut output, "Halstead", &function.halstead);
            }
        }
        return output;
    }

    if !result.files.is_empty() {
        write_maintainability_bands(&mut output, &result.maintainability_index_bands);
    }
    for file in &result.files {
        if !output.is_empty() {
            output.push('\n');
        }
        render_file(&mut output, file);
    }
    output
}

fn metric_value(value: Option<usize>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| value.to_string())
}

fn render_file(output: &mut String, file: &FileAnalysis) {
    let name_width = file
        .functions
        .iter()
        .map(|function| function.name.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let _ = writeln!(output, "{}", file.path);
    let _ = writeln!(
        output,
        "{:<name_width$}  {:>5}  {:>4}  {:>4}  {:>11}",
        "Function", "NLOC", "CC", "CogC", "CC density"
    );
    for function in &file.functions {
        let _ = writeln!(
            output,
            "{:<name_width$}  {:>5}  {:>4}  {:>4}  {:>11.3}",
            function.name,
            function.nloc,
            function.cyclomatic_complexity,
            function.cognitive_complexity,
            function.cyclomatic_density,
        );
    }
    let _ = writeln!(output, "File NLOC: {}", file.nloc);
    write_maintainability(
        output,
        "File maintainability index",
        file.maintainability_index.as_ref(),
    );
    write_halstead(output, "File Halstead", &file.halstead);
    for function in &file.functions {
        write_maintainability(
            output,
            &format!("Maintainability index for {}", function.name),
            function.maintainability_index.as_ref(),
        );
        write_halstead(
            output,
            &format!("Halstead for {}", function.name),
            &function.halstead,
        );
    }
    let unsupported: Vec<_> = file
        .injections
        .iter()
        .filter(|injection| !injection.analyzed)
        .collect();
    if !unsupported.is_empty() {
        let _ = writeln!(output, "Injections not analyzed:");
        for injection in unsupported {
            let _ = writeln!(
                output,
                "  {} bytes {}..{}",
                injection.language, injection.start_byte, injection.end_byte
            );
        }
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
