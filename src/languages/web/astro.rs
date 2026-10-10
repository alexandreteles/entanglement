use tree_sitter::{Language, Tree};

use crate::Result;
use crate::languages::{CapturedTree, LanguageHandler};
use crate::metrics::cyclomatic::FunctionScope;

use super::super::query::QueryAnalyzer;
use super::facts::{CaptureFacts, Captures};

mod injections;
mod scopes;
mod semantics;
mod tokens;

pub(super) fn build(language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let source = format!(
        "{}\n{}\n{}\n{}",
        include_str!("../../queries/web.scm"),
        include_str!("../../queries/typescript.scm"),
        include_str!("../../queries/astro.scm"),
        include_str!("../../queries/astro-injections.scm"),
    );
    let query = QueryAnalyzer::new(language, &source)?;
    Ok(Box::new(Analyzer {
        component_reference: query.capture_id("astro.component.reference"),
        frontmatter: query.capture_id("astro.frontmatter"),
        captures: Captures::new(&query),
        query,
    }))
}

struct Analyzer {
    query: QueryAnalyzer,
    captures: Captures,
    component_reference: Option<u32>,
    frontmatter: Option<u32>,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let root = tree.root_node();
        let root_range = root.byte_range();
        let mut facts = CaptureFacts::new(root_range.clone());
        let mut extras = semantics::Extras::default();
        let query = self
            .query
            .capture_with(tree, source, include_tokens, |_, captures, bytes| {
                facts.record(&self.captures, captures, bytes);
                extras.record(self, captures, bytes, &root_range);
            });
        let cutoff = extras.frontmatter_start.unwrap_or(root_range.start);
        let semantic = facts.finish(root);
        let mut captured = super::captured(query, semantic);

        semantics::finish(&mut extras, cutoff, &mut captured);
        injections::configure(root, source, cutoff, &mut captured.injections);
        tokens::filter(root, cutoff, &mut captured.tokens);
        captured.functions.push(FunctionScope {
            name: "<component>".into(),
            range: root_range.clone(),
            line: root.start_position().row + 1,
        });
        captured
            .definitions
            .push(semantics::component_definition(root_range.clone()));
        captured
            .exports
            .push(semantics::component_export(root_range));
        captured
            .functions
            .sort_by_key(|function| (function.range.start, function.range.end));
        Ok(captured)
    }
}
