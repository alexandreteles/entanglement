//! Render Microsoft Maintainability Index values and rating bands.

use std::fmt::Write;

use crate::metrics::maintainability::{
    MaintainabilityBand, MaintainabilityIndex, MaintainabilityRating,
};
use crate::model::MaintainabilityDelta;

/// Render all MI boundaries once for a human report.
pub(super) fn write_bands(output: &mut String, bands: &[MaintainabilityBand; 3]) {
    let _ = write!(output, "Maintainability Index bands: ");
    for (index, band) in bands.iter().enumerate() {
        if index > 0 {
            let _ = write!(output, "; ");
        }
        let upper = if band.maximum_inclusive {
            format!("{}", band.maximum as u32)
        } else {
            format!("<{}", band.maximum as u32)
        };
        let _ = write!(
            output,
            "{}–{} {} ({})",
            band.minimum_inclusive as u32, upper, band.color, band.meaning
        );
    }
    let _ = writeln!(
        output,
        ". Higher is better; a negative score change is a regression."
    );
}

/// Render one file or function score and the inputs used to calculate it.
pub(super) fn write_index(output: &mut String, label: &str, index: Option<&MaintainabilityIndex>) {
    if let Some(index) = index {
        let _ = writeln!(
            output,
            "{label}: {:.2} ({}) — volume {:.3}, CC {}, NLOC {}",
            display_score(index.score),
            rating_label(index.rating),
            index.volume,
            index.cyclomatic_complexity,
            index.nloc,
        );
    } else {
        let _ = writeln!(output, "{label}: not available");
    }
}

/// Render score, band, inputs, and the point effect from each input change.
pub(super) fn write_delta(output: &mut String, label: &str, delta: &MaintainabilityDelta) {
    let _ = writeln!(output, "  {label}:");
    match (delta.before, delta.after) {
        (Some(before), Some(after)) => write_matched_delta(output, before, after, delta),
        (Some(before), None) => {
            let _ = writeln!(
                output,
                "    {:.2} ({}) → —; scope deleted, score change unavailable",
                display_score(before.score),
                rating_label(before.rating),
            );
            write_inputs(output, "before inputs", before);
        }
        (None, Some(after)) => {
            let _ = writeln!(
                output,
                "    — → {:.2} ({}); new scope, score change unavailable",
                display_score(after.score),
                rating_label(after.rating),
            );
            write_inputs(output, "after inputs", after);
        }
        (None, None) => {
            let _ = writeln!(output, "    score change unavailable");
        }
    }
}

fn write_matched_delta(
    output: &mut String,
    before: MaintainabilityIndex,
    after: MaintainabilityIndex,
    delta: &MaintainabilityDelta,
) {
    let score_change = delta.score.as_ref().expect("matched scores have a delta");
    let _ = writeln!(
        output,
        "    {:.2} ({}) → {:.2} ({}) ({:+.3})",
        display_score(before.score),
        rating_label(before.rating),
        display_score(after.score),
        rating_label(after.rating),
        score_change.delta,
    );
    let _ = writeln!(
        output,
        "    inputs: volume {:.3} → {:.3}; CC {} → {}; NLOC {} → {}",
        before.volume,
        after.volume,
        before.cyclomatic_complexity,
        after.cyclomatic_complexity,
        before.nloc,
        after.nloc,
    );
    let _ = writeln!(
        output,
        "    score point effects: volume {:+.2}, CC {:+.2}, NLOC {:+.2}, clamp {:+.2}",
        delta.volume_effect.expect("matched scores have effects"),
        delta
            .cyclomatic_effect
            .expect("matched scores have effects"),
        delta.nloc_effect.expect("matched scores have effects"),
        delta
            .clamp_adjustment
            .expect("matched scores have clamp adjustment"),
    );
}

fn write_inputs(output: &mut String, label: &str, index: MaintainabilityIndex) {
    let _ = writeln!(
        output,
        "    {label}: volume {:.3}, CC {}, NLOC {}",
        index.volume, index.cyclomatic_complexity, index.nloc
    );
}

fn display_score(score: f64) -> f64 {
    (score * 100.0).trunc() / 100.0
}

fn rating_label(rating: MaintainabilityRating) -> &'static str {
    match rating {
        MaintainabilityRating::RedLow => "red / low",
        MaintainabilityRating::YellowModerate => "yellow / moderate",
        MaintainabilityRating::GreenGood => "green / good",
    }
}
