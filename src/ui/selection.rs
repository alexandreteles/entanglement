use crate::metrics::selection::{Metric, Selection};

pub(super) fn is_all(selection: Selection) -> bool {
    selection.includes(Metric::All)
}
