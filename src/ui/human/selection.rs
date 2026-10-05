use std::fmt::Write;

use crate::metrics::selection::{Metric, Selection};
use crate::model::{AnalysisResult, FileAnalysis, FunctionDelta};

use super::{
    metric_value, write_contributions, write_halstead, write_halstead_delta, write_maintainability,
    write_maintainability_bands, write_maintainability_delta, write_unsupported_injections,
};

#[derive(Clone, Copy)]
enum FunctionColumn {
    Cc,
    Cogc,
    Density,
}

const FILE_COLUMNS: [FunctionColumn; 3] = [
    FunctionColumn::Cc,
    FunctionColumn::Cogc,
    FunctionColumn::Density,
];

const PATCH_COLUMNS: [FunctionColumn; 2] = [FunctionColumn::Cc, FunctionColumn::Density];

pub(super) fn render(result: &AnalysisResult, selection: Selection) -> String {
    let mut output = String::new();
    if let Some(patch) = &result.patch {
        write_patch_report(&mut output, result, patch, selection);
        return output;
    }

    if !result.files.is_empty() && selection.includes(Metric::Mi) {
        write_maintainability_bands(&mut output, &result.maintainability_index_bands);
    }
    for file in &result.files {
        if !output.is_empty() {
            output.push('\n');
        }
        write_file(&mut output, file, selection);
    }
    output
}

fn write_patch_report(
    output: &mut String,
    result: &AnalysisResult,
    patch: &crate::model::PatchAnalysis,
    selection: Selection,
) {
    if selection.includes(Metric::Mi) {
        write_maintainability_bands(output, &result.maintainability_index_bands);
    }
    for file in &patch.files {
        if !output.is_empty() {
            output.push('\n');
        }
        write_patch_file(output, file, selection);
    }
}

fn write_patch_file(
    output: &mut String,
    file: &crate::model::FilePatchAnalysis,
    selection: Selection,
) {
    let _ = writeln!(output, "{}", file.path);
    if file.before.is_none() && file.after.is_none() {
        let _ = writeln!(output, "No registered grammar for this file.");
        return;
    }
    let _ = writeln!(
        output,
        "File NLOC: {} → {}",
        metric_value(file.before.as_ref().map(|before| before.nloc)),
        metric_value(file.after.as_ref().map(|after| after.nloc))
    );
    if selection.includes(Metric::Mi) {
        write_maintainability_delta(
            output,
            "File maintainability index",
            &file.maintainability_index,
        );
    }
    if selection.includes(Metric::Halstead) {
        write_halstead_delta(output, "File Halstead", &file.halstead);
    }
    for function in &file.functions {
        write_patch_function(output, function, selection);
    }
}

fn write_file(output: &mut String, file: &FileAnalysis, selection: Selection) {
    let name_width = file
        .functions
        .iter()
        .map(|function| function.name.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let _ = writeln!(output, "{}", file.path);
    let _ = write!(output, "{:<name_width$}  {:>5}", "Function", "NLOC");
    for column in selected_columns(selection, &FILE_COLUMNS) {
        write_column_header(output, column);
    }
    output.push('\n');

    for function in &file.functions {
        let _ = write!(
            output,
            "{:<name_width$}  {:>5}",
            function.name, function.nloc
        );
        for column in selected_columns(selection, &FILE_COLUMNS) {
            write_file_column(output, function, column);
        }
        output.push('\n');
    }
    let _ = writeln!(output, "File NLOC: {}", file.nloc);
    if selection.includes(Metric::Mi) {
        write_maintainability(
            output,
            "File maintainability index",
            file.maintainability_index.as_ref(),
        );
    }
    if selection.includes(Metric::Halstead) {
        write_halstead(output, "File Halstead", &file.halstead);
    }
    for function in &file.functions {
        if selection.includes(Metric::Mi) {
            write_maintainability(
                output,
                &format!("Maintainability index for {}", function.name),
                function.maintainability_index.as_ref(),
            );
        }
        if selection.includes(Metric::Halstead) {
            write_halstead(
                output,
                &format!("Halstead for {}", function.name),
                &function.halstead,
            );
        }
    }
    write_unsupported_injections(output, file);
}

fn write_patch_function(output: &mut String, function: &FunctionDelta, selection: Selection) {
    let _ = write!(
        output,
        "\n{}\n  NLOC    {} → {} ({:+})",
        function.name, function.nloc.before, function.nloc.after, function.nloc.delta
    );
    for column in selected_columns(selection, &PATCH_COLUMNS) {
        write_patch_column(output, function, column);
    }
    output.push('\n');
    if selection.includes(Metric::Cc) {
        write_contributions(
            output,
            "Complexity changes",
            &function.added_contributions,
            &function.removed_contributions,
        );
    }
    if selection.includes(Metric::Cogc) {
        let _ = writeln!(
            output,
            "  Cognitive complexity {} → {} ({:+})",
            function.cognitive_complexity.before,
            function.cognitive_complexity.after,
            function.cognitive_complexity.delta,
        );
        write_contributions(
            output,
            "Cognitive complexity changes",
            &function.added_cognitive_contributions,
            &function.removed_cognitive_contributions,
        );
    }
    if selection.includes(Metric::Mi) {
        write_maintainability_delta(
            output,
            "Maintainability index",
            &function.maintainability_index,
        );
    }
    if selection.includes(Metric::Halstead) {
        write_halstead_delta(output, "Halstead", &function.halstead);
    }
}

fn selected_columns(
    selection: Selection,
    columns: &[FunctionColumn],
) -> impl Iterator<Item = FunctionColumn> + '_ {
    columns
        .iter()
        .copied()
        .filter(move |column| selection.includes(column.metric()))
}

impl FunctionColumn {
    fn metric(self) -> Metric {
        match self {
            Self::Cc => Metric::Cc,
            Self::Cogc => Metric::Cogc,
            Self::Density => Metric::Density,
        }
    }
}

fn write_column_header(output: &mut String, column: FunctionColumn) {
    match column {
        FunctionColumn::Cc => {
            let _ = write!(output, "  {:>4}", "CC");
        }
        FunctionColumn::Cogc => {
            let _ = write!(output, "  {:>4}", "CogC");
        }
        FunctionColumn::Density => {
            let _ = write!(output, "  {:>11}", "CC density");
        }
    }
}

fn write_file_column(
    output: &mut String,
    function: &crate::model::FunctionAnalysis,
    column: FunctionColumn,
) {
    match column {
        FunctionColumn::Cc => {
            let _ = write!(output, "  {:>4}", function.cyclomatic_complexity);
        }
        FunctionColumn::Cogc => {
            let _ = write!(output, "  {:>4}", function.cognitive_complexity);
        }
        FunctionColumn::Density => {
            let _ = write!(output, "  {:>11.3}", function.cyclomatic_density);
        }
    }
}

fn write_patch_column(output: &mut String, function: &FunctionDelta, column: FunctionColumn) {
    match column {
        FunctionColumn::Cc => {
            let delta = &function.cyclomatic_complexity;
            let _ = write!(
                output,
                "\n  CC      {} → {} ({:+})",
                delta.before, delta.after, delta.delta
            );
        }
        FunctionColumn::Density => {
            let delta = &function.cyclomatic_density;
            let _ = write!(
                output,
                "\n  density {:.3} → {:.3} ({:+.3})",
                delta.before, delta.after, delta.delta
            );
        }
        FunctionColumn::Cogc => unreachable!("patch summary columns exclude cognitive scores"),
    }
}
