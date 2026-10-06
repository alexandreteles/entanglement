use tree_sitter::Language;

use crate::Result;

use super::super::LanguageHandler;
use super::build_web;

pub(super) fn build(language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = format!(
        "{}\n{}",
        include_str!("../../queries/web.scm"),
        include_str!("../../queries/javascript.scm")
    );
    build_web(language, &query)
}
