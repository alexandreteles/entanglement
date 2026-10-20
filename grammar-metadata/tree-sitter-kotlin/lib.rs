//! Kotlin grammar with context clauses used by Entanglement.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_kotlin() -> *const ();
}

/// Return the corrected Kotlin grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_kotlin) };
/// Describe the corrected Kotlin syntax nodes.
pub const NODE_TYPES: &str = include_str!(concat!(env!("OUT_DIR"), "/grammar/src/node-types.json"));
