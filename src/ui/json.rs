//! Render analysis results as JSON.

use crate::model::AnalysisResult;

/// Serialize an analysis result as pretty JSON.
///
/// The output contains the same file and patch data as the analysis result.
/// It returns a serialization error if the result cannot be encoded.
pub fn render(result: &AnalysisResult) -> crate::Result<String> {
    Ok(serde_json::to_string_pretty(result)?)
}
