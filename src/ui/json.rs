//! Render analysis results as JSON.

use crate::metrics::selection::Selection;
use crate::model::AnalysisResult;

mod selection;

/// Serialize an analysis result as pretty JSON.
///
/// The output contains the same file and patch data as the analysis result.
/// It returns a serialization error if the result cannot be encoded.
pub fn render(result: &AnalysisResult, selection: Selection) -> crate::Result<String> {
    if super::selection::is_all(selection) {
        return Ok(serde_json::to_string_pretty(result)?);
    }
    let mut value = serde_json::to_value(result)?;
    selection::project(&mut value, selection);
    Ok(serde_json::to_string_pretty(&value)?)
}
