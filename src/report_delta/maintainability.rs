//! Compare Maintainability Index scores and their component point effects.

use crate::metrics::maintainability::MaintainabilityIndex;
use crate::model;

/// Preserve absent sides and explain matched score changes by input metric.
pub(crate) fn compare(
    before: Option<&MaintainabilityIndex>,
    after: Option<&MaintainabilityIndex>,
) -> model::MaintainabilityDelta {
    let changes = before
        .zip(after)
        .map(|(before, after)| (*before).change_to(*after));
    model::MaintainabilityDelta {
        score: changes.as_ref().map(|change| model::MetricDelta {
            before: before.expect("change has before index").score,
            after: after.expect("change has after index").score,
            delta: change.score_delta,
        }),
        volume_effect: changes.as_ref().map(|change| change.volume_effect),
        cyclomatic_effect: changes.as_ref().map(|change| change.cyclomatic_effect),
        nloc_effect: changes.as_ref().map(|change| change.nloc_effect),
        clamp_adjustment: changes.as_ref().map(|change| change.clamp_adjustment),
        before: before.cloned(),
        after: after.cloned(),
    }
}
