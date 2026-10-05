use serde::Serialize;

const SCORE_SCALE: f64 = 100.0 / 171.0;

/// The Microsoft Maintainability Index rating for a clamped score.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaintainabilityRating {
    /// A score below 10.
    RedLow,
    /// A score from 10 up to, but not including, 20.
    YellowModerate,
    /// A score from 20 through 100.
    GreenGood,
}

/// One displayed score band, with its exact inclusive/exclusive boundaries.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct MaintainabilityBand {
    /// The rating assigned to scores in this band.
    pub rating: MaintainabilityRating,
    /// The inclusive lower score boundary.
    pub minimum_inclusive: f64,
    /// The upper score boundary.
    pub maximum: f64,
    /// Whether the upper score boundary is inclusive.
    pub maximum_inclusive: bool,
    /// The color associated with this band in reports.
    pub color: &'static str,
    /// The plain-language interpretation of the band.
    pub meaning: &'static str,
}

/// The Microsoft Maintainability Index rating bands shown by all reports.
pub const BANDS: [MaintainabilityBand; 3] = [
    MaintainabilityBand {
        rating: MaintainabilityRating::RedLow,
        minimum_inclusive: 0.0,
        maximum: 10.0,
        maximum_inclusive: false,
        color: "red",
        meaning: "low maintainability",
    },
    MaintainabilityBand {
        rating: MaintainabilityRating::YellowModerate,
        minimum_inclusive: 10.0,
        maximum: 20.0,
        maximum_inclusive: false,
        color: "yellow",
        meaning: "moderate maintainability",
    },
    MaintainabilityBand {
        rating: MaintainabilityRating::GreenGood,
        minimum_inclusive: 20.0,
        maximum: 100.0,
        maximum_inclusive: true,
        color: "green",
        meaning: "good maintainability",
    },
];

/// Microsoft Maintainability Index inputs, score, and color band.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct MaintainabilityIndex {
    /// The score clamped to the inclusive range from 0 to 100.
    pub score: f64,
    /// The score's band, classified before any display rounding.
    pub rating: MaintainabilityRating,
    /// The Halstead volume input.
    pub volume: f64,
    /// The cyclomatic complexity input.
    pub cyclomatic_complexity: usize,
    /// The source NLOC input.
    pub nloc: usize,
}

/// The point effects of changing each MI input before and after score clamping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaintainabilityChange {
    /// The clamped after score minus the clamped before score.
    pub score_delta: f64,
    /// The score effect from the change in Halstead volume.
    pub volume_effect: f64,
    /// The score effect from the change in cyclomatic complexity.
    pub cyclomatic_effect: f64,
    /// The score effect from the change in NLOC.
    pub nloc_effect: f64,
    /// The difference between summed input effects and the clamped score delta.
    pub clamp_adjustment: f64,
}

impl MaintainabilityIndex {
    /// Calculate Microsoft's maintainability score from volume, CC, and NLOC.
    ///
    /// The formula is `clamp((171 - 5.2 ln(V) - 0.23 CC - 16.2 ln(NLOC)) *
    /// 100 / 171, 0, 100)`. The logarithm inputs use `max(value, 1)` so empty
    /// code has a finite, measured score of 100. Classification uses the
    /// unrounded clamped score: below 10 is red, below 20 is yellow, and the
    /// rest is green. A nonfinite or negative volume is treated as zero.
    pub fn calculate(volume: f64, cyclomatic_complexity: usize, nloc: usize) -> Self {
        let volume = if volume.is_finite() {
            volume.max(0.0)
        } else {
            0.0
        };
        let raw_score = (171.0
            - 5.2 * volume.max(1.0).ln()
            - 0.23 * cyclomatic_complexity as f64
            - 16.2 * (nloc.max(1) as f64).ln())
            * SCORE_SCALE;
        let score = raw_score.clamp(0.0, 100.0);
        let rating = rating(score);

        Self {
            score,
            rating,
            volume,
            cyclomatic_complexity,
            nloc,
        }
    }

    /// Explain the score change by input, including any effect of clamping.
    pub fn change_to(self, after: Self) -> MaintainabilityChange {
        let score_delta = after.score - self.score;
        let volume_effect =
            -5.2 * (after.volume.max(1.0).ln() - self.volume.max(1.0).ln()) * SCORE_SCALE;
        let cyclomatic_effect = -0.23
            * (after.cyclomatic_complexity as f64 - self.cyclomatic_complexity as f64)
            * SCORE_SCALE;
        let nloc_effect = -16.2
            * ((after.nloc.max(1) as f64).ln() - (self.nloc.max(1) as f64).ln())
            * SCORE_SCALE;
        let clamp_adjustment = score_delta - volume_effect - cyclomatic_effect - nloc_effect;

        MaintainabilityChange {
            score_delta,
            volume_effect,
            cyclomatic_effect,
            nloc_effect,
            clamp_adjustment,
        }
    }
}

fn rating(score: f64) -> MaintainabilityRating {
    if score < 10.0 {
        MaintainabilityRating::RedLow
    } else if score < 20.0 {
        MaintainabilityRating::YellowModerate
    } else {
        MaintainabilityRating::GreenGood
    }
}

#[cfg(test)]
#[path = "../../tests/unit/maintainability.rs"]
mod tests;
