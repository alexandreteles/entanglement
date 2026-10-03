//! Render analysis results as readable terminal text.

use std::fmt::Write;

use crate::model::{AnalysisResult, ComplexityContribution, FileAnalysis};

/// Render file metrics or patch changes as readable terminal text.
///
/// The renderer uses values already present in `result`. It does not calculate
/// metrics or change contribution signs. It returns an empty string when the
/// result has no files and no patch data.
pub fn render(result: &AnalysisResult) -> String {
    let mut output = String::new();
    if let Some(patch) = &result.patch {
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
                    &function.added_contributions,
                    &function.removed_contributions,
                );
            }
        }
        return output;
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
        "{:<name_width$}  {:>5}  {:>4}  {:>11}",
        "Function", "NLOC", "CC", "CC density"
    );
    for function in &file.functions {
        let _ = writeln!(
            output,
            "{:<name_width$}  {:>5}  {:>4}  {:>11.3}",
            function.name,
            function.nloc,
            function.cyclomatic_complexity,
            function.cyclomatic_density,
        );
    }
    let _ = writeln!(output, "File NLOC: {}", file.nloc);
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

fn write_contributions(
    output: &mut String,
    added: &[ComplexityContribution],
    removed: &[ComplexityContribution],
) {
    if added.is_empty() && removed.is_empty() {
        return;
    }
    let _ = writeln!(output, "  Complexity changes");
    for contribution in added.iter().chain(removed) {
        let _ = writeln!(
            output,
            "    {:+} {:<20} line {}",
            contribution.value, contribution.kind, contribution.line
        );
    }
}
