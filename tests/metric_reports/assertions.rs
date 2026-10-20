use serde_json::Value;

pub(super) const INDICATORS: &[&str] = &[
    "distinct_operators",
    "distinct_operands",
    "total_operators",
    "total_operands",
    "vocabulary",
    "length",
    "estimated_length",
    "volume",
    "difficulty",
    "effort",
    "time",
    "program_level",
    "estimated_bugs",
];
pub(super) const EFFECTS: &[&str] = &[
    "volume_effect",
    "cyclomatic_effect",
    "nloc_effect",
    "clamp_adjustment",
];

pub(super) fn assert_halstead(metrics: &Value) {
    for name in INDICATORS {
        assert!(
            metrics[name].as_f64().is_some_and(f64::is_finite),
            "invalid {name}"
        );
    }
}

pub(super) fn assert_halstead_delta(delta: &Value) {
    for name in INDICATORS {
        assert!(
            ["before", "after", "delta"]
                .iter()
                .all(|side| delta[name][side].is_number()),
            "invalid {name} delta"
        );
    }
    for side in ["added_tokens", "removed_tokens"] {
        assert!(delta[side].is_array());
    }
}

fn expected_score(volume: f64, cc: usize, nloc: usize) -> f64 {
    let score =
        (171.0 - 5.2 * volume.max(1.0).ln() - 0.23 * cc as f64 - 16.2 * (nloc.max(1) as f64).ln())
            * 100.0
            / 171.0;
    score.clamp(0.0, 100.0)
}

pub(super) fn assert_index(index: &Value) {
    for key in ["score", "rating", "volume", "cyclomatic_complexity", "nloc"] {
        assert!(
            index.get(key).is_some(),
            "missing Maintainability Index field {key}"
        );
    }
    assert!(index.get("cognitive_complexity").is_none());
    let expected = expected_score(
        index["volume"].as_f64().unwrap(),
        index["cyclomatic_complexity"].as_u64().unwrap() as usize,
        index["nloc"].as_u64().unwrap() as usize,
    );
    assert!((index["score"].as_f64().unwrap() - expected).abs() < 1e-10);
    let score = index["score"].as_f64().unwrap();
    let rating = if score < 10.0 {
        "red_low"
    } else if score < 20.0 {
        "yellow_moderate"
    } else {
        "green_good"
    };
    assert_eq!(index["rating"], rating);
}

pub(super) fn assert_index_delta(delta: &Value) {
    for key in ["before", "after", "score"] {
        assert!(delta.get(key).is_some(), "missing MI delta field {key}");
    }
    let paired = delta["before"].is_object() && delta["after"].is_object();
    assert_eq!(delta["score"].is_object(), paired);
    assert_eq!(delta["score"].is_null(), !paired);
    for key in EFFECTS {
        assert_eq!(delta[key].is_number(), paired, "numeric effect {key}");
        assert_eq!(delta[key].is_null(), !paired, "absent effect {key}");
    }
}
