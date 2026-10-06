use tree_sitter::{Language, Tree};

use crate::Result;
use crate::languages::{CapturedTree, LanguageHandler};

use super::{QueryAnalyzer, QueryFacts};

pub(crate) fn syntax_only(
    language: &Language,
    query_source: &str,
) -> Result<Box<dyn LanguageHandler>> {
    Ok(Box::new(SyntaxOnly(QueryAnalyzer::new(
        language,
        query_source,
    )?)))
}

struct SyntaxOnly(QueryAnalyzer);

impl LanguageHandler for SyntaxOnly {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        Ok(self.0.capture(tree, source, include_tokens).into())
    }
}

impl From<QueryFacts> for CapturedTree {
    fn from(query: QueryFacts) -> Self {
        Self {
            events: query.events,
            parents: query.parents,
            functions: query.functions,
            tokens: query.tokens,
            injections: query.injections,
            ..Self::default()
        }
    }
}
