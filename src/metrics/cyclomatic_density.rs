/// Return complexity per nonzero code line.
///
/// Use one as the line count when `nloc` is zero. This avoids division by
/// zero and keeps empty-function density finite.
pub fn calculate(cyclomatic_complexity: usize, nloc: usize) -> f64 {
    cyclomatic_complexity as f64 / nloc.max(1) as f64
}
