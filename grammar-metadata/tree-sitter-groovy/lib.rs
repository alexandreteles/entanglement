//! Groovy grammar with nested command arguments used by Entanglement.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_groovy() -> *const ();
}

/// Return the corrected Groovy grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_groovy) };
/// Describe the corrected Groovy syntax nodes.
pub const NODE_TYPES: &str = include_str!(concat!(env!("OUT_DIR"), "/grammar/src/node-types.json"));
