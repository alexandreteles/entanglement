use super::{MaintainabilityIndex, MaintainabilityRating, rating};

#[test]
fn calculates_microsoft_formula_from_volume_cyclomatic_and_nloc() {
    let metrics = MaintainabilityIndex::calculate(64.0, 8, 16);
    let expected =
        (171.0 - 5.2 * 64.0_f64.ln() - 0.23 * 8.0 - 16.2 * 16.0_f64.ln()) * 100.0 / 171.0;

    assert!((metrics.score - expected).abs() < 1e-12);
    assert_eq!(metrics.volume, 64.0);
    assert_eq!(metrics.cyclomatic_complexity, 8);
    assert_eq!(metrics.nloc, 16);
    assert_eq!(metrics.rating, rating(metrics.score));
}

#[test]
fn zero_inputs_are_finite_and_measure_an_empty_scope_as_good() {
    let metrics = MaintainabilityIndex::calculate(0.0, 0, 0);

    assert_eq!(metrics.score, 100.0);
    assert_eq!(metrics.rating, MaintainabilityRating::GreenGood);
    assert_eq!(metrics.volume, 0.0);
    assert!(metrics.score.is_finite());
}

#[test]
fn clamps_score_and_uses_unrounded_band_boundaries() {
    assert_eq!(MaintainabilityIndex::calculate(1.0, 10_000, 1).score, 0.0);
    assert_eq!(rating(9.999_999), MaintainabilityRating::RedLow);
    assert_eq!(rating(10.0), MaintainabilityRating::YellowModerate);
    assert_eq!(rating(19.999_999), MaintainabilityRating::YellowModerate);
    assert_eq!(rating(20.0), MaintainabilityRating::GreenGood);
    assert_eq!(rating(100.0), MaintainabilityRating::GreenGood);
}

#[test]
fn explains_score_delta_by_input_and_clamp() {
    let before = MaintainabilityIndex::calculate(32.0, 4, 12);
    let after = MaintainabilityIndex::calculate(64.0, 6, 18);
    let change = before.change_to(after);
    let explained = change.volume_effect
        + change.cyclomatic_effect
        + change.nloc_effect
        + change.clamp_adjustment;

    assert!((change.score_delta - (after.score - before.score)).abs() < 1e-12);
    assert!((change.score_delta - explained).abs() < 1e-12);
    assert!(change.score_delta < 0.0);

    let clamped_before = MaintainabilityIndex::calculate(1.0, 10_000, 1);
    let clamped_after = MaintainabilityIndex::calculate(1.0, 0, 1);
    let clamped_change = clamped_before.change_to(clamped_after);
    assert_ne!(clamped_change.clamp_adjustment, 0.0);
    assert!(
        (clamped_change.score_delta
            - clamped_change.volume_effect
            - clamped_change.cyclomatic_effect
            - clamped_change.nloc_effect
            - clamped_change.clamp_adjustment)
            .abs()
            < 1e-9
    );
}
