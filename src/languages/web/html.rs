use tree_sitter::Language;

use crate::Result;

use super::super::LanguageHandler;
use super::super::query::handler::syntax_only;

pub(super) fn build(language: &Language) -> Result<Box<dyn LanguageHandler>> {
    syntax_only(language, include_str!("../../queries/html.scm"))
}
