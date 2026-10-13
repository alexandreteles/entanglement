mod collector;
mod emission;
mod targets;

pub(super) use collector::collect_references;
pub(super) use targets::{is_argument_label, reference_target, simple_path};
