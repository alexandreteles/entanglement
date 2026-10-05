use clap::ValueEnum;

/// A metric that can be selected for a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ValueEnum)]
pub enum Metric {
    /// Non-comment source lines.
    #[value(name = "nloc")]
    Nloc,
    /// Cyclomatic complexity.
    #[value(name = "cc", alias = "cyclomatic", alias = "cyclomatic-complexity")]
    Cc,
    /// Cyclomatic complexity divided by NLOC.
    #[value(name = "density", alias = "cyclomatic-density")]
    Density,
    /// SonarSource cognitive complexity.
    #[value(name = "cogc", alias = "cognitive", alias = "cognitive-complexity")]
    Cogc,
    /// Halstead metrics.
    #[value(name = "halstead")]
    Halstead,
    /// Microsoft Maintainability Index.
    #[value(
        name = "mi",
        alias = "maintainability",
        alias = "maintainability-index"
    )]
    Mi,
    /// Every available metric.
    #[value(name = "all")]
    All,
}

/// Select the report metrics and their required internal calculations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    cc: bool,
    density: bool,
    cogc: bool,
    halstead: bool,
    mi: bool,
}

impl Selection {
    /// Build a selection from command-line metric names.
    pub fn new(metrics: &[Metric]) -> Self {
        if metrics.contains(&Metric::All) {
            return Self::all();
        }
        Self {
            cc: metrics.contains(&Metric::Cc),
            density: metrics.contains(&Metric::Density),
            cogc: metrics.contains(&Metric::Cogc),
            halstead: metrics.contains(&Metric::Halstead),
            mi: metrics.contains(&Metric::Mi),
        }
    }

    /// Select every available metric.
    pub const fn all() -> Self {
        Self {
            cc: true,
            density: true,
            cogc: true,
            halstead: true,
            mi: true,
        }
    }

    /// Return whether a metric is requested for reporting.
    pub const fn includes(self, metric: Metric) -> bool {
        match metric {
            Metric::Nloc => true,
            Metric::Cc => self.cc,
            Metric::Density => self.density,
            Metric::Cogc => self.cogc,
            Metric::Halstead => self.halstead,
            Metric::Mi => self.mi,
            Metric::All => self.cc && self.density && self.cogc && self.halstead && self.mi,
        }
    }

    /// Return whether cyclomatic complexity is needed for the selected report.
    pub const fn needs_cc(self) -> bool {
        self.cc || self.density || self.mi
    }

    /// Return whether Halstead volume is needed for the selected report.
    pub const fn needs_halstead(self) -> bool {
        self.halstead || self.mi
    }

    /// Return whether cognitive complexity is selected.
    pub const fn needs_cognitive(self) -> bool {
        self.cogc
    }

    /// Return whether complexity density is selected.
    pub const fn needs_density(self) -> bool {
        self.density
    }

    /// Return whether Maintainability Index is selected.
    pub const fn needs_mi(self) -> bool {
        self.mi
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::all()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/selection.rs"]
mod tests;
