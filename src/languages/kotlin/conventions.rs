//! Statically represented Kotlin convention calls.
//!
//! Convention facts are emitted only when the grammar proves a Kotlin
//! operator, infix, or invoke expression.

mod capture;
mod convention_syntax;
mod convention_typing;
mod emission;
mod handlers;

pub(super) use capture::capture;
