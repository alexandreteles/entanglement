//! Render analysis results as JSON.

use crate::metrics::selection::{Metric, Selection};
use crate::model::AnalysisResult;

mod selection;

/// Serialize an analysis result as pretty JSON.
///
/// The output contains the same file and patch data as the analysis result.
/// It returns a serialization error if the result cannot be encoded.
pub fn render(result: &AnalysisResult, selection: Selection) -> crate::Result<String> {
    if selection.includes(Metric::All) {
        return Ok(serde_json::to_string_pretty(result)?);
    }
    let mut value = serde_json::to_value(result)?;
    selection::project(&mut value, selection);
    Ok(serde_json::to_string_pretty(&value)?)
}
