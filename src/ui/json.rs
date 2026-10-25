//! Render analysis results as JSON.

use crate::metrics::selection::{Metric, Selection};
use crate::model::AnalysisResult;
use serde::Serialize;

mod selection;

/// Serialize an analysis result as pretty JSON.
///
/// The output contains the same file and patch data as the analysis result.
/// It returns a serialization error if the result cannot be encoded.
pub fn render(result: &AnalysisResult, selection: Selection) -> crate::Result<String> {
    if selection.includes(Metric::All) {
        return Ok(serde_json::to_string_pretty(result)?);
    }
    let mut value = serde_json::to_value(MetricReport {
        files: &result.files,
        maintainability_index_bands: &result.maintainability_index_bands,
        patch: result.patch.as_ref(),
    })?;
    selection::project(&mut value, selection);
    Ok(serde_json::to_string_pretty(&SelectedReport {
        metrics: value,
        structure: result.structure.as_ref(),
    })?)
}

#[derive(Serialize)]
struct MetricReport<'a> {
    files: &'a [crate::model::FileAnalysis],
    maintainability_index_bands: &'a [crate::metrics::maintainability::MaintainabilityBand; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    patch: Option<&'a crate::model::PatchAnalysis>,
}

#[derive(Serialize)]
struct SelectedReport<'a> {
    #[serde(flatten)]
    metrics: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    structure: Option<&'a crate::structure::StructuralReport>,
}
