use super::HalsteadMetrics;

#[test]
fn calculates_standard_metrics_from_counts() {
    // n1=2, n2=3, N1=4, N2=5 is the formula fixture.
    let metrics = HalsteadMetrics::calculate(2, 3, 4, 5);
    assert_eq!(metrics.vocabulary, 5);
    assert_eq!(metrics.length, 9);
    assert!((metrics.estimated_length - (2.0 + 3.0 * 3.0_f64.log2())).abs() < 1e-12);
    assert!((metrics.volume - 9.0 * 5.0_f64.log2()).abs() < 1e-12);
    assert!((metrics.difficulty - (2.0 / 2.0) * (5.0 / 3.0)).abs() < 1e-12);
    assert!((metrics.effort - metrics.difficulty * metrics.volume).abs() < 1e-12);
    assert!((metrics.time - metrics.effort / 18.0).abs() < 1e-12);
    assert!((metrics.program_level - 1.0 / metrics.difficulty).abs() < 1e-12);
    assert!((metrics.estimated_bugs - metrics.volume / 3000.0).abs() < 1e-12);
}

#[test]
fn zero_counts_produce_finite_values() {
    let empty = HalsteadMetrics::calculate(0, 0, 0, 0);
    assert_eq!(empty.vocabulary, 0);
    assert_eq!(empty.length, 0);
    assert_eq!(empty.estimated_length, 0.0);
    assert_eq!(empty.volume, 0.0);
    assert_eq!(empty.difficulty, 0.0);
    assert_eq!(empty.effort, 0.0);
    assert_eq!(empty.time, 0.0);
    assert_eq!(empty.program_level, 0.0);
    assert_eq!(empty.estimated_bugs, 0.0);
    assert!(
        [
            empty.estimated_length,
            empty.volume,
            empty.difficulty,
            empty.effort,
            empty.time,
            empty.program_level,
            empty.estimated_bugs,
        ]
        .into_iter()
        .all(f64::is_finite)
    );
}
