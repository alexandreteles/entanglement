//! PHP grammars with the PHP 8.5 syntax corrections used by Entanglement.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_php() -> *const ();
    fn tree_sitter_php_only() -> *const ();
}

/// Return the grammar for PHP files that can contain text outside PHP tags.
pub const LANGUAGE_PHP: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_php) };
/// Return the grammar for PHP source without an opening tag.
pub const LANGUAGE_PHP_ONLY: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_php_only) };
/// Describe the corrected PHP syntax nodes.
pub const PHP_NODE_TYPES: &str =
    include_str!(concat!(env!("OUT_DIR"), "/grammar/php/src/node-types.json"));
/// Describe the corrected PHP-only syntax nodes.
pub const PHP_ONLY_NODE_TYPES: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/grammar/php_only/src/node-types.json"
));
